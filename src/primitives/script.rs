use crate::primitives::pretty;
use crate::{BrowserSession, Error, INJECTION_DIR};
use std::fs;

pub fn evaluate_js(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.is_empty() {
        return Err("usage: evaluate-js <expression>".into());
    }
    Ok(pretty(&b.eval(&args.join(" "))?))
}
pub fn inject_js(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let mut a = args.to_vec();
    let persistent = if let Some(i) = a.iter().position(|x| x == "--persistent") {
        a.remove(i);
        true
    } else {
        false
    };
    let source = if let Some(i) = a.iter().position(|x| x == "--file") {
        fs::read_to_string(a.get(i + 1).ok_or("--file requires path")?)?
    } else {
        a.join(" ")
    };
    if source.trim().is_empty() {
        return Err("usage: inject-js [--persistent] [--file path | <script>]".into());
    }
    let id = if persistent {
        fs::create_dir_all(INJECTION_DIR)?;
        let id = format!("pbj-{}", std::process::id());
        fs::write(format!("{INJECTION_DIR}/injected-{id}.js"), &source)?;
        Some(id)
    } else {
        None
    };
    b.eval(&format!("(()=>{{\n{source}\n}})()"))?;
    Ok(pretty(
        &serde_json::json!({"injected":true,"persistent":persistent,"identifier":id}),
    ))
}
