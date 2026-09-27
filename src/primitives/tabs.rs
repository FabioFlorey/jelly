use crate::primitives::target;
use crate::{ACTIVE_TARGET, BrowserSession, Error, ErrorKind, jelly_error};
use serde_json::json;
use std::{fs, thread, time::Duration};

pub fn tabs(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let v = b.browser_call("Target.getTargets", json!({}))?;
    let mut out = Vec::new();
    if let Some(xs) = v["result"]["targetInfos"].as_array() {
        for x in xs.iter().filter(|x| x["type"] == "page") {
            out.push(format!(
                "{} — {} — {}",
                x["targetId"].as_str().unwrap_or(""),
                x["title"].as_str().unwrap_or(""),
                x["url"].as_str().unwrap_or("")
            ))
        }
    }
    Ok(out.join("\n"))
}
pub fn switch_tab(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let q = args.first().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            "usage: switch-tab <id|title|url>",
            false,
        )
    })?;
    let v = b.browser_call("Target.getTargets", json!({}))?;
    let t = v["result"]["targetInfos"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|x| {
                x["type"] == "page"
                    && (x["targetId"].as_str() == Some(q)
                        || x["title"].as_str().is_some_and(|s| s.contains(q))
                        || x["url"].as_str().is_some_and(|s| s.contains(q)))
            })
        })
        .and_then(|x| x["targetId"].as_str())
        .ok_or_else(|| jelly_error(ErrorKind::TargetNotFound, "tab not found", true))?
        .to_owned();
    b.switch_target(&t)?;
    Ok(format!("Switched to {t}"))
}
pub fn close_tab(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let current = b.target_id().to_owned();
    b.browser_call("Target.closeTarget", json!({"targetId":current}))?;
    let _ = fs::remove_file(ACTIVE_TARGET);
    thread::sleep(Duration::from_millis(50));
    let v = b.browser_call("Target.getTargets", json!({}))?;
    if let Some(t) = v["result"]["targetInfos"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["type"] == "page"))
        .and_then(|x| x["targetId"].as_str())
        .map(str::to_owned)
    {
        b.switch_target(&t)?
    }
    Ok("Tab closed.".into())
}
pub fn open_in_new_tab(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: open-in-new-tab <target>")?;
    let v=b.eval(&format!(r#"(()=>{{const e={};if(!e)return null;const a=e.closest('a[href]');return a?.href||(e.tagName==='IMG'?(e.currentSrc||e.src):null)}})()"#,t.js_resolver()))?;
    let url = v
        .as_str()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::TargetNotFound,
                "target has no link or image URL",
                true,
            )
        })?
        .to_owned();
    let out = b.browser_call("Target.createTarget", json!({"url":url}))?;
    let id = out["result"]["targetId"]
        .as_str()
        .ok_or("new tab target missing")?
        .to_owned();
    b.switch_target(&id)?;
    Ok(format!("Opened in new tab: {url}"))
}
