use crate::primitives::target;
use crate::{BrowserSession, Error, ErrorKind, jelly_error};
use serde_json::json;
use std::{thread, time::Duration};

pub fn tabs(browser: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let targets = browser.logical_targets()?;
    Ok(targets
        .iter()
        .map(|target| {
            format!(
                "{} — {} — {} — target:{}",
                target.label(),
                target.title(),
                target.url(),
                target.target_id()
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        ))
}

pub fn switch_tab(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let query = args.first().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            "usage: switch-tab <label|id|title|url>",
            false,
        )
    })?;
    let (label, target_id) = browser.resolve_target_query(query)?;
    browser.switch_target(&target_id)?;
    Ok(format!("Switched to {label} — target:{target_id}"))
}

pub fn close_tab(browser: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let current_id = browser.target_id().to_owned();
    let current_label = browser
        .current_target_label()
        .map(str::to_owned)
        .unwrap_or_else(|| current_id.clone());

    browser.browser_call("Target.closeTarget", json!({"targetId":current_id}))?;
    thread::sleep(Duration::from_millis(50));
    browser.refresh_target_registry()?;

    let remaining = browser.logical_targets()?;
    if let Some(next) = remaining.first() {
        let next_id = next.target_id().to_owned();
        let next_label = next.label().to_owned();
        browser.switch_target(&next_id)?;
        Ok(format!(
            "Closed {current_label}; active {next_label} — target:{next_id}"
        ))
    } else {
        Ok(format!("Closed {current_label}; no page targets remain"))
    }
}

pub fn open_in_new_tab(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, "usage: open-in-new-tab <target>")?;
    let value=browser.eval(&format!(r#"(()=>{{const e={};if(!e)return null;const a=e.closest('a[href]');return a?.href||(e.tagName==='IMG'?(e.currentSrc||e.src):null)}})()"#,target.js_resolver()))?;
    let url = value
        .as_str()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::TargetNotFound,
                "target has no link or image URL",
                true,
            )
        })?
        .to_owned();

    let created = browser.browser_call("Target.createTarget", json!({"url":url}))?;
    let target_id = created["result"]["targetId"]
        .as_str()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "Target.createTarget response is missing targetId",
                false,
            )
        })?
        .to_owned();

    browser.switch_target(&target_id)?;
    let label = browser
        .current_target_label()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "new page target has no logical label after registry refresh",
                false,
            )
        })?
        .to_owned();

    Ok(format!("Opened {label}: {url} — target:{target_id}"))
}
