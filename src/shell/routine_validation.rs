//! Bounded, offline fixture replay for staged crystallized auto-routines.
//! A PASS here proves only that declared fixtures match the graph plan;
//! it is not CDP/browser verification and never promotes or executes effects.
use crate::core::routines::{PlannedNode, context_from_vars, plan_node, validate_inputs};
use crate::shell::routine_versions;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_STEPS: usize = 42;
const MAX_FIXTURE_BYTES: usize = 16 * 1024;

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_action(name: &str) -> bool {
    matches!(name, "click" | "check" | "select" | "scroll")
}

fn is_check(name: &str) -> bool {
    matches!(
        name,
        "assert-visible" | "assert-text" | "assert-title" | "assert-image-ready" | "wait-for"
    )
}

pub(crate) fn input_context(fixture: &Value) -> Result<Map<String, Value>, Box<dyn Error>> {
    let vars = fixture["vars"]
        .as_object()
        .ok_or("validation fixture requires vars object")?;
    if vars.len() > 32 {
        return Err("validation fixture has too many inputs".into());
    }
    let values = vars
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|text| (key.clone(), text.to_owned()))
                .ok_or_else(|| format!("validation input {key} must be a string"))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(context_from_vars(values))
}

fn validate_fixture(fixture: &Value) -> Result<(), Box<dyn Error>> {
    let bytes = serde_json::to_vec(fixture)?;
    if bytes.len() > MAX_FIXTURE_BYTES {
        return Err("validation fixture exceeds 16 KiB".into());
    }
    let fields = fixture
        .as_object()
        .ok_or("validation fixture must be an object")?;
    if fields.len() != 2 || !fields.contains_key("vars") || !fields.contains_key("steps") {
        return Err("validation fixture accepts exactly vars and steps".into());
    }
    let steps = fixture["steps"]
        .as_array()
        .ok_or("validation fixture requires steps array")?;
    if steps.is_empty() || steps.len() > MAX_STEPS {
        return Err("validation fixture must have 1-42 steps".into());
    }
    for step in steps {
        let fields = step.as_object().ok_or("fixture step must be an object")?;
        let tool = step["tool"].as_str().ok_or("fixture step requires tool")?;
        let _node = step["node"].as_str().ok_or("fixture step requires node")?;
        let args = step["args"]
            .as_array()
            .ok_or("fixture step requires args array")?;
        if args.len() > 4 || args.iter().any(|arg| !arg.is_string()) {
            return Err("fixture step requires 0-4 string args".into());
        }
        if !step["ok"].is_boolean() {
            return Err("fixture step requires boolean ok".into());
        }
        let check = is_check(tool);
        let expected_keys: &[&str] = if check {
            &[
                "node",
                "tool",
                "args",
                "ok",
                "expected",
                "observed",
                "evidence_id",
            ]
        } else if is_action(tool) {
            &["node", "tool", "args", "ok"]
        } else {
            return Err("fixture contains unsupported tool".into());
        };
        if fields.len() != expected_keys.len()
            || !fields
                .keys()
                .all(|key| expected_keys.contains(&key.as_str()))
        {
            return Err("fixture step has missing or unexpected fields".into());
        }
        if check {
            for key in ["expected", "observed", "evidence_id"] {
                if !step[key]
                    .as_str()
                    .is_some_and(|v| !v.trim().is_empty() && v.len() <= 256)
                {
                    return Err(format!("verification step requires bounded {key}").into());
                }
            }
        }
    }
    Ok(())
}

/// Stateless core: rejects unsupported graph shapes and malformed fixtures;
/// semantic mismatches produce a FAILED report rather than browser side effects.
fn replay(graph: &Value, fixture: &Value) -> Result<Value, Box<dyn Error>> {
    validate_fixture(fixture)?;
    if graph["mode"] != "auto" || graph["crystallization"]["reviewed"] != false {
        return Err(
            "only crystallized, unreviewed auto-routine revisions can be fixture-validated".into(),
        );
    }
    let context = input_context(fixture)?;
    validate_inputs(graph, &context)?;
    let nodes = graph["nodes"].as_object().ok_or("graph requires nodes")?;
    if nodes.len() > MAX_STEPS + 2 {
        return Err("validation graph exceeds node limit".into());
    }
    let cases = fixture["steps"].as_array().unwrap();
    if nodes.len() != cases.len() + 2
        || nodes["done"]["terminal"] != "success"
        || nodes["failed"]["terminal"] != "failure"
    {
        return Err("validation graph does not match the complete crystallized sequence".into());
    }
    let mut visited = HashSet::new();
    let mut current = graph["entry"]
        .as_str()
        .ok_or("graph requires entry")?
        .to_owned();
    let mut index = 0;
    let mut needs_verification = false;
    let mut checks = 0;
    let mut results = Vec::new();
    let mut failure: Option<&str> = None;
    loop {
        if !visited.insert(current.clone()) || visited.len() > MAX_STEPS + 1 {
            return Err("validation refuses loops and repeated nodes".into());
        }
        let node = nodes.get(&current).ok_or("graph points to missing node")?;
        match plan_node(&current, node, &context)? {
            PlannedNode::Terminal { success, .. } => {
                if !success
                    || current != "done"
                    || index != cases.len()
                    || needs_verification
                    || checks == 0
                {
                    failure = Some("non_success_terminal_or_incomplete_evidence");
                }
                break;
            }
            PlannedNode::Tool { name, args } => {
                // Limit this replay to the exact linear shape the crystallizer
                // generates: any other fields or transitions are refused.
                let is_verification = is_check(&name);
                if (!is_verification && !is_action(&name))
                    || node["next"].as_str().is_none()
                    || node["on_error"] != "failed"
                    || node.as_object().is_none_or(|obj| obj.len() != 4)
                    || (is_verification && !needs_verification)
                    || (!is_verification && needs_verification)
                {
                    return Err(
                        "validation supports only linear crystallized action/check pairs".into(),
                    );
                }
                if is_verification {
                    checks += 1;
                    needs_verification = false;
                } else {
                    needs_verification = true;
                }
                let case = cases.get(index);
                let matched = case.is_some_and(|c| {
                    c["node"] == current
                        && c["tool"] == name
                        && c["args"] == json!(args)
                        && c["ok"] == true
                        && (!is_verification || c["expected"] == c["observed"])
                });
                let evidence_hash = if is_verification {
                    case.and_then(|c| c["evidence_id"].as_str())
                        .map(|value| digest(value.as_bytes()))
                } else {
                    None
                };
                results.push(json!({
                    "node":current,"tool":name,"matched":matched,
                    "evidence_id_sha256":evidence_hash,
                }));
                index += 1;
                if !matched {
                    failure = Some("fixture_step_or_evidence_mismatch");
                    break;
                }
                current = node["next"].as_str().unwrap().to_owned();
            }
            _ => {
                return Err("validation refuses branches, agent, HITL and other node kinds".into());
            }
        }
    }
    if index > MAX_STEPS {
        return Err("validation step limit exceeded".into());
    }
    Ok(json!({
        "status":if failure.is_none() {"passed"} else {"failed"},
        "reason":failure,
        "validated_steps":results.len(),
        "verification_steps":checks,
        "steps":results,
    }))
}

pub(crate) fn store_report(root: &Path, report: &Value) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(root)?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    let id = report["validation_id"]
        .as_str()
        .ok_or("report is missing ID")?;
    let filename = root.join(format!("{id}.json"));
    let temporary = root.join(format!(".{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec_pretty(report)?)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        // Atomic create-new: neither a concurrent run nor a process crash
        // can expose a partially written report as a completed receipt.
        fs::hard_link(&temporary, filename)?;
        Ok::<_, Box<dyn Error>>(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

pub(crate) fn validate(
    routine_root: &Path,
    report_root: &Path,
    name: &str,
    version: u64,
    fixture: &Value,
) -> Result<Value, Box<dyn Error>> {
    // Pinned lookup; never reinterpret as the current active alias.
    let graph = routine_versions::exact(routine_root, name, version)?;
    let execution = replay(&graph, fixture)?;
    let report = json!({
        "validation_id":format!("validation-{:032x}", rand::random::<u128>()),
        "routine":name,
        "revision":version,
        "routine_sha256":digest(&serde_json::to_vec(&graph)?),
        "fixture_sha256":digest(&serde_json::to_vec(fixture)?),
        "created_at_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        "kind":"offline_fixture_replay",
        "browser_verified":false,
        "promotion_performed":false,
        "evidence_source":"caller_supplied_fixture",
        "execution":execution,
    });
    store_report(report_root, &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!(
                "jelly-validation-test-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn graph() -> Value {
        json!({"name":"sample","description":"Test crystallization", "mode":"auto",
        "entry":"step_000", "revision":2,
        "inputs":{"submit":{"type":"string","required":true}},
        "crystallization":{"reviewed":false, "trace_id":"trace-123"},
        "nodes":{
            "step_000":{"tool":"click","args":["{{submit}}"],"next":"step_001","on_error":"failed"},
            "step_001":{"tool":"assert-visible","args":["text:Done"],"next":"done","on_error":"failed"},
            "done":{"terminal":"success"},
            "failed":{"terminal":"failure"}
        }})
    }
    fn fixture() -> Value {
        json!({"vars":{"submit":"css:#submit"},
        "steps":[
            {"node":"step_000","tool":"click","args":["css:#submit"],"ok":true},
            {"node":"step_001","tool":"assert-visible","args":["text:Done"],
             "ok":true,"expected":"visible","observed":"visible",
             "evidence_id":"fixture:confirmed"}
        ]})
    }

    #[test]
    fn controlled_replay_passes_without_browser_and_keeps_report_private() {
        let dir = Temp::new();
        let routines = dir.0.join("routines");
        let reports = dir.0.join("reports");
        fs::create_dir(&routines).unwrap();
        let graph = graph();
        fs::write(
            routines.join("sample.json"),
            serde_json::to_vec(&graph).unwrap(),
        )
        .unwrap();
        let fixture = self::fixture();
        let report = validate(&routines, &reports, "sample", 2, &fixture).unwrap();
        assert_eq!(report["execution"]["status"], "passed");
        assert_eq!(report["execution"]["verification_steps"], 1);
        assert_eq!(report["browser_verified"], false);
        assert_eq!(report["promotion_performed"], false);
        assert_eq!(report["kind"], "offline_fixture_replay");
        assert_eq!(report["routine_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(
            fs::metadata(&reports).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let report_file = reports.join(format!(
            "{}.json",
            report["validation_id"].as_str().unwrap()
        ));
        assert_eq!(
            fs::metadata(&report_file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let persisted: Value = serde_json::from_slice(&fs::read(&report_file).unwrap()).unwrap();
        assert_eq!(persisted, report);
        assert!(!report.to_string().contains("css:#submit"));
        assert!(!report.to_string().contains("fixture:confirmed"));
        assert_eq!(
            fs::read(routines.join("sample.json")).unwrap(),
            serde_json::to_vec(&graph).unwrap()
        );
    }

    #[test]
    fn replay_rejects_wrong_evidence_missing_inputs_and_unsafe_shapes() {
        let graph = graph();
        let mut fixture = self::fixture();
        assert_eq!(replay(&graph, &fixture).unwrap()["status"], "passed");
        fixture["steps"][1]["observed"] = json!("not-visible");
        assert_eq!(replay(&graph, &fixture).unwrap()["status"], "failed");
        fixture["steps"][1]["observed"] = json!("visible");
        fixture["steps"][0]["args"] = json!(["css:#wrong"]);
        assert_eq!(replay(&graph, &fixture).unwrap()["status"], "failed");
        fixture["steps"][0]["args"] = json!(["css:#submit"]);
        fixture["vars"] = json!({});
        assert!(replay(&graph, &fixture).is_err());
        fixture = self::fixture();
        let mut unsafe_graph = graph.clone();
        unsafe_graph["nodes"]["step_000"]["tool"] = json!("fill");
        assert!(replay(&unsafe_graph, &fixture).is_err());
        unsafe_graph = graph.clone();
        unsafe_graph["nodes"]["step_000"]["next"] = json!("step_000");
        assert!(replay(&unsafe_graph, &fixture).is_err());
        unsafe_graph = graph.clone();
        unsafe_graph["mode"] = json!("jelly");
        assert!(replay(&unsafe_graph, &fixture).is_err());
        fixture["steps"][1]["unexpected"] = json!(true);
        assert!(replay(&graph, &fixture).is_err());
    }
}
