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
    let now = unix_seconds()?;
    let lease = current_lease(&state)?;
    let status = if state["inflight"] == true {
        "interrupted"
    } else if let Some((_, expiry, _)) = lease {
        if expiry <= now { "expired" } else { "claimed" }
    } else {
        "suspended"
    };
    Ok(json!({
        "status":status,
        "lease":lease.map(|(owner, expiry, _)| json!({
            "owner":owner,"expires_at":expiry
        })),
        "resume_id":id,
        "handoff_to":state["handoff_to"].as_str().unwrap_or("caller"),
        "pending_child":state["pending_child"],
        "awaiting":state["awaiting"],
        "current":state["current"]
    }))
}

/// A claim is an optional, time-limited *coordination* lease. The token is
/// issued only to the claimant; MCP authentication remains an independent
/// requirement and owner labels are not cryptographically attested identities.
const DEFAULT_LEASE_SECONDS: u64 = 900;

fn unix_seconds() -> Result<u64, Box<dyn std::error::Error>> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs())
}

fn validate_lease_duration(seconds: u64) -> Result<(), &'static str> {
    if !(60..=3600).contains(&seconds) {
        return Err("routine lease duration must be 60-3600 seconds");
    }
    Ok(())
}

fn current_lease(state: &Value) -> Result<Option<(&str, u64, &str)>, &'static str> {
    if state["lease"].is_null() {
        return Ok(None);
    }
    let lease = &state["lease"];
    let owner = lease["owner"]
        .as_str()
        .ok_or("invalid routine lease owner")?;
    let expires = lease["expires_at"]
        .as_u64()
        .ok_or("invalid routine lease expiry")?;
    let token = lease["token"]
        .as_str()
        .ok_or("invalid routine lease token")?;
    Ok(Some((owner, expires, token)))
}

fn valid_actor(actor: &str) -> bool {
    !actor.is_empty()
        && actor.len() <= 64
        && actor
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_')
}

fn token_matches(expected: &str, provided: &str) -> bool {
    if expected.len() != 32 || provided.len() != 32 {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in expected.bytes().zip(provided.bytes()) {
        difference |= left ^ right;
    }
    difference == 0
}

pub(crate) fn check_resume_lease(
    state: &Value,
    token: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    match current_lease(state)? {
        None if token.is_none() => Ok(()),
        None => Err("routine has no active claim; omit lease_token".into()),
        Some((_, expires, expected)) => {
            if expires <= unix_seconds()? {
                return Err(
                    "routine lease expired; inspect effects before explicit takeover".into(),
                );
            }
            let provided = token.ok_or("claimed routine requires lease_token to resume")?;
            if !token_matches(expected, provided) {
                return Err("invalid routine lease token".into());
            }
            Ok(())
        }
    }
}

fn ensure_claimable(state: &Value) -> Result<(), Box<dyn std::error::Error>> {
    if state["kind"] != "graph" || state["inflight"] == true {
        return Err("only safely suspended graph routines can be claimed".into());
    }
    if !matches!(
        state["awaiting"]["kind"].as_str(),
        Some("agent" | "agent_required")
    ) {
        return Err("only agent handoffs can be claimed".into());
    }
    Ok(())
}

pub(crate) fn claim(
    id: &str,
    owner: &str,
    seconds: Option<u64>,
    takeover_reason: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    claim_in(&root(), id, owner, seconds, takeover_reason)
}

fn claim_in(
    dir: &Path,
    id: &str,
    owner: &str,
    seconds: Option<u64>,
    takeover_reason: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    if !valid_actor(owner) {
        return Err("invalid lease owner; use 1-64 lowercase letters/digits/-/_".into());
    }
    let duration = seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
    validate_lease_duration(duration)?;
    if let Some(reason) = takeover_reason
        && (reason.trim().len() < 12 || reason.len() > 300)
    {
        return Err("takeover_reason must contain 12-300 characters".into());
    }
    let _lock = acquire_in(dir, id)?;
    if receipt_in(dir, id)?.is_some() {
        return Err("routine is already terminated".into());
    }
    let mut state = checkpoint_in(dir, id)?;
    ensure_claimable(&state)?;
    let now = unix_seconds()?;
    match (current_lease(&state)?, takeover_reason) {
        (None, None) => {}
        (None, Some(_)) => return Err("takeover requires a prior expired lease".into()),
        (Some((_, expiry, _)), _) if expiry > now => {
            return Err("routine is already claimed; wait for expiry or release".into());
        }
        (Some(_), None) => {
            return Err(
                "expired claim requires explicit takeover_reason after checking side effects"
                    .into(),
            );
        }
        (Some(_), Some(_)) => {}
    }
    let token = format!("{:032x}", rand::random::<u128>());
    let expires = now.checked_add(duration).ok_or("lease expiry overflow")?;
    state["lease"] = json!({
        "owner":owner, "token":token, "claimed_at":now,
        "expires_at":expires,"takeover_reason":takeover_reason
    });
    save_checkpoint_in(dir, id, &state)?;
    Ok(json!({
        "status":"claimed","resume_id":id,"owner":owner,
        "expires_at":expires,"lease_token":token
    }))
}

pub(crate) fn renew(
    id: &str,
    token: &str,
    seconds: Option<u64>,
) -> Result<Value, Box<dyn std::error::Error>> {
    renew_in(&root(), id, token, seconds)
}

fn renew_in(
    dir: &Path,
    id: &str,
    token: &str,
    seconds: Option<u64>,
) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    let duration = seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
    validate_lease_duration(duration)?;
    let _lock = acquire_in(dir, id)?;
    if receipt_in(dir, id)?.is_some() {
        return Err("routine is already terminated".into());
    }
    let mut state = checkpoint_in(dir, id)?;
    ensure_claimable(&state)?;
    check_resume_lease(&state, Some(token))?;
    let expiry = unix_seconds()?
        .checked_add(duration)
        .ok_or("lease expiry overflow")?;
    state["lease"]["expires_at"] = json!(expiry);
    save_checkpoint_in(dir, id, &state)?;
    Ok(json!({
        "status":"claimed","resume_id":id,
        "owner":state["lease"]["owner"],"expires_at":expiry
    }))
}

pub(crate) fn release(id: &str, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
    release_in(&root(), id, token)
}

fn release_in(dir: &Path, id: &str, token: &str) -> Result<Value, Box<dyn std::error::Error>> {
    validate_resume_id(id)?;
    let _lock = acquire_in(dir, id)?;
    if receipt_in(dir, id)?.is_some() {
        return Err("routine is already terminated".into());
    }
    let mut state = checkpoint_in(dir, id)?;
    ensure_claimable(&state)?;
    let (_, _, expected) = current_lease(&state)?.ok_or("routine is not claimed")?;
    if !token_matches(expected, token) {
        return Err("invalid routine lease token".into());
    }
    state["lease"] = Value::Null;
    save_checkpoint_in(dir, id, &state)?;
    Ok(json!({"status":"suspended","resume_id":id,"claim_released":true}))
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
    fn agent_claim_requires_token_and_exposes_only_public_lease_metadata() {
        let isolated = TestRoot::new();
        let dir = &isolated.0;
        let id = format!("997{:018}", rand::random::<u64>());
        let state = json!({
            "kind":"graph","inflight":false,
            "awaiting":{"kind":"agent","instruction":"Verify invoice"}
        });
        save_checkpoint_in(dir, &id, &state).unwrap();
        check_resume_lease(&state, None).unwrap();
        assert!(claim_in(dir, &id, "worker", Some(59), None).is_err());
        assert!(claim_in(dir, &id, "bad worker", None, None).is_err());
        let claimed = claim_in(dir, &id, "worker", None, None).unwrap();
        let token = claimed["lease_token"].as_str().unwrap();
        assert_eq!(token.len(), 32);
        assert_eq!(claimed["status"], "claimed");
        let checkpoint = checkpoint_in(dir, &id).unwrap();
        assert!(check_resume_lease(&checkpoint, None).is_err());
        assert!(check_resume_lease(&checkpoint, Some("not-a-token")).is_err());
        check_resume_lease(&checkpoint, Some(token)).unwrap();
        assert!(claim_in(dir, &id, "other-worker", None, None).is_err());
        let status = inspect_in(dir, &id).unwrap();
        assert_eq!(status["status"], "claimed");
        assert_eq!(status["lease"]["owner"], "worker");
        assert_eq!(status["lease"]["expires_at"], claimed["expires_at"]);
        assert!(!status.to_string().contains(token));
        assert!(renew_in(dir, &id, "bad", None).is_err());
        assert_eq!(
            renew_in(dir, &id, token, Some(900)).unwrap()["status"],
            "claimed"
        );
        assert!(release_in(dir, &id, "bad").is_err());
        assert_eq!(release_in(dir, &id, token).unwrap()["claim_released"], true);
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "suspended");
        check_resume_lease(&checkpoint_in(dir, &id).unwrap(), None).unwrap();
    }

    #[test]
    fn expired_claim_requires_explicit_takeover_and_never_replays_inflight() {
        let isolated = TestRoot::new();
        let dir = &isolated.0;
        let id = format!("996{:018}", rand::random::<u64>());
        let initial = json!({
            "kind":"graph","inflight":false,
            "awaiting":{"kind":"agent_required","instruction":"Recover selector"}
        });
        save_checkpoint_in(dir, &id, &initial).unwrap();
        let first = claim_in(dir, &id, "worker-a", None, None).unwrap();
        let token = first["lease_token"].as_str().unwrap();
        let mut state = checkpoint_in(dir, &id).unwrap();
        state["lease"]["expires_at"] = json!(0);
        save_checkpoint_in(dir, &id, &state).unwrap();
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "expired");
        assert!(check_resume_lease(&state, Some(token)).is_err());
        assert!(renew_in(dir, &id, token, None).is_err());
        assert!(claim_in(dir, &id, "worker-b", None, None).is_err());
        assert!(claim_in(dir, &id, "worker-b", None, Some("short")).is_err());
        let second = claim_in(
            dir,
            &id,
            "worker-b",
            None,
            Some("I checked that the previous action had no side effect"),
        )
        .unwrap();
        assert_ne!(second["lease_token"], first["lease_token"]);
        let state = checkpoint_in(dir, &id).unwrap();
        assert!(check_resume_lease(&state, Some(token)).is_err());
        check_resume_lease(&state, second["lease_token"].as_str()).unwrap();
        let mut unsafe_state = state;
        unsafe_state["inflight"] = json!(true);
        save_checkpoint_in(dir, &id, &unsafe_state).unwrap();
        assert!(claim_in(dir, &id, "worker-c", None, None).is_err());
        assert!(release_in(dir, &id, second["lease_token"].as_str().unwrap()).is_err());
        assert_eq!(inspect_in(dir, &id).unwrap()["status"], "interrupted");
    }

    #[test]
    fn non_agent_handoffs_and_completed_routines_cannot_be_claimed() {
        let isolated = TestRoot::new();
        let dir = &isolated.0;
        let id = format!("995{:018}", rand::random::<u64>());
        save_checkpoint_in(
            dir,
            &id,
            &json!({
                "kind":"graph","inflight":false,"awaiting":{"kind":"human"}
            }),
        )
        .unwrap();
        assert!(claim_in(dir, &id, "worker", None, None).is_err());
        record_in(dir, &id, &json!({"status":"completed"})).unwrap();
        assert!(claim_in(dir, &id, "worker", None, None).is_err());
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
