use crate::primitives::{arg, target};
use crate::{BrowserSession, Error, ErrorKind, Target, jelly_error};
use serde_json::{Value, json};
use std::{thread, time::Duration};

const SWITCH_TAB_USAGE: &str = "usage: switch-tab <label|id|title|url>";
const OPEN_IN_NEW_TAB_USAGE: &str = "usage: open-in-new-tab <target>";

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
        .join("\n"))
}

pub fn switch_tab(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let query = arg(args, 0, SWITCH_TAB_USAGE)?;
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

    close_target(browser, &current_id)?;
    browser.refresh_target_registry()?;

    match browser.logical_targets()?.first() {
        Some(next) => {
            let next_id = next.target_id().to_owned();
            let next_label = next.label().to_owned();
            browser.switch_target(&next_id)?;
            Ok(format!(
                "Closed {current_label}; active {next_label} — target:{next_id}"
            ))
        }
        None => Ok(format!("Closed {current_label}; no page targets remain")),
    }
}

pub fn open_in_new_tab(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, OPEN_IN_NEW_TAB_USAGE)?;
    let url = resolve_openable_url(browser, &target)?;
    let target_id = create_page_target(browser, &url)?;

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

fn close_target(browser: &mut BrowserSession, target_id: &str) -> Result<(), Error> {
    browser.browser_call("Target.closeTarget", json!({"targetId":target_id}))?;
    thread::sleep(Duration::from_millis(50));
    Ok(())
}

fn resolve_openable_url(browser: &mut BrowserSession, target: &Target) -> Result<String, Error> {
    let value = browser.eval(&format!(
        r#"(()=>{{const e={};if(!e)return null;const a=e.closest('a[href]');return a?.href||(e.tagName==='IMG'?(e.currentSrc||e.src):null)}})()"#,
        target.js_resolver()
    ))?;
    value.as_str().map(str::to_owned).ok_or_else(|| {
        jelly_error(
            ErrorKind::TargetNotFound,
            "target has no link or image URL",
            true,
        )
    })
}

fn create_page_target(browser: &mut BrowserSession, url: &str) -> Result<String, Error> {
    let created = browser.browser_call("Target.createTarget", json!({"url":url}))?;
    created_target_id(&created)
}

fn created_target_id(created: &Value) -> Result<String, Error> {
    created["result"]["targetId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "Target.createTarget response is missing targetId",
                false,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn create_target_response_requires_target_id() {
        let error = created_target_id(&json!({"result":{}})).unwrap_err();
        assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
    }
}
