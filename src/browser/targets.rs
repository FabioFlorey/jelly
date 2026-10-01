use crate::{Error, ErrorKind, jelly_error};
use serde_json::{Map, Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_FILE_NONCE: AtomicU64 = AtomicU64::new(1);

const SNAPSHOT_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalTarget {
    label: String,
    target_id: String,
    session_id: Option<String>,
    title: String,
    url: String,
}

impl LogicalTarget {
    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

#[derive(Debug, Clone)]
pub struct TargetRegistry {
    targets: Vec<LogicalTarget>,
    next_tab_number: u64,
}

impl TargetRegistry {
    pub fn reconcile_persisted(
        target_infos: &[Value],
        main_target_hint: Option<&str>,
        state_path: &str,
        lock_path: &str,
    ) -> Result<Self, Error> {
        let _lock = RegistryFileLock::acquire(lock_path)?;
        let snapshot = read_snapshot(state_path)?;
        let registry = Self::reconcile(target_infos, main_target_hint, snapshot.as_ref())?;
        write_snapshot_atomic(state_path, &registry.persistence_value())?;
        Ok(registry)
    }

    pub fn reconcile(
        target_infos: &[Value],
        main_target_hint: Option<&str>,
        snapshot: Option<&Value>,
    ) -> Result<Self, Error> {
        let mut live = target_infos
            .iter()
            .filter(|target| target["type"] == "page")
            .map(parse_page_target)
            .collect::<Result<Vec<_>, _>>()?;
        live.sort_by(|left, right| left.target_id.cmp(&right.target_id));

        let mut persisted = HashMap::<String, String>::new();
        let mut next_tab_number = 2_u64;
        if let Some(snapshot) = snapshot {
            let (labels, next) = parse_snapshot(snapshot)?;
            persisted = labels;
            next_tab_number = next;
        }

        let live_ids = live
            .iter()
            .map(|target| target.target_id.as_str())
            .collect::<HashSet<_>>();
        persisted.retain(|target_id, _| live_ids.contains(target_id.as_str()));

        validate_unique_labels(&persisted)?;

        let mut used_labels = persisted.values().cloned().collect::<HashSet<_>>();
        let main_hint_live = main_target_hint.filter(|hint| live_ids.contains(*hint));

        if let Some(main_target) = main_hint_live
            && !persisted.contains_key(main_target)
            && !used_labels.contains("main")
        {
            persisted.insert(main_target.to_owned(), "main".to_owned());
            used_labels.insert("main".to_owned());
        }

        if persisted.is_empty() && !live.is_empty() {
            let main_target = main_hint_live.unwrap_or(&live[0].target_id);
            persisted.insert(main_target.to_owned(), "main".to_owned());
            used_labels.insert("main".to_owned());
        }

        for target in &live {
            if persisted.contains_key(&target.target_id) {
                continue;
            }
            let label = loop {
                let candidate = format!("tab-{next_tab_number}");
                next_tab_number += 1;
                if !used_labels.contains(&candidate) {
                    break candidate;
                }
            };
            persisted.insert(target.target_id.clone(), label.clone());
            used_labels.insert(label);
        }

        for label in persisted.values() {
            if let Some(number) = tab_number(label) {
                next_tab_number = next_tab_number.max(number.saturating_add(1));
            }
        }

        let mut targets = live
            .into_iter()
            .map(|target| LogicalTarget {
                label: persisted
                    .get(&target.target_id)
                    .expect("every live target must have a logical label")
                    .clone(),
                target_id: target.target_id,
                session_id: None,
                title: target.title,
                url: target.url,
            })
            .collect::<Vec<_>>();
        targets.sort_by(logical_target_order);

        let registry = Self {
            targets,
            next_tab_number,
        };
        registry.validate()?;
        Ok(registry)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let mut labels = HashSet::with_capacity(self.targets.len());
        let mut target_ids = HashSet::with_capacity(self.targets.len());
        let mut session_ids = HashSet::with_capacity(self.targets.len());

        for target in &self.targets {
            validate_logical_label(&target.label)?;
            if target.target_id.trim().is_empty() {
                return Err(internal(
                    "logical target registry contains an empty target ID",
                ));
            }
            if !labels.insert(target.label.as_str()) {
                return Err(internal(format!(
                    "logical target registry contains duplicate label {}",
                    target.label
                )));
            }
            if !target_ids.insert(target.target_id.as_str()) {
                return Err(internal(format!(
                    "logical target registry contains duplicate target ID {}",
                    target.target_id
                )));
            }
            if let Some(session_id) = target.session_id() {
                if session_id.trim().is_empty() {
                    return Err(internal(
                        "logical target registry contains an empty CDP session ID",
                    ));
                }
                if !session_ids.insert(session_id) {
                    return Err(internal(format!(
                        "logical target registry contains duplicate CDP session ID {session_id}"
                    )));
                }
            }
        }

        if self.next_tab_number < 2 {
            return Err(internal(
                "logical target registry next_tab_number must be at least 2",
            ));
        }
        Ok(())
    }

    pub fn targets(&self) -> &[LogicalTarget] {
        &self.targets
    }

    pub fn get(&self, label: &str) -> Option<&LogicalTarget> {
        self.targets.iter().find(|target| target.label == label)
    }

    pub fn get_by_target_id(&self, target_id: &str) -> Option<&LogicalTarget> {
        self.targets
            .iter()
            .find(|target| target.target_id == target_id)
    }

    pub fn target_id_for_label(&self, label: &str) -> Option<&str> {
        self.get(label).map(LogicalTarget::target_id)
    }

    pub fn label_for_target_id(&self, target_id: &str) -> Option<&str> {
        self.get_by_target_id(target_id).map(LogicalTarget::label)
    }

    pub fn target_id_for_session(&self, session_id: &str) -> Option<&str> {
        self.targets
            .iter()
            .find(|target| target.session_id() == Some(session_id))
            .map(LogicalTarget::target_id)
    }

    pub fn session_id_for_target(&self, target_id: &str) -> Option<&str> {
        self.get_by_target_id(target_id)
            .and_then(LogicalTarget::session_id)
    }

    pub fn session_id_for_label(&self, label: &str) -> Option<&str> {
        self.get(label).and_then(LogicalTarget::session_id)
    }

    pub fn apply_target_notification_persisted(
        &mut self,
        method: &str,
        params: &Value,
        main_target_hint: Option<&str>,
        state_path: &str,
        lock_path: &str,
    ) -> Result<(), Error> {
        match method {
            "Target.targetCreated" | "Target.targetInfoChanged" => {
                if let Some(target_info) = params.get("targetInfo") {
                    self.upsert_target_info_persisted(
                        target_info,
                        main_target_hint,
                        state_path,
                        lock_path,
                    )?;
                }
            }
            "Target.attachedToTarget" => {
                if let Some(target_info) = params.get("targetInfo") {
                    self.upsert_target_info_persisted(
                        target_info,
                        main_target_hint,
                        state_path,
                        lock_path,
                    )?;
                    if let (Some(target_id), Some(session_id)) = (
                        target_info.get("targetId").and_then(Value::as_str),
                        params.get("sessionId").and_then(Value::as_str),
                    ) && self.get_by_target_id(target_id).is_some()
                    {
                        self.set_session(target_id, session_id.to_owned())?;
                    }
                }
            }
            "Target.targetDestroyed" => {
                if let Some(target_id) = params.get("targetId").and_then(Value::as_str) {
                    self.remove_target_persisted(target_id, state_path, lock_path)?;
                }
            }
            "Target.detachedFromTarget" => {
                if let Some(target_id) = params.get("targetId").and_then(Value::as_str) {
                    self.clear_session_for_target(target_id);
                } else if let Some(session_id) = params.get("sessionId").and_then(Value::as_str) {
                    self.clear_session_by_id(session_id);
                }
            }
            "Target.targetCrashed" => {
                if let Some(target_id) = params.get("targetId").and_then(Value::as_str) {
                    self.clear_session_for_target(target_id);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn set_session(&mut self, target_id: &str, session_id: String) -> Result<(), Error> {
        if session_id.trim().is_empty() {
            return Err(internal("CDP session ID must not be empty"));
        }
        if let Some(existing) = self.target_id_for_session(&session_id)
            && existing != target_id
        {
            return Err(internal(format!(
                "CDP session ID {session_id} is already bound to target {existing}"
            )));
        }

        let target = self
            .targets
            .iter_mut()
            .find(|target| target.target_id == target_id)
            .ok_or_else(|| {
                jelly_error(
                    ErrorKind::TargetNotFound,
                    format!("target ID is not present in logical registry: {target_id}"),
                    true,
                )
            })?;
        target.session_id = Some(session_id);
        self.validate()
    }

    pub fn clear_sessions(&mut self) {
        for target in &mut self.targets {
            target.session_id = None;
        }
    }

    fn clear_session_for_target(&mut self, target_id: &str) {
        if let Some(target) = self
            .targets
            .iter_mut()
            .find(|target| target.target_id == target_id)
        {
            target.session_id = None;
        }
    }

    fn clear_session_by_id(&mut self, session_id: &str) {
        if let Some(target) = self
            .targets
            .iter_mut()
            .find(|target| target.session_id() == Some(session_id))
        {
            target.session_id = None;
        }
    }

    fn upsert_target_info_persisted(
        &mut self,
        target_info: &Value,
        main_target_hint: Option<&str>,
        state_path: &str,
        lock_path: &str,
    ) -> Result<(), Error> {
        let target_type = target_info
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let target_id = target_info
            .get("targetId")
            .and_then(Value::as_str)
            .ok_or_else(|| internal("Target notification targetInfo is missing string targetId"))?;

        if target_type != "page" {
            if self.get_by_target_id(target_id).is_some() {
                self.remove_target_persisted(target_id, state_path, lock_path)?;
            }
            return Ok(());
        }

        let page = parse_page_target(target_info)?;
        let _lock = RegistryFileLock::acquire(lock_path)?;
        let base = read_snapshot(state_path)?.unwrap_or_else(|| self.persistence_value());
        let (mut labels, mut next_tab_number) = parse_snapshot(&base)?;

        let label = if let Some(label) = labels.get(&page.target_id) {
            label.clone()
        } else {
            let used = labels.values().cloned().collect::<HashSet<_>>();
            let label =
                if main_target_hint == Some(page.target_id.as_str()) && !used.contains("main") {
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
            labels.insert(page.target_id.clone(), label.clone());
            label
        };

        if let Some(number) = tab_number(&label) {
            next_tab_number = next_tab_number.max(number.saturating_add(1));
        }
        write_snapshot_atomic(state_path, &snapshot_value(&labels, next_tab_number))?;

        if let Some(existing) = self
            .targets
            .iter_mut()
            .find(|target| target.target_id == page.target_id)
        {
            existing.label = label;
            existing.title = page.title;
            existing.url = page.url;
        } else {
            self.targets.push(LogicalTarget {
                label,
                target_id: page.target_id,
                session_id: None,
                title: page.title,
                url: page.url,
            });
        }
        self.next_tab_number = self.next_tab_number.max(next_tab_number);
        self.targets.sort_by(logical_target_order);
        self.validate()
    }

    fn remove_target_persisted(
        &mut self,
        target_id: &str,
        state_path: &str,
        lock_path: &str,
    ) -> Result<(), Error> {
        let _lock = RegistryFileLock::acquire(lock_path)?;
        let base = read_snapshot(state_path)?.unwrap_or_else(|| self.persistence_value());
        let (mut labels, next_tab_number) = parse_snapshot(&base)?;
        labels.remove(target_id);
        write_snapshot_atomic(state_path, &snapshot_value(&labels, next_tab_number))?;

        self.targets.retain(|target| target.target_id != target_id);
        self.next_tab_number = self.next_tab_number.max(next_tab_number);
        self.validate()
    }

    pub fn resolve_compat(&self, query: &str) -> Option<&LogicalTarget> {
        self.get(query)
            .or_else(|| self.get_by_target_id(query))
            .or_else(|| {
                self.targets
                    .iter()
                    .find(|target| target.title.contains(query) || target.url.contains(query))
            })
    }

    pub fn persistence_value(&self) -> Value {
        let labels = self
            .targets
            .iter()
            .map(|target| (target.target_id.clone(), target.label.clone()))
            .collect::<HashMap<_, _>>();
        snapshot_value(&labels, self.next_tab_number)
    }
}

#[derive(Debug)]
struct PageTarget {
    target_id: String,
    title: String,
    url: String,
}

fn parse_page_target(value: &Value) -> Result<PageTarget, Error> {
    let target_id = value["targetId"]
        .as_str()
        .ok_or_else(|| internal("Target.getTargets page entry is missing string targetId"))?;
    if target_id.trim().is_empty() {
        return Err(internal(
            "Target.getTargets page entry contains an empty targetId",
        ));
    }

    Ok(PageTarget {
        target_id: target_id.to_owned(),
        title: value["title"].as_str().unwrap_or("").to_owned(),
        url: value["url"].as_str().unwrap_or("").to_owned(),
    })
}

fn snapshot_value(labels: &HashMap<String, String>, next_tab_number: u64) -> Value {
    let labels = labels
        .iter()
        .map(|(target_id, label)| (target_id.clone(), Value::String(label.clone())))
        .collect::<Map<_, _>>();
    json!({
        "version": SNAPSHOT_VERSION,
        "next_tab_number": next_tab_number,
        "labels": labels
    })
}

fn parse_snapshot(snapshot: &Value) -> Result<(HashMap<String, String>, u64), Error> {
    let object = snapshot
        .as_object()
        .ok_or_else(|| internal("logical target state must be a JSON object"))?;
    if object.get("version").and_then(Value::as_u64) != Some(SNAPSHOT_VERSION) {
        return Err(internal(format!(
            "unsupported logical target state version; expected {SNAPSHOT_VERSION}"
        )));
    }

    let next_tab_number = object
        .get("next_tab_number")
        .and_then(Value::as_u64)
        .filter(|value| *value >= 2)
        .ok_or_else(|| internal("logical target state next_tab_number must be an integer >= 2"))?;

    let labels = object
        .get("labels")
        .and_then(Value::as_object)
        .ok_or_else(|| internal("logical target state labels must be an object"))?;

    let mut parsed = HashMap::with_capacity(labels.len());
    for (target_id, label) in labels {
        if target_id.trim().is_empty() {
            return Err(internal("logical target state contains an empty target ID"));
        }
        let label = label.as_str().ok_or_else(|| {
            internal(format!(
                "logical target state label for {target_id} must be a string"
            ))
        })?;
        validate_logical_label(label)?;
        parsed.insert(target_id.clone(), label.to_owned());
    }
    validate_unique_labels(&parsed)?;

    Ok((parsed, next_tab_number))
}

fn validate_unique_labels(labels: &HashMap<String, String>) -> Result<(), Error> {
    let mut seen = HashSet::with_capacity(labels.len());
    for label in labels.values() {
        if !seen.insert(label.as_str()) {
            return Err(internal(format!(
                "logical target state contains duplicate label {label}"
            )));
        }
    }
    Ok(())
}

fn validate_logical_label(label: &str) -> Result<(), Error> {
    if label == "main" || tab_number(label).is_some() {
        return Ok(());
    }
    Err(internal(format!(
        "unsupported persisted logical target label: {label}"
    )))
}

fn tab_number(label: &str) -> Option<u64> {
    label
        .strip_prefix("tab-")?
        .parse::<u64>()
        .ok()
        .filter(|n| *n >= 2)
}

fn logical_target_order(left: &LogicalTarget, right: &LogicalTarget) -> std::cmp::Ordering {
    logical_label_key(&left.label)
        .cmp(&logical_label_key(&right.label))
        .then_with(|| left.target_id.cmp(&right.target_id))
}

fn logical_label_key(label: &str) -> (u8, u64, &str) {
    if label == "main" {
        return (0, 0, label);
    }
    if let Some(number) = tab_number(label) {
        return (1, number, label);
    }
    (2, u64::MAX, label)
}

fn read_snapshot(path: &str) -> Result<Option<Value>, Error> {
    let path = Path::new(path);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| {
        internal(format!(
            "failed to read logical target state {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        internal(format!(
            "failed to parse logical target state {}: {error}",
            path.display()
        ))
    })
}

fn write_snapshot_atomic(path: &str, value: &Value) -> Result<(), Error> {
    let path = Path::new(path);
    let parent = path.parent().ok_or_else(|| {
        internal(format!(
            "logical target state path has no parent: {}",
            path.display()
        ))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        internal(format!(
            "failed to create logical target state directory {}: {error}",
            parent.display()
        ))
    })?;

    let nonce = TEMP_FILE_NONCE.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
    let write_result = (|| -> Result<(), Error> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|error| {
                internal(format!(
                    "failed to create logical target state temp file {}: {error}",
                    temp.display()
                ))
            })?;
        let bytes = serde_json::to_vec(value)?;
        file.write_all(&bytes).map_err(|error| {
            internal(format!(
                "failed to write logical target state {}: {error}",
                temp.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            internal(format!(
                "failed to sync logical target state {}: {error}",
                temp.display()
            ))
        })?;
        fs::rename(&temp, path).map_err(|error| {
            internal(format!(
                "failed to atomically replace logical target state {}: {error}",
                path.display()
            ))
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                internal(format!(
                    "failed to sync logical target state directory {}: {error}",
                    parent.display()
                ))
            })?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    write_result
}

struct RegistryFileLock {
    file: File,
}

impl RegistryFileLock {
    fn acquire(path: &str) -> Result<Self, Error> {
        let path = Path::new(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|error| {
                internal(format!(
                    "failed to open logical target registry lock {}: {error}",
                    path.display()
                ))
            })?;
        file.lock().map_err(|error| {
            internal(format!(
                "failed to lock logical target registry {}: {error}",
                path.display()
            ))
        })?;
        Ok(Self { file })
    }
}

impl Drop for RegistryFileLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn internal(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::Internal, message, false)
}

#[cfg(test)]
mod tests {
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
}
