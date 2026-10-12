//! Filesystem-backed routine continuations. An advisory file lock serializes
//! resume by ID across processes; completion receipts make retries idempotent.
use crate::ROUTINE_STATE_DIR;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    ROUTINE_STATE_DIR.path().to_path_buf()
}

pub(crate) fn validate_resume_id(id: &str) -> Result<(), &'static str> {
    if id.is_empty() || id.len() > 40 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("invalid routine resume ID");
    }
    Ok(())
}

fn path_in(dir: &Path, id: &str, suffix: &str) -> PathBuf {
    dir.join(format!("{id}.{suffix}"))
}

/// Retain the lock file; deleting it while other processes hold an open file
/// descriptor would allow concurrent workers to lock different inodes.
pub(crate) fn acquire(id: &str) -> Result<File, Box<dyn std::error::Error>> {
    acquire_in(&root(), id)
}

fn acquire_in(dir: &Path, id: &str) -> Result<File, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(dir)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path_in(dir, id, "lock"))?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => "routine resume already in progress".into(),
        std::fs::TryLockError::Error(error) => Box::new(error) as Box<dyn std::error::Error>,
    })?;
    Ok(file)
}

/// Atomic replacement with owner-only file permissions. The unique temporary
/// file and same-directory rename prevent observers from reading partial JSON.
pub(crate) fn save_checkpoint(id: &str, value: &Value) -> Result<(), Box<dyn std::error::Error>> {
    save_checkpoint_in(&root(), id, value)
}

fn save_checkpoint_in(
    dir: &Path,
    id: &str,
    value: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!("{id}.{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&temp, path_in(dir, id, "state"))?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub(crate) fn checkpoint(id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    checkpoint_in(&root(), id)
}

fn checkpoint_in(dir: &Path, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    Ok(serde_json::from_slice(&fs::read(path_in(
        dir, id, "state",
    ))?)?)
}

pub(crate) fn receipt(id: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    receipt_in(&root(), id)
}

fn receipt_in(dir: &Path, id: &str) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    match fs::read(path_in(dir, id, "result")) {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// A receipt is written *before* removing the checkpoint; retries after a crash
/// between these two steps will return the receipt, never re-execute actions.
pub(crate) fn record(id: &str, result: &Value) -> Result<(), Box<dyn std::error::Error>> {
    record_in(&root(), id, result)
}

fn record_in(dir: &Path, id: &str, result: &Value) -> Result<(), Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    fs::create_dir_all(dir)?;
    if path_in(dir, id, "result").exists() {
        return Err("routine completion receipt already exists".into());
    }
    let temp = dir.join(format!("{id}.{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let written = (|| {
        file.write_all(&serde_json::to_vec_pretty(result)?)?;
        file.sync_all()?;
        fs::rename(&temp, path_in(dir, id, "result"))?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(temp);
    }
    written
}

pub(crate) fn inspect(id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    inspect_in(&root(), id)
}

fn inspect_in(dir: &Path, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    if !path_in(dir, id, "state").exists() && !path_in(dir, id, "result").exists() {
        return Err("routine continuation not found".into());
    }
    let _lock = match acquire_in(dir, id) {
        Ok(file) => file,
        Err(error) if error.to_string() == "routine resume already in progress" => {
            return Ok(json!({"status":"running","resume_id":id}));
        }
        Err(error) => return Err(error),
    };
    if let Some(receipt) = receipt_in(dir, id)? {
        return Ok(json!({
            "status":receipt["status"],"resume_id":id,
            "terminal":true
        }));
    }
    let state = checkpoint_in(dir, id)?;
    if state["kind"] != "graph" {
        return Err("routine continuation is not a JSON graph".into());
    }
    Ok(json!({
        "status":if state["inflight"] == true { "interrupted" } else { "suspended" },
        "resume_id":id,
        "handoff_to":state["handoff_to"].as_str().unwrap_or("caller"),
        "pending_child":state["pending_child"],
        "awaiting":state["awaiting"],
        "current":state["current"]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests must never write to the configured production runtime root.
    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "jelly-routine-state-test-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            // Only the uniquely generated directory created by this test.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resume_ids_are_numeric_and_bounded() {
        for id in ["", "abc", "../state", "12.4", "1".repeat(41).as_str()] {
            assert!(validate_resume_id(id).is_err());
        }
        assert!(validate_resume_id("1234").is_ok());
    }

    #[test]
    fn checkpoint_and_receipt_have_distinct_inspectable_states() {
        let isolated = TestRoot::new();
        let dir = &isolated.0;
        let id = format!("998{:018}", rand::random::<u64>());
        let mut state = json!({
            "kind":"graph","current":"verify","handoff_to":"caller",
            "awaiting":{"kind":"agent","instruction":"Inspect invoice"},"inflight":false
        });
        save_checkpoint_in(dir, &id, &state).unwrap();
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "suspended");
        assert_eq!(
            inspect_in(dir, &id).unwrap()["awaiting"]["instruction"],
            "Inspect invoice"
        );

        let held = acquire_in(dir, &id).unwrap();
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "running");
        drop(held);

        state["inflight"] = json!(true);
        save_checkpoint_in(dir, &id, &state).unwrap();
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "interrupted");

        let terminal = json!({"status":"completed","context":{"confidential":"private"}});
        record_in(dir, &id, &terminal).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path_in(dir, &id, "result"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let summary = inspect_in(dir, &id).unwrap();
        assert_eq!(summary["status"], "completed");
        assert_eq!(summary["terminal"], true);
        assert!(summary.get("context").is_none());
        assert_eq!(receipt_in(dir, &id).unwrap(), Some(terminal));
    }

    #[test]
    fn resume_locks_prevent_concurrent_workers() {
        let isolated = TestRoot::new();
        let dir = &isolated.0;
        let id = format!("999{:018}", rand::random::<u64>());
        let first = acquire_in(dir, &id).unwrap();
        let second = acquire_in(dir, &id).unwrap_err();
        assert!(second.to_string().contains("already in progress"));
        drop(first);
        let third = acquire_in(dir, &id).unwrap();
        drop(third);
        // The test owns this unique ID and has released every descriptor.
    }
}
