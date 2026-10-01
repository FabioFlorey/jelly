use super::*;

fn pages(ids: &[(&str, &str, &str)]) -> Vec<Value> {
    ids.iter()
        .map(|(id, title, url)| {
            json!({
                "type":"page",
                "targetId":id,
                "title":title,
                "url":url
            })
        })
        .collect()
}

#[test]
fn fresh_registry_assigns_main_and_stable_tab_numbers() {
    let registry = TargetRegistry::reconcile(
        &pages(&[
            ("b", "Second", "https://second.test"),
            ("a", "Main", "https://main.test"),
            ("c", "Third", "https://third.test"),
        ]),
        Some("a"),
        None,
    )
    .unwrap();

    assert_eq!(registry.label_for_target_id("a"), Some("main"));
    assert_eq!(registry.label_for_target_id("b"), Some("tab-2"));
    assert_eq!(registry.label_for_target_id("c"), Some("tab-3"));
    assert_eq!(
        registry
            .targets()
            .iter()
            .map(LogicalTarget::label)
            .collect::<Vec<_>>(),
        vec!["main", "tab-2", "tab-3"]
    );
}

#[test]
fn persisted_labels_survive_refresh_and_new_targets_get_monotonic_labels() {
    let first = TargetRegistry::reconcile(
        &pages(&[
            ("a", "Main", "https://main.test"),
            ("b", "Second", "https://second.test"),
        ]),
        Some("a"),
        None,
    )
    .unwrap();
    let snapshot = first.persistence_value();

    let refreshed = TargetRegistry::reconcile(
        &pages(&[
            ("c", "New", "https://new.test"),
            ("b", "Second changed", "https://second.test/2"),
            ("a", "Main changed", "https://main.test/2"),
        ]),
        Some("a"),
        Some(&snapshot),
    )
    .unwrap();

    assert_eq!(refreshed.label_for_target_id("a"), Some("main"));
    assert_eq!(refreshed.label_for_target_id("b"), Some("tab-2"));
    assert_eq!(refreshed.label_for_target_id("c"), Some("tab-3"));
    assert_eq!(refreshed.get("tab-2").unwrap().title(), "Second changed");
}

#[test]
fn closed_targets_are_removed_without_recycling_labels() {
    let snapshot = json!({
        "version":1,
        "next_tab_number":4,
        "labels":{"a":"main","b":"tab-2","c":"tab-3"}
    });
    let registry = TargetRegistry::reconcile(
        &pages(&[
            ("a", "Main", "https://main.test"),
            ("d", "New", "https://new.test"),
        ]),
        Some("a"),
        Some(&snapshot),
    )
    .unwrap();

    assert!(registry.get("tab-2").is_none());
    assert!(registry.get("tab-3").is_none());
    assert_eq!(registry.label_for_target_id("d"), Some("tab-4"));
}

#[test]
fn persistence_never_contains_session_ids() {
    let mut registry = TargetRegistry::reconcile(
        &pages(&[("a", "Main", "https://main.test")]),
        Some("a"),
        None,
    )
    .unwrap();
    registry.set_session("a", "session-secret".into()).unwrap();

    let persisted = registry.persistence_value();
    assert!(!persisted.to_string().contains("session-secret"));
    assert!(registry.get("main").unwrap().session_id().is_some());
}

#[test]
fn session_bindings_are_nonempty_and_unique() {
    let mut registry = TargetRegistry::reconcile(
        &pages(&[
            ("a", "Main", "https://main.test"),
            ("b", "Second", "https://second.test"),
        ]),
        Some("a"),
        None,
    )
    .unwrap();

    registry.set_session("a", "session-a".into()).unwrap();
    let duplicate = registry.set_session("b", "session-a".into()).unwrap_err();
    assert_eq!(
        crate::classify_error(duplicate.as_ref()),
        (ErrorKind::Internal, false)
    );
    assert!(duplicate.to_string().contains("already bound to target a"));

    let empty = registry.set_session("b", "   ".into()).unwrap_err();
    assert_eq!(
        crate::classify_error(empty.as_ref()),
        (ErrorKind::Internal, false)
    );
    assert!(empty.to_string().contains("must not be empty"));

    registry.set_session("b", "session-b".into()).unwrap();
    assert_eq!(registry.target_id_for_session("session-a"), Some("a"));
    assert_eq!(registry.target_id_for_session("session-b"), Some("b"));
}

#[test]
fn compatibility_resolution_prefers_exact_logical_label_then_legacy_fields() {
    let registry = TargetRegistry::reconcile(
        &pages(&[
            ("a", "Main page", "https://main.test"),
            ("b", "Invoice", "https://example.test/invoice"),
        ]),
        Some("a"),
        None,
    )
    .unwrap();

    assert_eq!(registry.resolve_compat("tab-2").unwrap().target_id(), "b");
    assert_eq!(registry.resolve_compat("a").unwrap().label(), "main");
    assert_eq!(registry.resolve_compat("Invoice").unwrap().label(), "tab-2");
    assert_eq!(
        registry.resolve_compat("/invoice").unwrap().label(),
        "tab-2"
    );
}

#[test]
fn duplicate_or_malformed_persisted_labels_fail_loudly() {
    for snapshot in [
        json!({
            "version":1,
            "next_tab_number":3,
            "labels":{"a":"main","b":"main"}
        }),
        json!({
            "version":1,
            "next_tab_number":3,
            "labels":{"a":"login"}
        }),
        json!({
            "version":99,
            "next_tab_number":3,
            "labels":{}
        }),
    ] {
        assert!(
            TargetRegistry::reconcile(
                &pages(&[("a", "Main", "https://main.test")]),
                Some("a"),
                Some(&snapshot)
            )
            .is_err()
        );
    }
}

#[test]
fn target_notifications_update_registry_and_persisted_identity_incrementally() {
    let nonce = TEMP_FILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jelly-target-events-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let state = dir.join("logical_targets.json");
    let lock = dir.join("logical_targets.lock");
    let state = state.to_string_lossy().into_owned();
    let lock = lock.to_string_lossy().into_owned();

    let mut registry = TargetRegistry::reconcile_persisted(
        &pages(&[("a", "Main", "https://main.test")]),
        Some("a"),
        &state,
        &lock,
    )
    .unwrap();

    registry
        .apply_target_notification_persisted(
            "Target.targetCreated",
            &json!({
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"Secondary",
                    "url":"https://secondary.test"
                }
            }),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.label_for_target_id("b"), Some("tab-2"));

    registry
        .apply_target_notification_persisted(
            "Target.targetInfoChanged",
            &json!({
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"Secondary updated",
                    "url":"https://secondary.test/updated"
                }
            }),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.get("tab-2").unwrap().title(), "Secondary updated");

    registry
        .apply_target_notification_persisted(
            "Target.attachedToTarget",
            &json!({
                "sessionId":"session-b",
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"Secondary updated",
                    "url":"https://secondary.test/updated"
                }
            }),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.target_id_for_session("session-b"), Some("b"));
    assert_eq!(registry.session_id_for_label("tab-2"), Some("session-b"));

    registry
        .apply_target_notification_persisted(
            "Target.targetInfoChanged",
            &json!({
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"Secondary navigated",
                    "url":"https://secondary.test/navigated"
                }
            }),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.session_id_for_label("tab-2"), Some("session-b"));
    assert_eq!(
        registry.get("tab-2").unwrap().title(),
        "Secondary navigated"
    );

    registry
        .apply_target_notification_persisted(
            "Target.targetCrashed",
            &json!({"targetId":"b","status":"crashed","errorCode":-1}),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert!(registry.get("tab-2").is_some());
    assert_eq!(registry.session_id_for_label("tab-2"), None);

    registry
        .apply_target_notification_persisted(
            "Target.attachedToTarget",
            &json!({
                "sessionId":"session-b2",
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"Secondary recovered",
                    "url":"https://secondary.test/recovered"
                }
            }),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.session_id_for_label("tab-2"), Some("session-b2"));

    registry
        .apply_target_notification_persisted(
            "Target.detachedFromTarget",
            &json!({"sessionId":"session-b2","targetId":"b"}),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert_eq!(registry.target_id_for_session("session-b2"), None);
    assert_eq!(registry.session_id_for_label("tab-2"), None);

    registry
        .apply_target_notification_persisted(
            "Target.targetDestroyed",
            &json!({"targetId":"b"}),
            Some("a"),
            &state,
            &lock,
        )
        .unwrap();
    assert!(registry.get_by_target_id("b").is_none());

    let persisted: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(persisted["labels"]["a"], "main");
    assert!(persisted["labels"].get("b").is_none());
    assert_eq!(persisted["next_tab_number"], 3);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_incremental_updates_do_not_lose_persisted_labels() {
    let nonce = TEMP_FILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "jelly-target-concurrency-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let state = dir
        .join("logical_targets.json")
        .to_string_lossy()
        .into_owned();
    let lock = dir
        .join("logical_targets.lock")
        .to_string_lossy()
        .into_owned();

    let base = TargetRegistry::reconcile_persisted(
        &pages(&[("a", "Main", "https://main.test")]),
        Some("a"),
        &state,
        &lock,
    )
    .unwrap();

    let left_state = state.clone();
    let left_lock = lock.clone();
    let mut left = base.clone();
    let left_thread = std::thread::spawn(move || {
        left.apply_target_notification_persisted(
            "Target.targetCreated",
            &json!({
                "targetInfo":{
                    "type":"page",
                    "targetId":"b",
                    "title":"B",
                    "url":"https://b.test"
                }
            }),
            Some("a"),
            &left_state,
            &left_lock,
        )
        .unwrap();
    });

    let right_state = state.clone();
    let right_lock = lock.clone();
    let mut right = base;
    let right_thread = std::thread::spawn(move || {
        right
            .apply_target_notification_persisted(
                "Target.targetCreated",
                &json!({
                    "targetInfo":{
                        "type":"page",
                        "targetId":"c",
                        "title":"C",
                        "url":"https://c.test"
                    }
                }),
                Some("a"),
                &right_state,
                &right_lock,
            )
            .unwrap();
    });

    left_thread.join().unwrap();
    right_thread.join().unwrap();

    let persisted: Value = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(persisted["labels"]["a"], "main");
    let b = persisted["labels"]["b"].as_str().unwrap();
    let c = persisted["labels"]["c"].as_str().unwrap();
    assert_ne!(b, c);
    assert!(matches!((b, c), ("tab-2", "tab-3") | ("tab-3", "tab-2")));
    assert_eq!(persisted["next_tab_number"], 4);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn non_page_targets_do_not_enter_the_registry() {
    let mut infos = pages(&[("a", "Main", "https://main.test")]);
    infos.push(json!({
        "type":"service_worker",
        "targetId":"worker",
        "title":"",
        "url":"https://main.test/sw.js"
    }));
    let registry = TargetRegistry::reconcile(&infos, Some("a"), None).unwrap();
    assert_eq!(registry.targets().len(), 1);
    assert!(registry.get_by_target_id("worker").is_none());
}
