//! Persistent, opt-in trace capture for Jelly's existing browser action log.
//! A single active capture covers the runtime's concurrent callers; capturing
//! does not itself launch Chromium, record video, or execute a routine.
use serde_json::{Value, json};
use std::cell::RefCell;

thread_local! {
    static SELECTED_CAPTURE: RefCell<Option<String>> = const { RefCell::new(None) };
}
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SCAN_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CAPTURE_EVENTS: usize = 10_000;

fn root() -> PathBuf {
    crate::ROUTINE_STATE_DIR.path().join("captures")
}

fn log() -> PathBuf {
    crate::LOG_DIR.path().join("actions.jsonl")
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn valid_id(id: &str) -> bool {
    id.len() == 40
        && id.starts_with("capture-")
        && id[8..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn state_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn initialize(dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(dir)?;
    // Restrict capture metadata even when the surrounding runtime was
    // provisioned with more permissive permissions.
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

/// Keep this lock file forever: unlinking a locked inode allows two writers
/// to acquire locks on distinct inodes under the same pathname.
fn lock(dir: &Path) -> Result<File, Box<dyn Error>> {
    initialize(dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join(".lock"))?;
    file.try_lock()
        .map_err(|e| format!("routine capture is busy: {e}"))?;
    Ok(file)
}

fn atomic_json(dir: &Path, name: &str, value: &Value) -> Result<(), Box<dyn Error>> {
    let temp = dir.join(format!(".{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let written = (|| {
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&temp, dir.join(name))?;
        Ok::<_, Box<dyn Error>>(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written
}

fn read_state(dir: &Path, id: &str) -> Result<Value, Box<dyn Error>> {
    if !valid_id(id) {
        return Err("invalid routine capture ID".into());
    }
    let state: Value = serde_json::from_slice(&fs::read(state_file(dir, id))?)?;
    if state["capture_id"] != id
        || !matches!(state["status"].as_str(), Some("capturing" | "stopped"))
    {
        return Err("corrupt routine capture state".into());
    }
    Ok(state)
}

fn active(dir: &Path) -> Result<Option<Value>, Box<dyn Error>> {
    let path = dir.join("active.json");
    let marker: Value = match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let id = marker["capture_id"]
        .as_str()
        .ok_or("invalid active capture marker")?;
    let state = read_state(dir, id)?;
    if state["status"] == "capturing" {
        Ok(Some(state))
    } else {
        Ok(None)
    }
}

/// Explicit isolation selector. A declared capture ID is *not* an authenticated
/// agent identity; callers with access to an ID can attach events to it.
pub fn validate_capture_id(id: &str) -> Result<(), Box<dyn Error>> {
    validate_capture_in(&root(), id)
}

fn validate_capture_in(dir: &Path, id: &str) -> Result<(), Box<dyn Error>> {
    let state = read_state(dir, id)?;
    if state["status"] != "capturing" || state["scope"] != "isolated" {
        return Err("capture must be an active isolated session".into());
    }
    Ok(())
}

pub(crate) fn with_capture_id<T>(id: Option<&str>, f: impl FnOnce() -> T) -> T {
    SELECTED_CAPTURE.with(|selection| {
        let previous = selection.replace(id.map(str::to_owned));
        struct Restore<'a>(&'a RefCell<Option<String>>, Option<String>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                self.0.replace(self.1.take());
            }
        }
        let _restore = Restore(selection, previous);
        f()
    })
}

/// Explicit capture selection takes precedence over legacy shared capture.
/// A caller can supply JELLY_CAPTURE_ID or a per-tool MCP capture_id.
pub fn active_trace_id() -> Option<String> {
    SELECTED_CAPTURE
        .with(|selected| selected.borrow().clone())
        .or_else(|| std::env::var("JELLY_CAPTURE_ID").ok())
        .or_else(|| active_id_in(&root()))
}

fn active_id_in(dir: &Path) -> Option<String> {
    active(dir)
        .ok()
        .flatten()
        .and_then(|state| state["capture_id"].as_str().map(str::to_owned))
}

fn summary(log: &Path, trace: &str) -> Result<Value, Box<dyn Error>> {
    let mut primitives = 0u64;
    let mut checks = 0u64;
    let mut failures = 0u64;
    let mut wrappers = 0u64;
    let mut unsupported = 0u64;
    let f = match File::open(log) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(json!({"primitives":0,"verifications":0,"failures":0,
                "wrapper_events":0,"unsupported_primitives":0}));
        }
        Err(e) => return Err(e.into()),
    };
    if f.metadata()?.len() > MAX_SCAN_BYTES {
        return Err("Jelly action log exceeds 32 MiB; rotate it before capture status".into());
    }
    let mut events = 0;
    for line in BufReader::new(f).lines() {
        let value: Value = serde_json::from_str(&line?)?;
        if value["trace_id"] != trace {
            continue;
        }
        events += 1;
        if events > MAX_CAPTURE_EVENTS {
            return Err("capture exceeds 10,000 events".into());
        }
        let kind = value["event"].as_str().unwrap_or("");
        if kind == "primitive.completed" || kind == "primitive.failed" {
            primitives += 1;
            if value["ok"] != true || kind.ends_with("failed") {
                failures += 1;
            }
            let tool = value["tool"].as_str().unwrap_or("");
            if matches!(
                tool,
                "agent-assert-visible"
                    | "agent-assert-text"
                    | "agent-assert-title"
                    | "agent-assert-url"
                    | "agent-assert-image-ready"
                    | "agent-wait-for"
            ) {
                checks += 1;
            } else if !matches!(
                tool,
                "agent-click" | "agent-check" | "agent-select" | "agent-scroll"
            ) {
                unsupported += 1;
            }
        } else {
            wrappers += 1;
            if value["ok"] != true {
                failures += 1;
            }
        }
    }
    Ok(
        json!({"primitives":primitives,"verifications":checks,"failures":failures,
        "wrapper_events":wrappers,"unsupported_primitives":unsupported}),
    )
}

fn with_summary(log: &Path, mut state: Value) -> Result<Value, Box<dyn Error>> {
    let id = state["capture_id"]
        .as_str()
        .ok_or("capture ID missing")?
        .to_owned();
    state["evidence"] = summary(log, &id)?;
    Ok(state)
}

/// Real management entrypoint used by the existing call-routine CLI/MCP tool.
pub(crate) fn manage(operation: &str, id: Option<&str>) -> Result<Value, Box<dyn Error>> {
    manage_in(&root(), &log(), operation, id)
}

fn manage_in(
    dir: &Path,
    log: &Path,
    operation: &str,
    id: Option<&str>,
) -> Result<Value, Box<dyn Error>> {
    match operation {
        "start" | "start-isolated" => {
            if id.is_some() {
                return Err("capture start does not accept an ID".into());
            }
            let isolated = operation == "start-isolated";
            let _guard = lock(dir)?;
            if !isolated && active(dir)?.is_some() {
                return Err(
                    "another shared routine capture is already active; stop it first".into(),
                );
            }
            // The old marker may remain after a crash between stopping a
            // capture and removing it. It is safe to replace if no longer live.
            let name = format!("capture-{:032x}", rand::random::<u128>());
            let state = json!({"capture_id":name,"status":"capturing",
                "started_at_ms":timestamp_ms(),"stopped_at_ms":null,
                "source":"Jelly actions.jsonl",
                "scope":if isolated {"isolated"} else {"shared"}});
            let new_file = state_file(dir, &name);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&new_file)?;
            file.write_all(&serde_json::to_vec_pretty(&state)?)?;
            file.sync_all()?;
            if !isolated
                && let Err(error) = atomic_json(dir, "active.json", &json!({"capture_id":name}))
            {
                let _ = fs::remove_file(&new_file);
                return Err(error);
            }
            Ok(state)
        }
        "status" => {
            let state = if let Some(id) = id {
                read_state(dir, id)?
            } else {
                match active(dir)? {
                    Some(state) => state,
                    None => return Ok(json!({"status":"idle"})),
                }
            };
            if state["status"] == "stopped" && state.get("evidence").is_some() {
                Ok(state)
            } else {
                with_summary(log, state)
            }
        }
        "stop" => {
            let id = id.ok_or("capture stop requires capture_id")?;
            let _guard = lock(dir)?;
            let current = read_state(dir, id)?;
            if current["status"] != "capturing" {
                return Err("capture is already stopped".into());
            }
            if current["scope"] != "isolated" {
                let shared = active(dir)?.ok_or("no shared routine capture is active")?;
                if shared["capture_id"] != id {
                    return Err("capture ID does not match active shared session".into());
                }
            }
            let mut finished = current;
            finished["status"] = json!("stopped");
            finished["stopped_at_ms"] = json!(timestamp_ms());
            // Stopping is always possible even after log rotation or damage.
            finished = match with_summary(log, finished.clone()) {
                Ok(with_evidence) => with_evidence,
                Err(error) => {
                    finished["evidence"] = json!({"available":false,"reason":error.to_string()});
                    finished
                }
            };
            atomic_json(dir, &format!("{id}.json"), &finished)?;
            if finished["scope"] != "isolated" {
                let _ = fs::remove_file(dir.join("active.json"));
            }
            Ok(finished)
        }
        _ => Err("capture operation must be start, status or stop".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jelly-capture-test-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn start_status_stop_is_persistent_and_automatically_resolves_trace() {
        let root = Root::new();
        let sessions = root.0.join("captures");
        let log = root.0.join("actions.jsonl");
        assert_eq!(
            manage_in(&sessions, &log, "status", None).unwrap()["status"],
            "idle"
        );
        let started = manage_in(&sessions, &log, "start", None).unwrap();
        let id = started["capture_id"].as_str().unwrap();
        assert!(valid_id(id));
        assert_eq!(active_id_in(&sessions).as_deref(), Some(id));
        assert!(manage_in(&sessions, &log, "start", None).is_err());
        fs::write(&log, format!(
            "{}\n{}\n{}\n",
            json!({"trace_id":id,"event":"primitive.completed","tool":"agent-click","ok":true}),
            json!({"trace_id":id,"event":"primitive.completed","tool":"agent-assert-visible","ok":true}),
            json!({"trace_id":"elsewhere","event":"primitive.failed","tool":"agent-fill","ok":false})
        )).unwrap();
        let status = manage_in(&sessions, &log, "status", None).unwrap();
        assert_eq!(status["evidence"]["primitives"], 2);
        assert_eq!(status["evidence"]["verifications"], 1);
        assert_eq!(status["evidence"]["failures"], 0);
        assert!(manage_in(&sessions, &log, "stop", Some("capture-bad")).is_err());
        let completed = manage_in(&sessions, &log, "stop", Some(id)).unwrap();
        assert_eq!(completed["status"], "stopped");
        assert_eq!(completed["evidence"]["primitives"], 2);
        assert!(active_id_in(&sessions).is_none());
        assert_eq!(
            manage_in(&sessions, &log, "status", None).unwrap()["status"],
            "idle"
        );
        // History retains the frozen evidence counts after log rotation.
        fs::remove_file(&log).unwrap();
        let historic = manage_in(&sessions, &log, "status", Some(id)).unwrap();
        assert_eq!(historic["status"], "stopped");
        assert_eq!(historic["evidence"]["primitives"], 2);
        assert!(manage_in(&sessions, &log, "stop", Some(id)).is_err());
    }

    #[test]
    fn independent_isolated_captures_do_not_mix_or_depend_on_shared_marker() {
        let root = Root::new();
        let sessions = root.0.join("captures");
        let log = root.0.join("actions.jsonl");
        let first = manage_in(&sessions, &log, "start-isolated", None).unwrap();
        let second = manage_in(&sessions, &log, "start-isolated", None).unwrap();
        let first_id = first["capture_id"].as_str().unwrap();
        let second_id = second["capture_id"].as_str().unwrap();
        assert_ne!(first_id, second_id);
        assert!(active_id_in(&sessions).is_none());
        validate_capture_in(&sessions, first_id).unwrap();
        validate_capture_in(&sessions, second_id).unwrap();
        fs::write(
            &log,
            format!(
                "{}\n{}\n",
                json!({"trace_id":first_id,"event":"primitive.completed",
                "tool":"agent-click","ok":true}),
                json!({"trace_id":second_id,"event":"primitive.completed",
                "tool":"agent-assert-visible","ok":true})
            ),
        )
        .unwrap();
        assert_eq!(
            manage_in(&sessions, &log, "status", Some(first_id)).unwrap()["evidence"]["primitives"],
            1
        );
        assert_eq!(
            manage_in(&sessions, &log, "status", Some(second_id)).unwrap()["evidence"]["verifications"],
            1
        );
        manage_in(&sessions, &log, "stop", Some(first_id)).unwrap();
        assert!(validate_capture_in(&sessions, first_id).is_err());
        validate_capture_in(&sessions, second_id).unwrap();
        let shared = manage_in(&sessions, &log, "start", None).unwrap();
        assert_eq!(shared["scope"], "shared");
        assert_eq!(
            active_id_in(&sessions).as_deref(),
            shared["capture_id"].as_str()
        );
        manage_in(&sessions, &log, "stop", Some(second_id)).unwrap();
        assert!(active_id_in(&sessions).is_some());
        manage_in(&sessions, &log, "stop", shared["capture_id"].as_str()).unwrap();
        assert!(active_id_in(&sessions).is_none());
    }

    #[test]
    fn nested_capture_selector_restores_previous_selection() {
        let original = SELECTED_CAPTURE.with(|slot| slot.borrow().clone());
        with_capture_id(Some("capture-11111111111111111111111111111111"), || {
            assert_eq!(
                SELECTED_CAPTURE
                    .with(|slot| slot.borrow().clone())
                    .as_deref(),
                Some("capture-11111111111111111111111111111111")
            );
            with_capture_id(Some("capture-22222222222222222222222222222222"), || {
                assert_eq!(
                    SELECTED_CAPTURE
                        .with(|slot| slot.borrow().clone())
                        .as_deref(),
                    Some("capture-22222222222222222222222222222222")
                );
            });
            assert_eq!(
                SELECTED_CAPTURE
                    .with(|slot| slot.borrow().clone())
                    .as_deref(),
                Some("capture-11111111111111111111111111111111")
            );
        });
        assert_eq!(
            SELECTED_CAPTURE.with(|slot| slot.borrow().clone()),
            original
        );
    }

    #[test]
    fn capture_isolated_permissions_and_invalid_ids() {
        let root = Root::new();
        let sessions = root.0.join("captures");
        let log = root.0.join("actions.jsonl");
        let capture = manage_in(&sessions, &log, "start", None).unwrap();
        let id = capture["capture_id"].as_str().unwrap();
        assert_eq!(
            fs::metadata(&sessions).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(state_file(&sessions, id))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(sessions.join("active.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(manage_in(&sessions, &log, "status", Some("../active")).is_err());
        assert!(
            manage_in(
                &sessions,
                &log,
                "stop",
                Some("capture-ffffffffffffffffffffffffffffffff")
            )
            .is_err()
        );
        // A bad action-log line must not trap the operator in an active
        // capture. Return a stopped session with explicit incomplete evidence.
        fs::write(&log, b"corrupted JSONL\n").unwrap();
        let stopped = manage_in(&sessions, &log, "stop", Some(id)).unwrap();
        assert_eq!(stopped["status"], "stopped");
        assert_eq!(stopped["evidence"]["available"], false);
        assert!(active_id_in(&sessions).is_none());
    }
}
