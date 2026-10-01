use crate::{Error, ErrorKind, jelly_error, new_id};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

pub const DEFAULT_CDP_EVENT_MAX_COUNT: usize = 1024;
pub const DEFAULT_CDP_EVENT_MAX_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_CDP_EVENT_POLL_LIMIT: usize = 100;
pub const MAX_CDP_EVENT_POLL_LIMIT: usize = 500;
pub const MAX_CDP_EVENT_SUBSCRIPTIONS: usize = 128;

#[derive(Debug, Clone, PartialEq)]
pub struct CdpEvent {
    sequence: u64,
    method: String,
    params: Value,
    session_id: Option<String>,
    target_id: Option<String>,
    target: Option<String>,
    wire_bytes: usize,
}

impl CdpEvent {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn params(&self) -> &Value {
        &self.params
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn target_id(&self) -> Option<&str> {
        self.target_id.as_deref()
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn wire_bytes(&self) -> usize {
        self.wire_bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CdpEventRingStats {
    pub retained_count: usize,
    pub retained_bytes: usize,
    pub dropped: u64,
    pub latest_dropped_sequence: Option<u64>,
    pub stream_resets: u64,
    pub oldest_sequence: Option<u64>,
    pub newest_sequence: Option<u64>,
    pub next_sequence: u64,
    pub max_count: usize,
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpEventFilter {
    target: Option<String>,
    methods: Vec<String>,
    method_prefixes: Vec<String>,
}

impl CdpEventFilter {
    pub fn new(
        target: Option<String>,
        mut methods: Vec<String>,
        mut method_prefixes: Vec<String>,
    ) -> Self {
        methods.sort();
        methods.dedup();
        method_prefixes.sort();
        method_prefixes.dedup();
        Self {
            target,
            methods,
            method_prefixes,
        }
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn methods(&self) -> impl Iterator<Item = &str> {
        self.methods.iter().map(String::as_str)
    }

    pub fn method_prefixes(&self) -> impl Iterator<Item = &str> {
        self.method_prefixes.iter().map(String::as_str)
    }

    pub fn matches(&self, event: &CdpEvent) -> bool {
        if let Some(target) = self.target()
            && event.target() != Some(target)
        {
            return false;
        }

        if self.methods.is_empty() && self.method_prefixes.is_empty() {
            return true;
        }

        self.methods.iter().any(|method| method == event.method())
            || self
                .method_prefixes
                .iter()
                .any(|prefix| event.method().starts_with(prefix))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CdpEventCursor {
    pub stream: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
pub struct CdpEventPoll {
    pub events: Vec<CdpEvent>,
    pub cursor_before: CdpEventCursor,
    pub cursor_after: CdpEventCursor,
    pub cursor_lost: bool,
    pub dropped: u64,
    pub stream_resets: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
struct CdpEventSubscription {
    filter: CdpEventFilter,
    cursor: CdpEventCursor,
    dropped_seen: u64,
    stream_resets_seen: u64,
}

#[derive(Debug, Clone, Default)]
struct CdpEventSubscriptions {
    subscriptions: HashMap<String, CdpEventSubscription>,
}

impl CdpEventSubscriptions {
    pub(crate) fn subscribe(
        &mut self,
        filter: CdpEventFilter,
        stats: CdpEventRingStats,
    ) -> Result<(String, CdpEventCursor), Error> {
        if self.subscriptions.len() >= MAX_CDP_EVENT_SUBSCRIPTIONS {
            return Err(jelly_error(
                ErrorKind::ConditionFailed,
                format!(
                    "maximum active browser event subscriptions reached ({MAX_CDP_EVENT_SUBSCRIPTIONS})"
                ),
                false,
            ));
        }

        let id = new_id("event-subscription");
        let cursor = CdpEventCursor {
            stream: stats.stream_resets,
            sequence: stats.next_sequence.saturating_sub(1),
        };
        self.subscriptions.insert(
            id.clone(),
            CdpEventSubscription {
                filter,
                cursor,
                dropped_seen: stats.dropped,
                stream_resets_seen: stats.stream_resets,
            },
        );
        Ok((id, cursor))
    }

    pub(crate) fn unsubscribe(&mut self, id: &str) -> Result<(), Error> {
        if self.subscriptions.remove(id).is_none() {
            return Err(subscription_not_found(id));
        }
        Ok(())
    }

    pub(crate) fn filter(&self, id: &str) -> Result<&CdpEventFilter, Error> {
        self.subscriptions
            .get(id)
            .map(|subscription| &subscription.filter)
            .ok_or_else(|| subscription_not_found(id))
    }

    pub(crate) fn poll(
        &mut self,
        id: &str,
        ring: &CdpEventRing,
        limit: usize,
    ) -> Result<CdpEventPoll, Error> {
        if !(1..=MAX_CDP_EVENT_POLL_LIMIT).contains(&limit) {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                format!(
                    "browser event poll limit must be between 1 and {MAX_CDP_EVENT_POLL_LIMIT}"
                ),
                false,
            ));
        }

        let subscription = self
            .subscriptions
            .get_mut(id)
            .ok_or_else(|| subscription_not_found(id))?;
        let stats = ring.stats();
        let cursor_before = subscription.cursor;
        let dropped = stats.dropped.saturating_sub(subscription.dropped_seen);
        let stream_resets = stats
            .stream_resets
            .saturating_sub(subscription.stream_resets_seen);

        let mut cursor_lost = stream_resets > 0;
        let mut effective_sequence = if cursor_lost {
            stats
                .oldest_sequence
                .map(|sequence| sequence.saturating_sub(1))
                .unwrap_or_else(|| stats.next_sequence.saturating_sub(1))
        } else {
            cursor_before.sequence
        };

        if !cursor_lost
            && stats
                .latest_dropped_sequence
                .is_some_and(|sequence| sequence > cursor_before.sequence)
        {
            cursor_lost = true;
        }
        if !cursor_lost
            && let Some(oldest) = stats.oldest_sequence
            && effective_sequence.saturating_add(1) < oldest
        {
            cursor_lost = true;
        }
        if cursor_lost && stream_resets == 0 {
            effective_sequence = cursor_before.sequence;
        }

        let retained = ring.events();
        let mut matched = Vec::new();
        let mut cursor_after_sequence = effective_sequence;
        let mut stopped_at_limit = false;

        for event in &retained {
            if event.sequence() <= effective_sequence {
                continue;
            }
            cursor_after_sequence = event.sequence();
            if subscription.filter.matches(event) {
                matched.push(event.clone());
                if matched.len() == limit {
                    stopped_at_limit = true;
                    break;
                }
            }
        }

        if !stopped_at_limit {
            cursor_after_sequence = stats.next_sequence.saturating_sub(1);
        }

        let has_more = stopped_at_limit
            && retained.iter().any(|event| {
                event.sequence() > cursor_after_sequence && subscription.filter.matches(event)
            });
        if stopped_at_limit && !has_more {
            cursor_after_sequence = stats.next_sequence.saturating_sub(1);
        }

        let cursor_after = CdpEventCursor {
            stream: stats.stream_resets,
            sequence: cursor_after_sequence,
        };
        subscription.cursor = cursor_after;
        subscription.dropped_seen = stats.dropped;
        subscription.stream_resets_seen = stats.stream_resets;

        Ok(CdpEventPoll {
            events: matched,
            cursor_before,
            cursor_after,
            cursor_lost,
            dropped,
            stream_resets,
            has_more,
        })
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.subscriptions.len()
    }
}

fn subscription_not_found(id: &str) -> Error {
    jelly_error(
        ErrorKind::SubscriptionNotFound,
        format!("browser event subscription not found: {id}"),
        false,
    )
}

#[derive(Debug, Clone)]
struct CdpEventRing {
    events: VecDeque<CdpEvent>,
    retained_bytes: usize,
    dropped: u64,
    latest_dropped_sequence: Option<u64>,
    stream_resets: u64,
    next_sequence: u64,
    max_count: usize,
    max_bytes: usize,
}

impl Default for CdpEventRing {
    fn default() -> Self {
        Self::new(DEFAULT_CDP_EVENT_MAX_COUNT, DEFAULT_CDP_EVENT_MAX_BYTES)
            .expect("default CDP event ring limits must be valid")
    }
}

impl CdpEventRing {
    pub(crate) fn new(max_count: usize, max_bytes: usize) -> Result<Self, &'static str> {
        if max_count == 0 {
            return Err("CDP event ring max_count must be greater than zero");
        }
        if max_bytes == 0 {
            return Err("CDP event ring max_bytes must be greater than zero");
        }
        Ok(Self {
            events: VecDeque::new(),
            retained_bytes: 0,
            dropped: 0,
            latest_dropped_sequence: None,
            stream_resets: 0,
            next_sequence: 1,
            max_count,
            max_bytes,
        })
    }

    pub(crate) fn push(
        &mut self,
        method: String,
        params: Value,
        session_id: Option<String>,
        target_id: Option<String>,
        target: Option<String>,
        wire_bytes: usize,
    ) -> u64 {
        if self.next_sequence == u64::MAX {
            self.reset_stream();
            self.next_sequence = 1;
        }
        let sequence = self.next_sequence;
        self.next_sequence += 1;

        if wire_bytes > self.max_bytes {
            self.dropped = self.dropped.saturating_add(1);
            self.latest_dropped_sequence = Some(sequence);
            return sequence;
        }

        while self.events.len() >= self.max_count
            || self.retained_bytes.saturating_add(wire_bytes) > self.max_bytes
        {
            let Some(dropped) = self.events.pop_front() else {
                break;
            };
            self.retained_bytes = self.retained_bytes.saturating_sub(dropped.wire_bytes);
            self.dropped = self.dropped.saturating_add(1);
            self.latest_dropped_sequence = Some(dropped.sequence);
        }

        self.retained_bytes = self.retained_bytes.saturating_add(wire_bytes);
        self.events.push_back(CdpEvent {
            sequence,
            method,
            params,
            session_id,
            target_id,
            target,
            wire_bytes,
        });
        sequence
    }

    pub(crate) fn reset_stream(&mut self) {
        self.events.clear();
        self.retained_bytes = 0;
        self.latest_dropped_sequence = None;
        self.stream_resets = self.stream_resets.saturating_add(1);
    }

    pub(crate) fn events(&self) -> Vec<CdpEvent> {
        self.events.iter().cloned().collect()
    }

    pub(crate) fn stats(&self) -> CdpEventRingStats {
        CdpEventRingStats {
            retained_count: self.events.len(),
            retained_bytes: self.retained_bytes,
            dropped: self.dropped,
            latest_dropped_sequence: self.latest_dropped_sequence,
            stream_resets: self.stream_resets,
            oldest_sequence: self.events.front().map(CdpEvent::sequence),
            newest_sequence: self.events.back().map(CdpEvent::sequence),
            next_sequence: self.next_sequence,
            max_count: self.max_count,
            max_bytes: self.max_bytes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EventState {
    ring: CdpEventRing,
    subscriptions: CdpEventSubscriptions,
}

impl EventState {
    pub(crate) fn events(&self) -> Vec<CdpEvent> {
        self.ring.events()
    }

    pub(crate) fn stats(&self) -> CdpEventRingStats {
        self.ring.stats()
    }

    pub(crate) fn reset_stream(&mut self) {
        self.ring.reset_stream();
    }

    pub(crate) fn subscribe(
        &mut self,
        filter: CdpEventFilter,
    ) -> Result<(String, CdpEventCursor), Error> {
        self.subscriptions.subscribe(filter, self.ring.stats())
    }

    pub(crate) fn ensure_subscription(&self, id: &str) -> Result<(), Error> {
        self.subscriptions.filter(id).map(|_| ())
    }

    pub(crate) fn subscription_filter(&self, id: &str) -> Result<CdpEventFilter, Error> {
        self.subscriptions.filter(id).cloned()
    }

    pub(crate) fn poll(&mut self, id: &str, limit: usize) -> Result<CdpEventPoll, Error> {
        self.subscriptions.poll(id, &self.ring, limit)
    }

    pub(crate) fn unsubscribe(&mut self, id: &str) -> Result<(), Error> {
        self.subscriptions.unsubscribe(id)
    }

    pub(crate) fn push(
        &mut self,
        method: String,
        params: Value,
        session_id: Option<String>,
        target_id: Option<String>,
        target: Option<String>,
        wire_bytes: usize,
    ) -> u64 {
        self.ring
            .push(method, params, session_id, target_id, target, wire_bytes)
    }
}

#[cfg(test)]
mod tests;
