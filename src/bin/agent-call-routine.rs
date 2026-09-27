use jelly::{BrowserSession, ROUTINE_STATE_DIR, execute_browser_primitive, is_browser_primitive};
use serde_json::json;
use std::{
    collections::HashMap,
    env, fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const ROUTINES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.agent/routines");
const STATE: &str = ROUTINE_STATE_DIR;

fn vars(args: &[String]) -> HashMap<String, String> {
    args.iter()
        .filter_map(|x| x.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn render(mut s: String, vars: &HashMap<String, String>) -> String {
    for (k, v) in vars {
        s = s
            .replace(&format!("{{{{ {k} }}}}"), v)
            .replace(&format!("{{{{{k}}}}}"), v);
    }
    s
}

fn split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur)
    }
    out
}

fn state_id() -> String {
    format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

fn run(
    lines: &[String],
    start: usize,
    mut vars: HashMap<String, String>,
    id: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(STATE)?;
    let mut browser: Option<BrowserSession> = None;
    for (i, raw) in lines.iter().enumerate().skip(start) {
        let line = render(raw.trim().to_owned(), &vars);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts = split(&line);
        if parts.is_empty() {
            continue;
        }
        match parts[0].as_str() {
            "signal" => {
                let cmd = parts.get(1).ok_or("signal requires a tool")?;
                let output = if is_browser_primitive(cmd) {
                    if browser.is_none() {
                        browser = Some(BrowserSession::connect()?);
                    }
                    execute_browser_primitive(browser.as_mut().unwrap(), cmd, &parts[2..])?
                } else {
                    tool(cmd, &parts[2..])?
                };
                println!("[signal:{cmd}]\n{}", output.trim());
            }
            "handoff" => {
                let sid = id.clone().unwrap_or_else(state_id);
                let message = parts[1..].join(" ");
                let body = json!({"next":i+1,"lines":lines,"vars":vars});
                fs::write(format!("{STATE}/{sid}.state"), serde_json::to_vec(&body)?)?;
                println!("handoff {sid} {message}");
                return Ok(());
            }
            "hitl" => {
                let message = parts.get(1..).unwrap_or_default().join(" ");
                if message.is_empty() {
                    return Err("hitl requires a message".into());
                }
                tool("hitl", &parts[1..])?;
                let sid = id.clone().unwrap_or_else(state_id);
                let body = json!({"next":i+1,"lines":lines,"vars":vars});
                fs::write(format!("{STATE}/{sid}.state"), serde_json::to_vec(&body)?)?;
                println!("hitl {sid} {message}");
                return Ok(());
            }
            "set" => {
                let kv = parts.get(1).ok_or("set requires name=value")?;
                let (k, v) = kv.split_once('=').ok_or("set requires name=value")?;
                vars.insert(k.to_owned(), v.to_owned());
            }
            cmd => {
                let output = if is_browser_primitive(cmd) {
                    if browser.is_none() {
                        browser = Some(BrowserSession::connect()?);
                    }
                    execute_browser_primitive(browser.as_mut().unwrap(), cmd, &parts[1..])?
                } else {
                    tool(cmd, &parts[1..])?
                };
                if !output.trim().is_empty() {
                    println!("{}", output.trim())
                }
            }
        }
    }
    if let Some(id) = id {
        let _ = fs::remove_file(format!("{STATE}/{id}.state"));
    }
    Ok(())
}

fn tool(name: &str, args: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let bin_dir = env::current_exe()?
        .parent()
        .ok_or("call-routine executable has no parent")?
        .to_path_buf();
    let target = bin_dir.join(format!("agent-{name}"));
    if !target.exists() {
        return Err(format!("unknown/unbuilt tool: {name}").into());
    }
    let bin = bin_dir.join("agent-run");
    let out = Command::new(bin).arg(name).args(args).output()?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr)
            .trim()
            .to_owned()
            .into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn resume(id: &str, extra: HashMap<String, String>) -> Result<(), Box<dyn std::error::Error>> {
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(format!("{STATE}/{id}.state"))?)?;
    let start = value["next"].as_u64().ok_or("bad state")? as usize;
    let lines = value["lines"]
        .as_array()
        .ok_or("bad state")?
        .iter()
        .filter_map(|x| x.as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let mut vars = value["vars"]
        .as_object()
        .ok_or("bad state")?
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
        .collect::<HashMap<_, _>>();
    vars.extend(extra);
    run(&lines, start, vars, Some(id.to_owned()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("resume") {
        return resume(
            args.get(1)
                .ok_or("usage: call-routine resume <id> [key=value]")?,
            vars(&args[2..]),
        );
    }
    let name = args
        .first()
        .ok_or("usage: call-routine <name> [key=value]")?;
    let path = format!("{ROUTINES}/{name}.jinja");
    let lines = fs::read_to_string(path)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    run(&lines, 0, vars(&args[1..]), None)
}
