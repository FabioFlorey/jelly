use super::Result;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};
fn records(path: &PathBuf) -> Result<Vec<Value>> {
    let v: Value = serde_json::from_slice(&fs::read(path)?)?;
    Ok(v.as_array().ok_or("expected JSON array")?.clone())
}
fn subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|got| subset(v, got))),
        (Value::Array(e), Value::Array(a)) => {
            e.len() == a.len() && e.iter().zip(a).all(|(x, y)| subset(x, y))
        }
        _ => expected == actual,
    }
}
fn reference(cases: &[Value]) -> Vec<Value> {
    cases.iter().map(|c|json!({"case_id":c["id"],"tool":c["expected_tool"],"arguments":c["required_args"],"checks":c["checks"]})).collect()
}
fn score(cases: &[Value], predictions: &[Value]) -> Result<Value> {
    let mut case_ids = HashSet::new();
    for c in cases {
        let id = c["id"].as_str().ok_or("invalid case ID")?;
        if !case_ids.insert(id) {
            return Err("duplicate case ID".into());
        }
    }
    let mut by_id = HashMap::new();
    for p in predictions {
        let id = p["case_id"].as_str().ok_or("invalid prediction case ID")?;
        if !case_ids.contains(id) {
            return Err(format!("unknown case ID {id}").into());
        }
        if by_id.insert(id, p).is_some() {
            return Err("duplicate prediction case ID".into());
        }
    }
    let mut outcomes = vec![];
    let mut route = 0;
    let mut slots = 0;
    let mut checks = 0;
    let mut complete = 0;
    for c in cases {
        let p = by_id
            .get(c["id"].as_str().unwrap())
            .copied()
            .unwrap_or(&Value::Null);
        let r = p["tool"] == c["expected_tool"]
            && !c["avoid"]
                .as_array()
                .is_some_and(|avoid| avoid.contains(&p["tool"]));
        let sl = r
            && subset(&c["required_args"], &p["arguments"])
            && !(p["tool"] == "browser-call" && p["arguments"]["on_error"] == "continue");
        let ck = p["checks"].as_array().is_some_and(|offered| {
            offered.iter().all(Value::is_string)
                && c["checks"]
                    .as_array()
                    .is_some_and(|required| required.iter().all(|v| offered.contains(v)))
        });
        route += usize::from(r);
        slots += usize::from(sl);
        checks += usize::from(ck);
        complete += usize::from(r && sl && ck);
        outcomes.push(
            json!({"id":c["id"],"route":r,"slots":sl,"planned_checks":ck,"complete":r&&sl&&ck}),
        );
    }
    let n = cases.len();
    let frac = |x: usize| if n == 0 { 0.0 } else { x as f64 / n as f64 };
    Ok(
        json!({"total":n,"predicted":predictions.len(),"tool_selection_accuracy":frac(route),"slot_filling_accuracy":frac(slots),"verification_plan_coverage":frac(checks),"combined_plan_accuracy":frac(complete),"task_outcome_success_rate":null,"cases":outcomes}),
    )
}
pub fn run(root: &std::path::Path, args: Vec<String>) -> Result<()> {
    let cases = records(&root.join("tests/fixtures/agent-guidance-cases.json"))?;
    if args == ["--self-test"] {
        let good = reference(&cases);
        assert_eq!(
            score(&cases, &good)?["combined_plan_accuracy"].as_f64(),
            Some(1.0)
        );
        let mut bad = good.clone();
        bad[0]["tool"] = json!("cdp-call");
        bad[1]["arguments"] = json!({"action":"schema"});
        bad[2]["checks"] = json!([]);
        let outcome = score(&cases, &bad)?;
        assert!(outcome["tool_selection_accuracy"].as_f64().unwrap() < 1.0);
        assert!(outcome["slot_filling_accuracy"].as_f64().unwrap() < 1.0);
        assert!(outcome["verification_plan_coverage"].as_f64().unwrap() < 1.0);
        assert!(score(&cases, &[good.clone(), vec![good[0].clone()]].concat()).is_err());
        println!(
            "PASS: {} routing/slot/verification cases and negative controls; no browser/model calls",
            cases.len()
        );
        return Ok(());
    }
    let mut predictions = None;
    let mut emit = None;
    let mut report = None;
    let mut strict = false;
    let mut it = args.into_iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--predictions" => {
                predictions = Some(PathBuf::from(it.next().ok_or("missing predictions path")?))
            }
            "--emit-reference" => {
                emit = Some(PathBuf::from(
                    it.next().ok_or("missing reference output path")?,
                ))
            }
            "--report" => {
                report = Some(PathBuf::from(
                    it.next().ok_or("missing report output path")?,
                ))
            }
            "--strict" => strict = true,
            _ => return Err(format!("unknown guidance argument {flag}").into()),
        }
    }
    if let Some(path) = emit {
        fs::write(
            &path,
            serde_json::to_string_pretty(&reference(&cases))? + "\n",
        )?;
        println!(
            "Wrote {} reference decisions to {}",
            cases.len(),
            path.display()
        );
        return Ok(());
    }
    let predictions =
        records(&predictions.ok_or("use --self-test, --emit-reference or --predictions")?)?;
    let result = score(&cases, &predictions)?;
    let mut summary = result.clone();
    summary.as_object_mut().unwrap().remove("cases");
    println!("{}", serde_json::to_string_pretty(&summary)?);
    if let Some(report) = report {
        fs::write(report, serde_json::to_string_pretty(&result)? + "\n")?;
    }
    if strict && result["combined_plan_accuracy"] != 1 {
        return Err("not all guidance cases passed".into());
    }
    Ok(())
}
