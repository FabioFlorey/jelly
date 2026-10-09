//! Pure logical-target label decisions over a snapshot already read by the shell.
//!
//! No filesystem, global registry, CDP session or locks belong here.

use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Planned mutation of persisted labels. Apply it only after the shell's atomic
/// write succeeds, so in-memory and on-disk state preserve their ordering.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LabelUpsert {
    pub(crate) label: String,
    pub(crate) labels: HashMap<String, String>,
    pub(crate) next_tab_number: u64,
}

/// Consume an independently parsed snapshot and decide its new label state.
/// The caller remains responsible for locking, writing, and updating the live registry.
pub(crate) fn plan_label_upsert(
    mut labels: HashMap<String, String>,
    mut next_tab_number: u64,
    target_id: &str,
    main_target_hint: Option<&str>,
) -> LabelUpsert {
    let label = if let Some(label) = labels.get(target_id) {
        label.clone()
    } else {
        let used = labels.values().cloned().collect::<HashSet<_>>();
        let label = if main_target_hint == Some(target_id) && !used.contains("main") {
            "main".to_owned()
        } else {
            loop {
                let candidate = format!("tab-{next_tab_number}");
                next_tab_number = next_tab_number.saturating_add(1);
                if !used.contains(&candidate) {
                    break candidate;
                }
            }
        };
        labels.insert(target_id.to_owned(), label.clone());
        label
    };

    if let Some(number) = tab_number(&label) {
        next_tab_number = next_tab_number.max(number.saturating_add(1));
    }

    LabelUpsert {
        label,
        labels,
        next_tab_number,
    }
}

/// A destroyed target loses its mapping but never rewinds the tab counter.
pub(crate) fn plan_label_removal(
    mut labels: HashMap<String, String>,
    target_id: &str,
) -> HashMap<String, String> {
    labels.remove(target_id);
    labels
}

pub(crate) fn tab_number(label: &str) -> Option<u64> {
    label
        .strip_prefix("tab-")?
        .parse::<u64>()
        .ok()
        .filter(|n| *n >= 2)
}

/// Classify a CDP notification without reading or changing the registry.
#[derive(Debug, PartialEq)]
pub(crate) enum TargetEvent<'a> {
    Upsert {
        info: &'a Value,
        attached_session: Option<&'a str>,
    },
    Remove(&'a str),
    ClearSessionForTarget(&'a str),
    ClearSessionById(&'a str),
    Ignore,
}

pub(crate) fn classify_target_event<'a>(method: &str, params: &'a Value) -> TargetEvent<'a> {
    match method {
        "Target.targetCreated" | "Target.targetInfoChanged" => params
            .get("targetInfo")
            .map(|info| TargetEvent::Upsert {
                info,
                attached_session: None,
            })
            .unwrap_or(TargetEvent::Ignore),
        "Target.attachedToTarget" => params
            .get("targetInfo")
            .map(|info| TargetEvent::Upsert {
                info,
                attached_session: params.get("sessionId").and_then(Value::as_str),
            })
            .unwrap_or(TargetEvent::Ignore),
        "Target.targetDestroyed" => params
            .get("targetId")
            .and_then(Value::as_str)
            .map(TargetEvent::Remove)
            .unwrap_or(TargetEvent::Ignore),
        "Target.detachedFromTarget" => params
            .get("targetId")
            .and_then(Value::as_str)
            .map(TargetEvent::ClearSessionForTarget)
            .or_else(|| {
                params
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(TargetEvent::ClearSessionById)
            })
            .unwrap_or(TargetEvent::Ignore),
        "Target.targetCrashed" => params
            .get("targetId")
            .and_then(Value::as_str)
            .map(TargetEvent::ClearSessionForTarget)
            .unwrap_or(TargetEvent::Ignore),
        _ => TargetEvent::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(ids: &[(&str, &str)]) -> HashMap<String, String> {
        ids.iter()
            .map(|(id, label)| (id.to_string(), label.to_string()))
            .collect()
    }

    #[test]
    fn existing_label_is_preserved_and_the_counter_never_rewinds() {
        let plan = plan_label_upsert(labels(&[("a", "main"), ("b", "tab-9")]), 3, "b", Some("b"));
        assert_eq!(plan.label, "tab-9");
        assert_eq!(plan.next_tab_number, 10);
        assert_eq!(plan.labels["a"], "main");
    }

    #[test]
    fn hinted_main_is_used_only_if_available() {
        let main = plan_label_upsert(HashMap::new(), 2, "a", Some("a"));
        assert_eq!(main.label, "main");
        assert_eq!(main.next_tab_number, 2);
        let other = plan_label_upsert(main.labels, 2, "b", Some("b"));
        assert_eq!(other.label, "tab-2");
        assert_eq!(other.next_tab_number, 3);
    }

    #[test]
    fn new_unhinted_target_gets_next_available_monotonic_label() {
        let snapshot = labels(&[("a", "main"), ("b", "tab-2")]);
        let plan = plan_label_upsert(snapshot, 2, "c", Some("a"));
        assert_eq!(plan.label, "tab-3");
        assert_eq!(plan.next_tab_number, 4);
    }

    #[test]
    fn removing_a_target_does_not_recycle_a_tab_number() {
        let snapshot = labels(&[("a", "main"), ("b", "tab-2")]);
        let remaining = plan_label_removal(snapshot.clone(), "b");
        assert!(!remaining.contains_key("b"));
        assert_eq!(snapshot["b"], "tab-2");
        let next = plan_label_upsert(remaining, 3, "c", Some("a"));
        assert_eq!(next.label, "tab-3");
        assert_eq!(next.next_tab_number, 4);
    }

    #[test]
    fn removing_an_unknown_target_leaves_snapshot_unchanged() {
        let original = labels(&[("a", "main")]);
        assert_eq!(plan_label_removal(original.clone(), "missing"), original);
    }

    #[test]
    fn creation_and_attachment_preserve_distinct_session_binding_semantics() {
        let params = serde_json::json!({"targetInfo":{"type":"page","targetId":"b"},"sessionId":"session-b"});
        assert_eq!(
            classify_target_event("Target.targetCreated", &params),
            TargetEvent::Upsert {
                info: &params["targetInfo"],
                attached_session: None
            }
        );
        assert_eq!(
            classify_target_event("Target.attachedToTarget", &params),
            TargetEvent::Upsert {
                info: &params["targetInfo"],
                attached_session: Some("session-b")
            }
        );
        assert_eq!(
            classify_target_event("Target.targetInfoChanged", &params),
            TargetEvent::Upsert {
                info: &params["targetInfo"],
                attached_session: None
            }
        );
    }

    #[test]
    fn destruction_detach_and_crash_keep_original_target_precedence() {
        let params = serde_json::json!({"targetId":"b","sessionId":"session-b"});
        assert_eq!(
            classify_target_event("Target.targetDestroyed", &params),
            TargetEvent::Remove("b")
        );
        assert_eq!(
            classify_target_event("Target.detachedFromTarget", &params),
            TargetEvent::ClearSessionForTarget("b")
        );
        assert_eq!(
            classify_target_event("Target.targetCrashed", &params),
            TargetEvent::ClearSessionForTarget("b")
        );
        let by_session = serde_json::json!({"sessionId":"session-b"});
        assert_eq!(
            classify_target_event("Target.detachedFromTarget", &by_session),
            TargetEvent::ClearSessionById("session-b")
        );
    }

    #[test]
    fn unknown_and_incomplete_notifications_are_ignored_without_side_effects() {
        let params = serde_json::json!({});
        for method in [
            "Target.attachedToTarget",
            "Target.targetCreated",
            "Target.targetDestroyed",
            "Target.detachedFromTarget",
            "Target.targetCrashed",
            "other",
        ] {
            assert_eq!(classify_target_event(method, &params), TargetEvent::Ignore);
        }
        let malformed = serde_json::json!({"targetInfo":null});
        assert_eq!(
            classify_target_event("Target.targetCreated", &malformed),
            TargetEvent::Upsert {
                info: &Value::Null,
                attached_session: None
            }
        );
    }

    #[test]
    fn tab_numbers_require_supported_positive_labels() {
        assert_eq!(tab_number("tab-2"), Some(2));
        assert_eq!(tab_number("tab-0"), None);
        assert_eq!(tab_number("tab-1"), None);
        assert_eq!(tab_number("main"), None);
        assert_eq!(tab_number("tab-two"), None);
    }
}
