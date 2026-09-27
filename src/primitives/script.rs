use crate::primitives::pretty;
use crate::{BrowserSession, Error, ErrorKind, INJECTION_DIR, jelly_error};
use std::fs;

pub fn evaluate_js(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.is_empty() {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: evaluate-js <expression>",
            false,
        ));
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
        fs::read_to_string(a.get(i + 1).ok_or_else(|| {
            jelly_error(ErrorKind::InvalidArguments, "--file requires path", false)
        })?)?
    } else {
        a.join(" ")
    };
    if source.trim().is_empty() {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: inject-js [--persistent] [--file path | <script>]",
            false,
        ));
    }
    let id = if persistent {
        fs::create_dir_all(INJECTION_DIR)?;
        let id = format!("jelly-{}", std::process::id());
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
