//! Download state data and deterministic transitions.
//! The shell supplies timestamps, sanitized URLs, and observed filesystem paths.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) const DOWNLOAD_STATE_VERSION: u32 = 1;
pub(crate) const MAX_DOWNLOAD_RECORDS: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub suggested_filename: String,
    pub state: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub materialized_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collision_policy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub cancel_requested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownloadState {
    pub(crate) version: u32,
    pub(crate) downloads: Vec<DownloadRecord>,
}

impl Default for DownloadState {
    fn default() -> Self {
        Self {
            version: DOWNLOAD_STATE_VERSION,
            downloads: Vec::new(),
        }
    }
}

/// Construct a new record from explicit, already observed inputs.
pub(crate) fn new_record(
    id: &str,
    sanitized_url: String,
    suggested_filename: &str,
    now_ms: u64,
) -> DownloadRecord {
    DownloadRecord {
        id: id.to_owned(),
        url: sanitized_url,
        suggested_filename: suggested_filename.to_owned(),
        state: "in_progress".into(),
        received_bytes: 0,
        total_bytes: 0,
        started_at_ms: now_ms,
        updated_at_ms: now_ms,
        file_path: None,
        materialized_path: None,
        collision_policy: None,
        failure_reason: None,
        cancel_requested: false,
        artifact: None,
    }
}

/// Paths are supplied by the shell; the core never checks the filesystem.
pub(crate) struct Progress<'a> {
    pub(crate) protocol_state: &'a str,
    pub(crate) received_bytes: u64,
    pub(crate) total_bytes: u64,
    pub(crate) event_path: Option<&'a str>,
    pub(crate) fallback_path: Option<&'a str>,
    pub(crate) now_ms: u64,
}

pub(crate) fn apply_progress(record: &DownloadRecord, event: &Progress<'_>) -> DownloadRecord {
    let mut next = record.clone();
    next.received_bytes = event.received_bytes;
    next.total_bytes = event.total_bytes;
    next.updated_at_ms = event.now_ms;
    match event.protocol_state {
        "completed" => {
            next.state = "completed".into();
            next.failure_reason = None;
            next.file_path = event.event_path.or(event.fallback_path).map(str::to_owned);
        }
        "canceled" => {
            next.state = "canceled".into();
            next.failure_reason = Some(
                if record.cancel_requested {
                    "canceled_by_user"
                } else {
                    "canceled_by_browser"
                }
                .into(),
            );
        }
        _ => next.state = "in_progress".into(),
    }
    next
}

pub(crate) fn interrupt_record(
    record: &DownloadRecord,
    reason: &str,
    now_ms: u64,
) -> DownloadRecord {
    if record.state != "in_progress" {
        return record.clone();
    }
    let mut next = record.clone();
    next.state = "interrupted".into();
    next.updated_at_ms = now_ms;
    next.failure_reason = Some(reason.to_owned());
    next
}

pub(crate) fn request_cancel(record: &DownloadRecord, now_ms: u64) -> DownloadRecord {
    let mut next = record.clone();
    next.cancel_requested = true;
    next.updated_at_ms = now_ms;
    if next.state == "canceled" {
        next.failure_reason = Some("canceled_by_user".into());
    }
    next
}

/// Copy policies are decisions; opening/copying a file belongs to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CollisionPolicy {
    Fail,
    Overwrite,
    Uniquify,
}

impl CollisionPolicy {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Fail => "fail",
            Self::Overwrite => "overwrite",
            Self::Uniquify => "uniquify",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MaterializationPlan {
    UseManagedFile,
    Copy {
        directory: PathBuf,
        requested: PathBuf,
        policy: CollisionPolicy,
    },
}

/// The shell must first establish that the managed file exists. As before,
/// an absent destination ignores the collision option altogether.
pub(crate) fn plan_materialization(
    destination: Option<&str>,
    collision: &str,
    filename: &str,
    fallback_id: &str,
) -> Result<MaterializationPlan, ()> {
    let Some(destination) = destination else {
        return Ok(MaterializationPlan::UseManagedFile);
    };
    let policy = match collision {
        "fail" => CollisionPolicy::Fail,
        "overwrite" => CollisionPolicy::Overwrite,
        "uniquify" => CollisionPolicy::Uniquify,
        _ => return Err(()),
    };
    let directory = PathBuf::from(destination);
    Ok(MaterializationPlan::Copy {
        requested: directory.join(safe_filename(filename, fallback_id)),
        directory,
        policy,
    })
}

pub(crate) fn safe_filename(value: &str, fallback: &str) -> String {
    Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != "." && *value != "..")
        .unwrap_or(fallback)
        .to_owned()
}

pub(crate) fn numbered_path(path: &Path, index: u32) -> PathBuf {
    if index == 0 {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    let name = match extension {
        Some(extension) => format!("{stem} ({index}).{extension}"),
        None => format!("{stem} ({index})"),
    };
    parent.join(name)
}

/// State validation runs before choosing a destination or registering an artifact.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FinalizeState {
    Ready,
    NotCompleted { retryable: bool },
}

pub(crate) fn finalize_state(record: &DownloadRecord) -> FinalizeState {
    if record.state == "completed" {
        FinalizeState::Ready
    } else {
        FinalizeState::NotCompleted {
            retryable: record.state == "in_progress",
        }
    }
}

/// The shell resolves the source from disk, registers the artifact and provides
/// the timestamp before computing this new persisted record.
pub(crate) fn finalized_record(
    record: &DownloadRecord,
    source: String,
    materialized_path: String,
    collision_policy: Option<String>,
    artifact: Value,
    now_ms: u64,
) -> DownloadRecord {
    let mut next = record.clone();
    next.file_path = Some(source);
    next.materialized_path = Some(materialized_path);
    next.collision_policy = collision_policy;
    next.artifact = Some(artifact);
    next.updated_at_ms = now_ms;
    next
}

/// A decision about download progress independent of wall clock and polling.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WaitState {
    Finalize,
    Failed,
    Pending,
}

pub(crate) fn wait_state(record: &DownloadRecord) -> WaitState {
    match record.state.as_str() {
        "completed" => WaitState::Finalize,
        "canceled" | "interrupted" => WaitState::Failed,
        _ => WaitState::Pending,
    }
}

pub(crate) fn has_in_progress(records: &[DownloadRecord]) -> bool {
    records.iter().any(|record| record.state == "in_progress")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DownloadRecord {
        new_record(
            "guid-a",
            "https://example.test/safe".into(),
            "example.pdf",
            100,
        )
    }

    #[test]
    fn materialization_preserves_no_destination_and_invalid_policy_semantics() {
        assert_eq!(
            plan_materialization(None, "unrecognized", "report.pdf", "guid-a"),
            Ok(MaterializationPlan::UseManagedFile),
        );
        assert!(
            plan_materialization(Some("/tmp/out"), "unrecognized", "report.pdf", "guid-a").is_err()
        );
        for (name, policy) in [
            ("fail", CollisionPolicy::Fail),
            ("overwrite", CollisionPolicy::Overwrite),
            ("uniquify", CollisionPolicy::Uniquify),
        ] {
            assert_eq!(policy.name(), name);
            assert_eq!(
                plan_materialization(Some("/tmp/out"), name, "../report.pdf", "guid-a"),
                Ok(MaterializationPlan::Copy {
                    directory: PathBuf::from("/tmp/out"),
                    requested: PathBuf::from("/tmp/out/report.pdf"),
                    policy,
                })
            );
        }
    }

    #[test]
    fn filenames_and_numbering_are_pure_and_preserve_extensions() {
        assert_eq!(safe_filename("../report.csv", "fallback"), "report.csv");
        assert_eq!(safe_filename("", "fallback"), "fallback");
        assert_eq!(safe_filename("..", "fallback"), "fallback");
        let base = Path::new("/tmp/report.csv");
        assert_eq!(numbered_path(base, 0), base);
        assert_eq!(numbered_path(base, 1), PathBuf::from("/tmp/report (1).csv"));
        assert_eq!(
            numbered_path(Path::new("/tmp/plain"), 3),
            PathBuf::from("/tmp/plain (3)")
        );
    }

    #[test]
    fn finalize_state_preserves_retryable_only_for_in_progress() {
        let mut record = sample();
        assert_eq!(
            finalize_state(&record),
            FinalizeState::NotCompleted { retryable: true }
        );
        for state in ["interrupted", "canceled", "unknown"] {
            record.state = state.into();
            assert_eq!(
                finalize_state(&record),
                FinalizeState::NotCompleted { retryable: false }
            );
        }
        record.state = "completed".into();
        assert_eq!(finalize_state(&record), FinalizeState::Ready);
    }

    #[test]
    fn finalized_record_only_updates_artifact_and_materialization_fields() {
        let mut record = sample();
        record.state = "completed".to_owned();
        record.received_bytes = 2048;
        record.total_bytes = 2048;
        let artifact = serde_json::json!({"id":"artifact-9"});
        let finalized = finalized_record(
            &record,
            "/tmp/managed/guid-a".into(),
            "/tmp/out/file.pdf".into(),
            Some("uniquify".into()),
            artifact.clone(),
            456,
        );
        assert_eq!(finalized.file_path.as_deref(), Some("/tmp/managed/guid-a"));
        assert_eq!(
            finalized.materialized_path.as_deref(),
            Some("/tmp/out/file.pdf")
        );
        assert_eq!(finalized.collision_policy.as_deref(), Some("uniquify"));
        assert_eq!(finalized.artifact, Some(artifact));
        assert_eq!(finalized.updated_at_ms, 456);
        assert_eq!(finalized.started_at_ms, 100);
        assert_eq!(finalized.state, "completed");
        assert_eq!(
            (finalized.received_bytes, finalized.total_bytes),
            (2048, 2048)
        );
        assert_eq!(record.file_path, None);
        assert_eq!(record.materialized_path, None);
        assert_eq!(record.artifact, None);
    }

    #[test]
    fn wait_decision_distinguishes_finished_failed_and_pending_without_a_clock() {
        let mut record = sample();
        assert_eq!(wait_state(&record), WaitState::Pending);
        record.state = "completed".into();
        assert_eq!(wait_state(&record), WaitState::Finalize);
        record.state = "canceled".into();
        assert_eq!(wait_state(&record), WaitState::Failed);
        record.state = "interrupted".into();
        assert_eq!(wait_state(&record), WaitState::Failed);
        record.state = "unknown".into();
        assert_eq!(wait_state(&record), WaitState::Pending);
    }

    #[test]
    fn restart_detection_only_considers_downloads_actually_in_progress() {
        let mut record = sample();
        assert!(!has_in_progress(&[]));
        assert!(has_in_progress(&[record.clone()]));
        for status in ["completed", "canceled", "interrupted"] {
            record.state = status.into();
            assert!(!has_in_progress(&[record.clone()]));
            assert_eq!(
                interrupt_record(&record, "browser_restarted_before_completion", 999),
                record
            );
        }
        record.state = "in_progress".into();
        let recovered = interrupt_record(&record, "browser_restarted_before_completion", 999);
        assert_eq!(recovered.state, "interrupted");
        assert_eq!(
            recovered.failure_reason.as_deref(),
            Some("browser_restarted_before_completion")
        );
        assert_eq!(recovered.updated_at_ms, 999);
        assert_eq!(record.state, "in_progress");
    }

    #[test]
    fn new_download_preserves_the_persisted_record_contract() {
        let record = sample();
        assert_eq!(record.id, "guid-a");
        assert_eq!(record.state, "in_progress");
        assert_eq!(record.started_at_ms, 100);
        assert_eq!(record.updated_at_ms, 100);
        assert_eq!(record.received_bytes, 0);
        assert_eq!(record.failure_reason, None);
        assert!(!record.cancel_requested);
        assert_eq!(
            serde_json::to_value(&record).unwrap()["suggested_filename"],
            "example.pdf"
        );
        assert!(
            serde_json::to_value(&record)
                .unwrap()
                .get("file_path")
                .is_none()
        );
    }

    #[test]
    fn progress_complete_chooses_event_path_then_fallback_and_clears_failure() {
        let mut record = sample();
        record.failure_reason = Some("previous_failure".into());
        let event = Progress {
            protocol_state: "completed",
            received_bytes: 88,
            total_bytes: 100,
            event_path: Some("/download/reported"),
            fallback_path: Some("/download/guid-a"),
            now_ms: 200,
        };
        let completed = apply_progress(&record, &event);
        assert_eq!(completed.state, "completed");
        assert_eq!(completed.file_path.as_deref(), Some("/download/reported"));
        assert_eq!(completed.failure_reason, None);
        assert_eq!(
            (
                completed.received_bytes,
                completed.total_bytes,
                completed.updated_at_ms
            ),
            (88, 100, 200)
        );
        assert_eq!(record.state, "in_progress");
        assert_eq!(
            apply_progress(
                &record,
                &Progress {
                    event_path: None,
                    ..event
                }
            )
            .file_path
            .as_deref(),
            Some("/download/guid-a")
        );
    }

    #[test]
    fn cancellation_origin_tracks_the_requested_flag() {
        let event = Progress {
            protocol_state: "canceled",
            received_bytes: 2,
            total_bytes: 10,
            event_path: None,
            fallback_path: None,
            now_ms: 250,
        };
        let record = sample();
        assert_eq!(
            apply_progress(&record, &event).failure_reason.as_deref(),
            Some("canceled_by_browser")
        );
        let requested = request_cancel(&record, 210);
        assert_eq!(
            apply_progress(&requested, &event).failure_reason.as_deref(),
            Some("canceled_by_user")
        );
    }

    #[test]
    fn interrupt_affects_only_in_progress_downloads() {
        let original = sample();
        let interrupted = interrupt_record(&original, "browser_stopped", 500);
        assert_eq!(interrupted.state, "interrupted");
        assert_eq!(interrupted.updated_at_ms, 500);
        assert_eq!(
            interrupted.failure_reason.as_deref(),
            Some("browser_stopped")
        );
        let completed = apply_progress(
            &original,
            &Progress {
                protocol_state: "completed",
                received_bytes: 12,
                total_bytes: 12,
                event_path: None,
                fallback_path: None,
                now_ms: 350,
            },
        );
        assert_eq!(interrupt_record(&completed, "shutdown", 500), completed);
        assert_eq!(original.state, "in_progress");
    }

    #[test]
    fn unknown_protocol_state_preserves_existing_failure_and_paths() {
        let mut record = sample();
        record.file_path = Some("/prior".into());
        record.failure_reason = Some("older".into());
        let next = apply_progress(
            &record,
            &Progress {
                protocol_state: "unknown",
                received_bytes: 1,
                total_bytes: 2,
                event_path: None,
                fallback_path: None,
                now_ms: 300,
            },
        );
        assert_eq!(next.state, "in_progress");
        assert_eq!(next.file_path.as_deref(), Some("/prior"));
        assert_eq!(next.failure_reason.as_deref(), Some("older"));
    }

    #[test]
    fn request_cancel_updates_a_racing_canceled_record() {
        let mut record = sample();
        record.state = "canceled".into();
        record.failure_reason = Some("canceled_by_browser".into());
        let next = request_cancel(&record, 400);
        assert!(next.cancel_requested);
        assert_eq!(next.failure_reason.as_deref(), Some("canceled_by_user"));
    }
}
