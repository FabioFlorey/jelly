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
pub(crate) struct CdpEventSubscriptions {
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
pub(crate) struct CdpEventRing {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn push(ring: &mut CdpEventRing, method: &str, bytes: usize) -> u64 {
        push_target(ring, method, "main", bytes)
    }

    fn push_target(ring: &mut CdpEventRing, method: &str, target: &str, bytes: usize) -> u64 {
        ring.push(
            method.to_owned(),
            json!({"method":method}),
            Some(format!("session-{target}")),
            Some(format!("target-id-{target}")),
            Some(target.to_owned()),
            bytes,
        )
    }

    #[test]
    fn subscriptions_start_at_now_and_filter_by_target_exact_method_or_prefix() {
        let mut ring = CdpEventRing::new(16, 4096).unwrap();
        push(&mut ring, "Runtime.executionContextCreated", 10);

        let mut subscriptions = CdpEventSubscriptions::default();
        let filter = CdpEventFilter::new(
            Some("main".into()),
            vec!["Page.loadEventFired".into()],
            vec!["Network.webSocket".into()],
        );
        let (id, cursor) = subscriptions.subscribe(filter, ring.stats()).unwrap();
        assert_eq!(subscriptions.len(), 1);
        assert_eq!(cursor.sequence, 1);

        push_target(&mut ring, "Page.loadEventFired", "main", 10);
        push_target(&mut ring, "Page.loadEventFired", "tab-2", 10);
        push_target(&mut ring, "Network.webSocketCreated", "main", 10);
        push_target(&mut ring, "Runtime.consoleAPICalled", "main", 10);

        let poll = subscriptions.poll(&id, &ring, 10).unwrap();
        assert_eq!(
            poll.events.iter().map(CdpEvent::method).collect::<Vec<_>>(),
            vec!["Page.loadEventFired", "Network.webSocketCreated"]
        );
        assert_eq!(poll.cursor_before.sequence, 1);
        assert_eq!(poll.cursor_after.sequence, 5);
        assert!(!poll.cursor_lost);
        assert_eq!(poll.dropped, 0);
        assert_eq!(poll.stream_resets, 0);
        assert!(!poll.has_more);
    }

    #[test]
    fn poll_limit_advances_cursor_to_last_scanned_match_and_reports_more() {
        let mut ring = CdpEventRing::new(16, 4096).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), vec!["Page.".into()]),
                ring.stats(),
            )
            .unwrap();

        push(&mut ring, "Runtime.consoleAPICalled", 10);
        push(&mut ring, "Page.frameStartedLoading", 10);
        push(&mut ring, "Runtime.bindingCalled", 10);
        push(&mut ring, "Page.loadEventFired", 10);

        let first = subscriptions.poll(&id, &ring, 1).unwrap();
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].sequence(), 2);
        assert_eq!(first.cursor_after.sequence, 2);
        assert!(first.has_more);

        let second = subscriptions.poll(&id, &ring, 1).unwrap();
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].sequence(), 4);
        assert_eq!(second.cursor_after.sequence, 4);
        assert!(!second.has_more);

        let third = subscriptions.poll(&id, &ring, 1).unwrap();
        assert!(third.events.is_empty());
        assert_eq!(third.cursor_before.sequence, 4);
        assert_eq!(third.cursor_after.sequence, 4);
    }

    #[test]
    fn polls_with_no_matches_still_advance_over_observed_stream_data() {
        let mut ring = CdpEventRing::new(16, 4096).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, vec!["Page.loadEventFired".into()], Vec::new()),
                ring.stats(),
            )
            .unwrap();

        push(&mut ring, "Runtime.consoleAPICalled", 10);
        push(&mut ring, "Network.requestWillBeSent", 10);

        let first = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(first.events.is_empty());
        assert_eq!(first.cursor_after.sequence, 2);

        let second = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(second.events.is_empty());
        assert_eq!(second.cursor_before.sequence, 2);
        assert_eq!(second.cursor_after.sequence, 2);
    }

    #[test]
    fn ring_eviction_marks_cursor_loss_and_reports_drop_delta_once() {
        let mut ring = CdpEventRing::new(2, 4096).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();

        push(&mut ring, "Page.one", 10);
        push(&mut ring, "Page.two", 10);
        push(&mut ring, "Page.three", 10);

        let poll = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(poll.cursor_lost);
        assert_eq!(poll.dropped, 1);
        assert_eq!(
            poll.events
                .iter()
                .map(CdpEvent::sequence)
                .collect::<Vec<_>>(),
            vec![2, 3]
        );

        let next = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(!next.cursor_lost);
        assert_eq!(next.dropped, 0);
    }

    #[test]
    fn oversized_future_event_marks_cursor_loss_and_advances_past_the_gap() {
        let mut ring = CdpEventRing::new(8, 16).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();

        push(&mut ring, "Page.tooLarge", 17);
        let first = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(first.cursor_lost);
        assert_eq!(first.dropped, 1);
        assert!(first.events.is_empty());
        assert_eq!(first.cursor_after.sequence, 1);

        push(&mut ring, "Page.next", 10);
        let second = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(!second.cursor_lost);
        assert_eq!(second.dropped, 0);
        assert_eq!(second.events.len(), 1);
        assert_eq!(second.events[0].sequence(), 2);
    }

    #[test]
    fn eviction_of_already_consumed_history_does_not_falsely_lose_cursor() {
        let mut ring = CdpEventRing::new(2, 4096).unwrap();
        push(&mut ring, "Page.one", 10);
        push(&mut ring, "Page.two", 10);

        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, cursor) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();
        assert_eq!(cursor.sequence, 2);

        push(&mut ring, "Page.three", 10);
        let poll = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(!poll.cursor_lost);
        assert_eq!(poll.dropped, 1);
        assert_eq!(poll.events.len(), 1);
        assert_eq!(poll.events[0].sequence(), 3);
    }

    #[test]
    fn stream_reset_marks_cursor_loss_and_moves_subscription_to_new_stream() {
        let mut ring = CdpEventRing::new(8, 4096).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();

        push(&mut ring, "Page.beforeReset", 10);
        ring.reset_stream();
        push(&mut ring, "Page.afterReset", 10);

        let poll = subscriptions.poll(&id, &ring, 10).unwrap();
        assert!(poll.cursor_lost);
        assert_eq!(poll.stream_resets, 1);
        assert_eq!(poll.cursor_before.stream, 0);
        assert_eq!(poll.cursor_after.stream, 1);
        assert_eq!(poll.events.len(), 1);
        assert_eq!(poll.events[0].method(), "Page.afterReset");
    }

    #[test]
    fn subscription_registry_is_session_local_and_bounded() {
        let ring = CdpEventRing::new(8, 4096).unwrap();
        let mut first = CdpEventSubscriptions::default();
        let (id, _) = first
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();

        let mut independent = CdpEventSubscriptions::default();
        let error = independent.poll(&id, &ring, 10).unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::SubscriptionNotFound, false)
        );

        for _ in 0..MAX_CDP_EVENT_SUBSCRIPTIONS {
            independent
                .subscribe(
                    CdpEventFilter::new(None, Vec::new(), Vec::new()),
                    ring.stats(),
                )
                .unwrap();
        }
        let error = independent
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (ErrorKind::ConditionFailed, false)
        );
    }

    #[test]
    fn unsubscribe_is_final_and_unknown_ids_have_a_typed_error() {
        let ring = CdpEventRing::new(8, 4096).unwrap();
        let mut subscriptions = CdpEventSubscriptions::default();
        let (id, _) = subscriptions
            .subscribe(
                CdpEventFilter::new(None, Vec::new(), Vec::new()),
                ring.stats(),
            )
            .unwrap();
        subscriptions.unsubscribe(&id).unwrap();
        assert_eq!(subscriptions.len(), 0);

        for error in [
            subscriptions.unsubscribe(&id).unwrap_err(),
            subscriptions.poll(&id, &ring, 10).unwrap_err(),
        ] {
            assert_eq!(
                crate::classify_error(error.as_ref()),
                (ErrorKind::SubscriptionNotFound, false)
            );
        }
    }

    #[test]
    fn sequence_is_monotonic_and_metadata_is_retained() {
        let mut ring = CdpEventRing::new(4, 1024).unwrap();
        assert_eq!(push(&mut ring, "Page.loadEventFired", 10), 1);
        assert_eq!(push(&mut ring, "Network.requestWillBeSent", 20), 2);

        let events = ring.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence(), 1);
        assert_eq!(events[1].sequence(), 2);
        assert_eq!(events[1].session_id(), Some("session-main"));
        assert_eq!(events[1].target_id(), Some("target-id-main"));
        assert_eq!(events[1].target(), Some("main"));
        assert_eq!(events[1].wire_bytes(), 20);

        let stats = ring.stats();
        assert_eq!(stats.retained_count, 2);
        assert_eq!(stats.retained_bytes, 30);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.oldest_sequence, Some(1));
        assert_eq!(stats.newest_sequence, Some(2));
        assert_eq!(stats.next_sequence, 3);
    }

    #[test]
    fn count_overflow_evicts_oldest_and_reports_drops() {
        let mut ring = CdpEventRing::new(2, 1024).unwrap();
        push(&mut ring, "A.one", 10);
        push(&mut ring, "B.two", 10);
        push(&mut ring, "C.three", 10);

        let events = ring.events();
        assert_eq!(
            events.iter().map(CdpEvent::sequence).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(ring.stats().dropped, 1);
    }

    #[test]
    fn byte_overflow_evicts_until_new_event_fits() {
        let mut ring = CdpEventRing::new(8, 25).unwrap();
        push(&mut ring, "A.one", 10);
        push(&mut ring, "B.two", 10);
        push(&mut ring, "C.three", 12);

        let events = ring.events();
        assert_eq!(
            events.iter().map(CdpEvent::sequence).collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(ring.stats().retained_bytes, 22);
        assert_eq!(ring.stats().dropped, 1);
    }

    #[test]
    fn oversized_single_event_is_explicitly_dropped_but_consumes_sequence() {
        let mut ring = CdpEventRing::new(8, 16).unwrap();
        assert_eq!(push(&mut ring, "Huge.event", 17), 1);
        assert!(ring.events().is_empty());
        let stats = ring.stats();
        assert_eq!(stats.dropped, 1);
        assert_eq!(stats.next_sequence, 2);
    }

    #[test]
    fn reset_preserves_sequence_monotonicity_and_marks_loss_of_continuity() {
        let mut ring = CdpEventRing::new(8, 1024).unwrap();
        push(&mut ring, "A.one", 10);
        push(&mut ring, "B.two", 10);
        ring.reset_stream();

        let stats = ring.stats();
        assert_eq!(stats.retained_count, 0);
        assert_eq!(stats.retained_bytes, 0);
        assert_eq!(stats.dropped, 0);
        assert_eq!(stats.stream_resets, 1);
        assert_eq!(stats.next_sequence, 3);

        assert_eq!(push(&mut ring, "C.three", 10), 3);
    }

    #[test]
    fn sequence_rollover_starts_a_new_stream_instead_of_reusing_a_sequence_silently() {
        let mut ring = CdpEventRing::new(8, 1024).unwrap();
        ring.next_sequence = u64::MAX;
        assert_eq!(push(&mut ring, "After.rollover", 10), 1);
        let stats = ring.stats();
        assert_eq!(stats.stream_resets, 1);
        assert_eq!(stats.next_sequence, 2);
        assert_eq!(stats.oldest_sequence, Some(1));
    }

    #[test]
    fn zero_limits_are_rejected() {
        assert!(CdpEventRing::new(0, 1024).is_err());
        assert!(CdpEventRing::new(8, 0).is_err());
    }
}
