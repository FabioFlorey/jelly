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
fn event_state_owns_ring_and_subscription_lifecycle_together() {
    let mut state = EventState::default();
    let (id, cursor) = state
        .subscribe(CdpEventFilter::new(
            Some("main".into()),
            vec!["Page.loadEventFired".into()],
            Vec::new(),
        ))
        .unwrap();
    assert_eq!(cursor.sequence, 0);

    state.push(
        "Page.loadEventFired".into(),
        json!({"timestamp":1}),
        Some("session-main".into()),
        Some("target-main".into()),
        Some("main".into()),
        32,
    );

    let poll = state.poll(&id, 10).unwrap();
    assert_eq!(poll.events.len(), 1);
    assert_eq!(poll.events[0].target(), Some("main"));
    assert_eq!(poll.cursor_after.sequence, 1);

    state.unsubscribe(&id).unwrap();
    assert!(state.ensure_subscription(&id).is_err());
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
