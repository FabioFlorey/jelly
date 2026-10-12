//! Bounded live replay in a dedicated, headless Chromium and static data-page.
use crate::core::routines::{PlannedNode, plan_node, validate_inputs};
use crate::shell::{routine_validation, routine_versions};
use base64::{Engine, engine::general_purpose::STANDARD};
use rustwright::{ActionOptions, GotoOptions, LaunchOptions, chromium};
use serde_json::{Map, Value, json};
use std::{
    collections::HashSet,
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

struct Step {
    node: String,
    tool: String,
    args: Vec<String>,
    check: bool,
}
fn action(s: &str) -> bool {
    matches!(s, "click" | "check" | "select" | "scroll")
}
fn check(s: &str) -> bool {
    matches!(
        s,
        "assert-visible" | "assert-text" | "assert-title" | "assert-image-ready" | "wait-for"
    )
}
fn selector(s: &str) -> Result<&str, Box<dyn Error>> {
    let q = s
        .strip_prefix("css:")
        .ok_or("only CSS selectors supported in isolated browser")?;
    if q.is_empty() || q.len() > 180 || q.contains('\0') {
        return Err("invalid CSS selector".into());
    }
    Ok(q)
}
type HtmlFixtureContext<'a> = (&'a str, Map<String, Value>);
fn has_event_handler_attribute(html: &str) -> bool {
    let bytes = html.as_bytes();
    for i in 1..bytes.len().saturating_sub(2) {
        if bytes[i] != b'o'
            || bytes[i + 1] != b'n'
            || !(bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b'/')
        {
            continue;
        }
        let mut j = i + 2;
        while j < bytes.len()
            && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'-' | b'_'))
        {
            j += 1;
        }
        if j == i + 2 {
            continue;
        }
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) == Some(&b'=') {
            return true;
        }
    }
    false
}

fn fixture(f: &Value) -> Result<HtmlFixtureContext<'_>, Box<dyn Error>> {
    let fields = f.as_object().ok_or("fixture must be object")?;
    if fields.len() != 2 || !fields.contains_key("html") || !fields.contains_key("vars") {
        return Err("live fixture requires exactly html and vars".into());
    }
    if serde_json::to_vec(f)?.len() > 16384 {
        return Err("live fixture exceeds 16KiB".into());
    }
    let html = f["html"].as_str().ok_or("html must be string")?;
    if html.trim().is_empty() {
        return Err("html must not be empty".into());
    }
    let lower = html.to_ascii_lowercase();
    if [
        "<script",
        "<iframe",
        "<frame",
        "<object",
        "<embed",
        "<form",
        "<meta",
        "<base",
        "<link",
        "<svg",
        "<math",
        "<video",
        "<audio",
        "javascript:",
        "http:",
        "https:",
        "file:",
        "src=",
        "href=",
        "onload",
        "onclick",
        "onerror",
        "onfocus",
        "onchange",
        "oninput",
        "@import",
        "url(",
        "<style",
        "formaction",
        "srcdoc",
    ]
    .iter()
    .any(|token| lower.contains(token))
        || has_event_handler_attribute(&lower)
    {
        return Err("active or external HTML not allowed in live fixture".into());
    }
    Ok((html, routine_validation::input_context(f)?))
}
fn plan(graph: &Value, vars: &Map<String, Value>) -> Result<Vec<Step>, Box<dyn Error>> {
    if graph["mode"] != "auto" || graph["crystallization"]["reviewed"] != false {
        return Err("only crystallized auto candidate can be live-validated".into());
    }
    validate_inputs(graph, vars)?;
    let nodes = graph["nodes"].as_object().ok_or("missing nodes")?;
    if nodes.len() < 4
        || nodes.len() > 44
        || nodes["done"]["terminal"] != "success"
        || nodes["failed"]["terminal"] != "failure"
    {
        return Err("only bounded crystallized graphs accepted".into());
    }
    let mut name = graph["entry"].as_str().ok_or("missing entry")?.to_owned();
    let mut visited = HashSet::new();
    let mut expect_check = false;
    let mut out = Vec::new();
    loop {
        if !visited.insert(name.clone()) || visited.len() > 43 {
            return Err("loops forbidden".into());
        }
        let node = nodes.get(&name).ok_or("missing graph node")?;
        match plan_node(&name, node, vars)? {
            PlannedNode::Tool { name: tool, args } => {
                let checking = check(&tool);
                if !(checking || action(&tool))
                    || checking != expect_check
                    || node.as_object().is_none_or(|obj| obj.len() != 4)
                    || node["on_error"] != "failed"
                    || !node["next"].is_string()
                    || args.len() > 3
                    || args.iter().any(|arg| arg.len() > 256)
                {
                    return Err("unsupported live step or sequence".into());
                }
                match tool.as_str() {
                    "click" | "check" | "assert-visible" | "assert-image-ready" => {
                        if args.len() != 1 {
                            return Err("invalid target args".into());
                        }
                        selector(&args[0])?;
                    }
                    "select" | "assert-text" => {
                        if args.len() != 2 && !(tool == "assert-text" && args.len() == 3) {
                            return Err("invalid select/text args".into());
                        }
                        selector(&args[0])?;
                    }
                    "assert-title" => {
                        if args.is_empty() || args.len() > 2 {
                            return Err("invalid title args".into());
                        }
                    }
                    "scroll" => {
                        if args.len() > 1
                            || args.first().is_some_and(|v| {
                                !matches!(v.as_str(), "down" | "up" | "top" | "bottom")
                            })
                        {
                            return Err("invalid scroll args".into());
                        }
                    }
                    "wait-for" => {
                        if !(2..=3).contains(&args.len())
                            || !matches!(
                                args[0].as_str(),
                                "exists" | "visible" | "hidden" | "text" | "title"
                            )
                        {
                            return Err("invalid wait-for predicate".into());
                        }
                        if matches!(args[0].as_str(), "exists" | "visible" | "hidden") {
                            selector(&args[1])?;
                        }
                        if args.len() == 3
                            && args[2].parse::<u64>().ok().is_none_or(|s| s == 0 || s > 3)
                        {
                            return Err("wait-for budget must be 1..=3 seconds".into());
                        }
                    }
                    _ => return Err("unsupported browser primitive".into()),
                }
                out.push(Step {
                    node: name.clone(),
                    tool,
                    args,
                    check: checking,
                });
                expect_check = !checking;
                name = node["next"].as_str().unwrap().to_owned();
            }
            PlannedNode::Terminal { success: true, .. } if name == "done" => {
                if expect_check || out.len() < 2 || out.len() > 42 || nodes.len() != out.len() + 2 {
                    return Err("incomplete crystallized graph".into());
                }
                return Ok(out);
            }
            _ => return Err("unsupported non-linear browser graph".into()),
        }
    }
}
struct Isolated {
    browser: rustwright::Browser,
    profile: PathBuf,
}
impl Drop for Isolated {
    fn drop(&mut self) {
        let _ = self.browser.close();
        let _ = fs::remove_dir_all(&self.profile);
    }
}
fn launch() -> Result<Isolated, Box<dyn Error>> {
    let profile =
        std::env::temp_dir().join(format!("jelly-live-check-{:032x}", rand::random::<u128>()));
    fs::create_dir(&profile)?;
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))?;
    let mut options = LaunchOptions::default()
        .headless(true)
        .executable_path("/usr/bin/chromium")
        .timeout(Some(12000.0));
    options.user_data_dir = Some(profile.to_string_lossy().into_owned());
    options.chromium_sandbox = true;
    for flag in [
        "--no-first-run",
        "--disable-background-networking",
        "--disable-sync",
        "--disable-extensions",
        "--disable-default-apps",
        "--no-proxy-server",
        "--host-resolver-rules=MAP * ~NOTFOUND",
        "--disable-breakpad",
        "--disable-component-update",
        "--disable-features=Translate",
    ] {
        options = options.arg(flag);
    }
    let browser = match chromium().launch(options) {
        Ok(browser) => browser,
        Err(e) => {
            let _ = fs::remove_dir_all(profile);
            return Err(format!("isolated Chromium launch failed: {e}").into());
        }
    };
    Ok(Isolated { browser, profile })
}
fn eval(page: &rustwright::Page, script: &str, arg: Value) -> Result<Value, Box<dyn Error>> {
    Ok(page.evaluate(script, Some(&arg), ActionOptions::timeout(2000.0))?)
}
fn visible(page: &rustwright::Page, selector: &str) -> Result<Value, Box<dyn Error>> {
    eval(
        page,
        r#"(selector)=>{
        const e=document.querySelector(selector);
        if(!e)return {exists:false,visible:false,ready:false};
        const s=getComputedStyle(e),r=e.getBoundingClientRect();
        const v=s.display!=='none'&&s.visibility!=='hidden'&&Number(s.opacity)>0&&r.width>0&&r.height>0;
        return {exists:true,visible:v,ready:e instanceof HTMLImageElement&&e.complete&&e.naturalWidth>0};
    }"#,
        json!(selector),
    )
}
fn run_step(page: &rustwright::Page, step: &Step) -> Result<(bool, Value), Box<dyn Error>> {
    let a = &step.args;
    let result = match step.tool.as_str() {
        "click" => {
            page.click(selector(&a[0])?, ActionOptions::timeout(2000.0))?;
            (true, json!({"clicked":true}))
        }
        "check" => {
            let v = eval(
                page,
                r#"(s)=>{const e=document.querySelector(s);
                if(!e||e.tagName!=='INPUT'||e.type!=='checkbox'||e.disabled)return false;
                if(!e.checked)e.click();return e.checked;}"#,
                json!(selector(&a[0])?),
            )?;
            (v == true, json!({"checked":v}))
        }
        "select" => {
            let v = eval(
                page,
                r#"([s,v])=>{const e=document.querySelector(s);
                if(!e||e.tagName!=='SELECT'||e.disabled)return false;
                const opt=[...e.options].find(x=>x.value===v||x.text.trim()===v);
                if(!opt)return false;e.value=opt.value;e.dispatchEvent(new Event('change',{bubbles:true}));return true;}"#,
                json!([selector(&a[0])?, a[1]]),
            )?;
            (v == true, json!({"selected":v}))
        }
        "scroll" => {
            let direction = a.first().map(String::as_str).unwrap_or("down");
            let v = eval(
                page,
                r#"(d)=>{if(d==='top')scrollTo(0,0);
                else if(d==='bottom')scrollTo(0,document.body.scrollHeight);
                else scrollBy(0,d==='up'?-innerHeight*.8:innerHeight*.8);return true;}"#,
                json!(direction),
            )?;
            (v == true, json!({"scroll":v}))
        }
        "assert-visible" | "assert-image-ready" => {
            let v = visible(page, selector(&a[0])?)?;
            (
                v["visible"] == true && (step.tool == "assert-visible" || v["ready"] == true),
                v,
            )
        }
        "assert-text" => {
            let v = eval(
                page,
                r#"(s)=>{const e=document.querySelector(s);
                return e?(e.innerText||e.textContent||'').trim():null;}"#,
                json!(selector(&a[0])?),
            )?;
            let actual = v.as_str().unwrap_or("");
            let matched = match a.get(2).map(String::as_str).unwrap_or("contains") {
                "exact" => actual == a[1],
                "contains" => actual.contains(&a[1]),
                _ => return Err("invalid comparison mode".into()),
            };
            (matched, json!({"actual":v,"matched":matched}))
        }
        "assert-title" => {
            let actual = page.title(ActionOptions::timeout(2000.0))?;
            let ok = match a.get(1).map(String::as_str).unwrap_or("contains") {
                "exact" => actual == a[0],
                "contains" => actual.contains(&a[0]),
                _ => return Err("invalid comparison mode".into()),
            };
            (ok, json!({"title":actual,"matched":ok}))
        }
        "wait-for" => {
            let budget = a.get(2).and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
            let deadline = std::time::Instant::now() + Duration::from_secs(budget);
            loop {
                let (ok, v) = match a[0].as_str() {
                    "visible" | "exists" | "hidden" => {
                        let v = visible(page, selector(&a[1])?)?;
                        (
                            match a[0].as_str() {
                                "visible" => v["visible"] == true,
                                "exists" => v["exists"] == true,
                                _ => v["visible"] != true,
                            },
                            v,
                        )
                    }
                    "title" | "text" => {
                        let actual = if a[0] == "title" {
                            page.title(ActionOptions::timeout(2000.0))?
                        } else {
                            eval(page, "() => document.body?.innerText || ''", Value::Null)?
                                .as_str()
                                .unwrap_or("")
                                .to_owned()
                        };
                        (actual.contains(&a[1]), json!({"value":actual}))
                    }
                    _ => return Err("invalid wait condition".into()),
                };
                if ok || std::time::Instant::now() >= deadline {
                    break (ok, v);
                }
                thread::sleep(Duration::from_millis(60));
            }
        }
        _ => return Err("unsupported live primitive".into()),
    };
    Ok(result)
}
fn execute(html: &str, steps: &[Step]) -> Result<Value, Box<dyn Error>> {
    let isolated = launch()?;
    let page = isolated.browser.new_page()?;
    let csp = "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; script-src 'none'; connect-src 'none'; img-src data:; style-src 'unsafe-inline'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'\">";
    let url = format!(
        "data:text/html;base64,{}",
        STANDARD.encode(format!("{csp}{html}"))
    );
    page.goto(
        &url,
        GotoOptions::default()
            .wait_until("domcontentloaded")
            .timeout(5000.0),
    )?;
    let mut outcomes = Vec::new();
    let mut checked = 0;
    let mut failure = None;
    for step in steps {
        let (ok, v) = match run_step(&page, step) {
            Ok(result) => result,
            Err(_) => (false, json!({"tool_error":true})),
        };
        if ok && step.check {
            checked += 1;
        }
        outcomes.push(json!({"node":step.node,"tool":step.tool,"matched":ok,
            "observed_sha256":routine_validation::digest(&serde_json::to_vec(&v)?)}));
        if !ok {
            failure = Some(if step.check {
                "browser_assertion_failed"
            } else {
                "browser_action_failed"
            });
            break;
        }
    }
    Ok(
        json!({"status":if failure.is_none(){"passed"}else{"failed"},
        "reason":failure,"validated_steps":outcomes.len(),"verification_steps":checked,"steps":outcomes}),
    )
}
pub(crate) fn validate(
    routines: &Path,
    reports: &Path,
    name: &str,
    revision: u64,
    fixture_value: &Value,
) -> Result<Value, Box<dyn Error>> {
    let graph = routine_versions::exact(routines, name, revision)?;
    let (html, vars) = fixture(fixture_value)?;
    let steps = plan(&graph, &vars)?;
    let execution = execute(html, &steps)?;
    let report = json!({
        "validation_id":format!("validation-{:032x}",rand::random::<u128>()),
        "routine":name,"revision":revision,
        "routine_sha256":routine_validation::digest(&serde_json::to_vec(&graph)?),
        "fixture_sha256":routine_validation::digest(&serde_json::to_vec(fixture_value)?),
        "created_at_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
        "kind":"isolated_chromium_validation",
        "evidence_source":"isolated_chromium_dom",
        "browser_verified":execution["status"]=="passed",
        "isolation":"temporary_headless_profile",
        "external_network":"restricted_by_fixture_csp_and_chromium_flags",
        "promotion_performed":false,"execution":execution
    });
    routine_validation::store_report(reports, &report)?;
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn graph() -> Value {
        json!({"name":"sample","description":"Candidate","revision":2,"mode":"auto",
        "crystallization":{"reviewed":false},"entry":"step_000",
        "inputs":{"check_target":{"type":"string","required":true}},
        "nodes":{"step_000":{"tool":"check","args":["{{check_target}}"],"next":"step_001","on_error":"failed"},
        "step_001":{"tool":"assert-visible","args":["css:#check"],"next":"done","on_error":"failed"},
        "done":{"terminal":"success"},"failed":{"terminal":"failure"}}})
    }
    #[test]
    fn refuses_active_html_and_non_linear_graphs_before_launch() {
        let mut f = json!({"html":"<input id='check' type='checkbox'>","vars":{"check_target":"css:#check"}});
        let (_, vars) = fixture(&f).unwrap();
        assert_eq!(plan(&graph(), &vars).unwrap().len(), 2);
        for html in [
            "<script>alert(1)</script>",
            "<img src='http://example.org'>",
            "<button onclick='alert(1)'>",
            "<button onpointerdown='alert(1)'>",
            "<iframe srcdoc='hi'>",
        ] {
            f["html"] = json!(html);
            assert!(fixture(&f).is_err());
        }
        f["html"] = json!("<p>ok</p>");
        f["vars"] = json!({});
        let (_, vars) = fixture(&f).unwrap();
        assert!(plan(&graph(), &vars).is_err());
        let mut bad = graph();
        bad["nodes"]["step_000"]["tool"] = json!("fill");
        assert!(
            plan(
                &bad,
                &routine_validation::input_context(&json!({"vars":{"check_target":"css:#check"}}))
                    .unwrap()
            )
            .is_err()
        );
    }
}
