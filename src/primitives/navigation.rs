use crate::primitives::js_helpers::LAYOUT_VISIBLE_FN;
use crate::primitives::{arg, js};
use crate::{BrowserSession, Error, ErrorKind, INJECTION_DIR, jelly_error};
use serde_json::{Value, json};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

const WAIT_USAGE: &str = "usage: wait <text|css|visible|url|gone|js> <value> [seconds]";
const NAVIGATE_USAGE: &str = "usage: navigate <url>";
const TAB_HISTORY_USAGE: &str = "usage: tab-history <back|forward>";

struct WaitRequest<'a> {
    condition: &'a str,
    value: &'a str,
    seconds: u64,
}

pub fn wait(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_wait_request(args)?;
    let expression = wait_expression(request.condition, request.value)?;

    let start = Instant::now();
    loop {
        match browser.eval(&expression) {
            Ok(value) if value.as_bool() == Some(true) => return Ok("Condition met.".into()),
            Ok(_) | Err(_) => {}
        }

        if start.elapsed() > Duration::from_secs(request.seconds) {
            return Err(jelly_error(
                ErrorKind::ConditionTimeout,
                format!(
                    "wait timed out after {}s for {} {:?}",
                    request.seconds, request.condition, request.value
                ),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

pub fn navigate(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let url = arg(args, 0, NAVIGATE_USAGE)?;

    install_injections_for_future_documents(browser)?;
    navigate_to(browser, url)?;
    wait_for_document_ready(browser, url)?;
    apply_injections_to_current_document(browser);

    navigation_result(browser, url)
}

pub fn tab_history(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let direction = arg(args, 0, TAB_HISTORY_USAGE)?;
    let history = browser.call("Page.getNavigationHistory", json!({}))?;
    let entry_id = history_entry_id(&history, direction)?;

    browser.call("Page.navigateToHistoryEntry", json!({"entryId":entry_id}))?;
    Ok(format!("Went {direction}."))
}

fn parse_wait_request(args: &[String]) -> Result<WaitRequest<'_>, Error> {
    let condition = arg(args, 0, WAIT_USAGE)?;
    let value = arg(args, 1, WAIT_USAGE)?;
    let seconds = args
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);

    Ok(WaitRequest {
        condition,
        value,
        seconds,
    })
}

fn wait_expression(condition: &str, value: &str) -> Result<String, Error> {
    match condition {
        "text" => Ok(format!(
            "document.body?.innerText.includes({})===true",
            js(value)
        )),
        "css" => Ok(format!("!!document.querySelector({})", js(value))),
        "visible" => Ok(format!(
            r#"(() => {{
                const element = document.querySelector({});
                if (!element) return false;
                const visible = {LAYOUT_VISIBLE_FN};
                return visible(element);
            }})()"#,
            js(value)
        )),
        "url" => Ok(format!("location.href.includes({})", js(value))),
        "gone" => Ok(format!("!document.querySelector({})", js(value))),
        "js" => Ok(format!("!!({value})")),
        _ => Err(jelly_error(
            ErrorKind::InvalidArguments,
            "condition must be text|css|visible|url|gone|js",
            false,
        )),
    }
}

fn install_injections_for_future_documents(browser: &mut BrowserSession) -> Result<(), Error> {
    for source in injections() {
        browser.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":source}),
        )?;
    }
    Ok(())
}

fn navigate_to(browser: &mut BrowserSession, url: &str) -> Result<(), Error> {
    let response = browser.call("Page.navigate", json!({"url":url}))?;
    if let Some(message) = response["result"]["errorText"].as_str() {
        return Err(jelly_error(ErrorKind::NavigationFailed, message, true));
    }
    Ok(())
}

fn wait_for_document_ready(browser: &mut BrowserSession, url: &str) -> Result<(), Error> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match browser.eval("document.readyState") {
            Ok(state)
                if state
                    .as_str()
                    .is_some_and(|value| value == "interactive" || value == "complete") =>
            {
                return Ok(());
            }
            _ => {}
        }

        if Instant::now() >= deadline {
            return Err(jelly_error(
                ErrorKind::NavigationTimeout,
                format!("document did not become interactive within 10s after navigating to {url}"),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn apply_injections_to_current_document(browser: &mut BrowserSession) {
    for source in injections() {
        let _ = browser.eval(&format!("(()=>{{\n{source}\n}})()"));
    }
}

fn navigation_result(browser: &mut BrowserSession, requested_url: &str) -> Result<String, Error> {
    let observed = browser.eval(
        "({requested:null,url:location.href,title:document.title||'',ready_state:document.readyState})",
    )?;
    let mut observed = observed.as_object().cloned().unwrap_or_default();
    observed.insert("requested".into(), json!(requested_url));
    Ok(serde_json::to_string_pretty(&Value::Object(observed))?)
}

fn history_entry_id(history: &Value, direction: &str) -> Result<i64, Error> {
    let current = history["result"]["currentIndex"].as_i64().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Page.getNavigationHistory response is missing currentIndex",
            false,
        )
    })?;
    let entries = history["result"]["entries"].as_array().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Page.getNavigationHistory response is missing entries",
            false,
        )
    })?;
    let wanted = match direction {
        "back" => current - 1,
        "forward" => current + 1,
        _ => {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                TAB_HISTORY_USAGE,
                false,
            ));
        }
    };
    if wanted < 0 || wanted >= entries.len() as i64 {
        return Err(jelly_error(
            ErrorKind::NavigationFailed,
            format!("cannot go {direction}: no navigation history entry"),
            false,
        ));
    }
    entries[wanted as usize]["id"].as_i64().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Page.getNavigationHistory returned an entry without a numeric id",
            false,
        )
    })
}

pub(crate) fn injections() -> Vec<String> {
    fs::read_dir(INJECTION_DIR)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            path.file_name()
                .and_then(|name| name.to_str())
                .filter(|name| name.starts_with("injected-") && name.ends_with(".js"))?;
            fs::read_to_string(path).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn wait_request_preserves_default_timeout_and_condition_validation() {
        let args = vec!["text".into(), "ready".into()];
        let request = parse_wait_request(&args).unwrap();
        assert_eq!(request.condition, "text");
        assert_eq!(request.value, "ready");
        assert_eq!(request.seconds, 10);

        let args = vec!["text".into(), "ready".into(), "3".into()];
        assert_eq!(parse_wait_request(&args).unwrap().seconds, 3);

        let error = wait_expression("unknown", "value").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
    }

    #[test]
    fn history_direction_and_bounds_have_typed_errors() {
        let history = json!({
            "result":{
                "currentIndex":0,
                "entries":[{"id":10},{"id":11}]
            }
        });

        let error = history_entry_id(&history, "sideways").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );

        let error = history_entry_id(&history, "back").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::NavigationFailed, false)
        );

        assert_eq!(history_entry_id(&history, "forward").unwrap(), 11);
    }

    #[test]
    fn malformed_history_response_is_an_explicit_internal_error() {
        for history in [
            json!({"result":{"entries":[]}}),
            json!({"result":{"currentIndex":0}}),
            json!({"result":{"currentIndex":0,"entries":[{"id":10},{}]}}),
        ] {
            let error = history_entry_id(&history, "forward").unwrap_err();
            assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
        }
    }
}
