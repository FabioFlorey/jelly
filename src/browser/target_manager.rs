use super::{LogicalTarget, TargetRegistry};
use crate::{
    ACTIVE_TARGET, Error, ErrorKind, LOGICAL_TARGETS, LOGICAL_TARGETS_LOCK, PAGE_TARGET,
    jelly_error,
};
use serde_json::Value;
use std::fs;

pub(crate) struct TargetManager {
    registry: TargetRegistry,
    active_target_id: String,
    active_session_id: String,
}

impl TargetManager {
    pub(crate) fn from_initial_targets(target_infos: &[Value]) -> Result<Self, Error> {
        let page_hint = read_state_id(PAGE_TARGET);
        let active_hint = read_state_id(ACTIVE_TARGET);
        let registry = TargetRegistry::reconcile_persisted(
            target_infos,
            page_hint.as_deref(),
            LOGICAL_TARGETS,
            LOGICAL_TARGETS_LOCK,
        )?;

        let active_target_id = active_hint
            .as_deref()
            .filter(|id| registry.get_by_target_id(id).is_some())
            .or_else(|| {
                page_hint
                    .as_deref()
                    .filter(|id| registry.get_by_target_id(id).is_some())
            })
            .or_else(|| registry.get("main").map(LogicalTarget::target_id))
            .or_else(|| registry.targets().first().map(LogicalTarget::target_id))
            .ok_or_else(|| target_not_found("no web page target"))?
            .to_owned();

        Ok(Self {
            registry,
            active_target_id,
            active_session_id: String::new(),
        })
    }

    pub(crate) fn active_target_id(&self) -> &str {
        &self.active_target_id
    }

    pub(crate) fn active_session_id(&self) -> &str {
        &self.active_session_id
    }

    pub(crate) fn current_label(&self) -> Option<&str> {
        self.registry.label_for_target_id(&self.active_target_id)
    }

    pub(crate) fn targets(&self) -> &[LogicalTarget] {
        self.registry.targets()
    }

    pub(crate) fn resolve_compat(&self, query: &str) -> Option<&LogicalTarget> {
        self.registry.resolve_compat(query)
    }

    pub(crate) fn target_id_for_label(&self, label: &str) -> Option<&str> {
        self.registry.target_id_for_label(label)
    }

    pub(crate) fn label_for_target_id(&self, target_id: &str) -> Option<&str> {
        self.registry.label_for_target_id(target_id)
    }

    pub(crate) fn target_id_for_session(&self, session_id: &str) -> Option<&str> {
        self.registry.target_id_for_session(session_id)
    }

    pub(crate) fn session_id_for_target(&self, target_id: &str) -> Option<&str> {
        self.registry.session_id_for_target(target_id)
    }

    pub(crate) fn contains_target(&self, target_id: &str) -> bool {
        self.registry.get_by_target_id(target_id).is_some()
    }

    pub(crate) fn set_session(&mut self, target_id: &str, session_id: String) -> Result<(), Error> {
        self.registry.set_session(target_id, session_id)
    }

    pub(crate) fn activate(&mut self, target_id: &str, session_id: String) -> Result<(), Error> {
        if !self.contains_target(target_id) {
            return Err(target_not_found(format!(
                "browser target not found: {target_id}"
            )));
        }
        self.registry.set_session(target_id, session_id.clone())?;
        self.active_target_id = target_id.to_owned();
        self.active_session_id = session_id;
        fs::write(ACTIVE_TARGET, target_id)?;
        Ok(())
    }

    pub(crate) fn persist_current_target(&self) -> Result<(), Error> {
        fs::write(ACTIVE_TARGET, &self.active_target_id)?;
        Ok(())
    }

    pub(crate) fn external_active_target(&self) -> Option<String> {
        read_state_id(ACTIVE_TARGET).filter(|target| target != &self.active_target_id)
    }

    pub(crate) fn reconcile(&mut self, target_infos: &[Value]) -> Result<(), Error> {
        let sessions = self
            .registry
            .targets()
            .iter()
            .filter_map(|target| {
                target
                    .session_id()
                    .map(|session_id| (target.target_id().to_owned(), session_id.to_owned()))
            })
            .collect::<Vec<_>>();

        let page_hint = read_state_id(PAGE_TARGET);
        let mut registry = TargetRegistry::reconcile_persisted(
            target_infos,
            page_hint.as_deref(),
            LOGICAL_TARGETS,
            LOGICAL_TARGETS_LOCK,
        )?;

        for (target_id, session_id) in sessions {
            if registry.get_by_target_id(&target_id).is_some() {
                registry.set_session(&target_id, session_id)?;
            }
        }

        if !self.active_session_id.is_empty()
            && registry.get_by_target_id(&self.active_target_id).is_some()
        {
            registry.set_session(&self.active_target_id, self.active_session_id.clone())?;
        }

        self.registry = registry;
        Ok(())
    }

    pub(crate) fn apply_notification(&mut self, method: &str, params: &Value) -> Result<(), Error> {
        let page_hint = read_state_id(PAGE_TARGET);
        self.registry.apply_target_notification_persisted(
            method,
            params,
            page_hint.as_deref(),
            LOGICAL_TARGETS,
            LOGICAL_TARGETS_LOCK,
        )
    }
}

fn read_state_id(path: &str) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn target_not_found(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::TargetNotFound, message, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn targets() -> Vec<Value> {
        vec![
            json!({"type":"page","targetId":"a","title":"A","url":"https://a.test/"}),
            json!({"type":"page","targetId":"b","title":"B","url":"https://b.test/"}),
        ]
    }

    #[test]
    fn manager_tracks_active_session_without_exposing_registry_mutation() {
        let registry = TargetRegistry::reconcile(&targets(), Some("a"), None).unwrap();
        let mut manager = TargetManager {
            registry,
            active_target_id: "a".into(),
            active_session_id: String::new(),
        };

        manager.set_session("a", "session-a".into()).unwrap();
        manager.activate("a", "session-a".into()).unwrap();

        assert_eq!(manager.active_target_id(), "a");
        assert_eq!(manager.active_session_id(), "session-a");
        assert_eq!(manager.current_label(), Some("main"));
        assert_eq!(manager.session_id_for_target("a"), Some("session-a"));
    }

    #[test]
    fn reconcile_preserves_known_sessions_for_live_targets() {
        let registry = TargetRegistry::reconcile(&targets(), Some("a"), None).unwrap();
        let mut manager = TargetManager {
            registry,
            active_target_id: "a".into(),
            active_session_id: "session-a".into(),
        };
        manager.set_session("a", "session-a".into()).unwrap();
        manager.set_session("b", "session-b".into()).unwrap();

        manager.reconcile(&targets()).unwrap();

        assert_eq!(manager.session_id_for_target("a"), Some("session-a"));
        assert_eq!(manager.session_id_for_target("b"), Some("session-b"));
    }
}
