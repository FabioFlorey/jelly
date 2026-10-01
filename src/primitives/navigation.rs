use crate::primitives::js;
use crate::{BrowserSession, Error, ErrorKind, INJECTION_DIR, jelly_error};
use serde_json::json;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

pub fn wait(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: wait <text|css|visible|url|gone|js> <value> [seconds]",
            false,
        ));
    }
    let secs = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(10);
    let start = Instant::now();
    loop {
        let e = match args[0].as_str() {
            "text" => format!("document.body?.innerText.includes({})===true", js(&args[1])),
            "css" => format!("!!document.querySelector({})", js(&args[1])),
            "visible" => format!(
                "(()=>{{const e=document.querySelector({});if(!e)return false;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'}})()",
                js(&args[1])
            ),
            "url" => format!("location.href.includes({})", js(&args[1])),
            "gone" => format!("!document.querySelector({})", js(&args[1])),
            "js" => format!("!!({})", args[1]),
            _ => {
                return Err(jelly_error(
                    ErrorKind::InvalidArguments,
                    "condition must be text|css|visible|url|gone|js",
                    false,
                ));
            }
        };
        match b.eval(&e) {
            Ok(v) if v.as_bool() == Some(true) => return Ok("Condition met.".into()),
            Ok(_) | Err(_) => {}
        }
        if start.elapsed() > Duration::from_secs(secs) {
            return Err(jelly_error(
                ErrorKind::ConditionTimeout,
                format!("wait timed out after {secs}s for {} {:?}", args[0], args[1]),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(250))
    }
}

pub fn navigate(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let url = args
        .first()
        .ok_or_else(|| jelly_error(ErrorKind::InvalidArguments, "usage: navigate <url>", false))?;
    for source in injections() {
        b.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":source}),
        )?;
    }
    let v = b.call("Page.navigate", json!({"url":url}))?;
    if let Some(e) = v["result"]["errorText"].as_str() {
        return Err(jelly_error(ErrorKind::NavigationFailed, e, true));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match b.eval("document.readyState") {
            Ok(s)
                if s.as_str()
                    .is_some_and(|x| x == "interactive" || x == "complete") =>
            {
                break;
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
        thread::sleep(Duration::from_millis(50))
    }
    for source in injections() {
        let _ = b.eval(&format!("(()=>{{\n{source}\n}})()"));
    }
    let observed = b.eval("({requested:null,url:location.href,title:document.title||'',ready_state:document.readyState})")?;
    let mut observed = observed.as_object().cloned().unwrap_or_default();
    observed.insert("requested".into(), json!(url));
    Ok(serde_json::to_string_pretty(&serde_json::Value::Object(
        observed,
    ))?)
}

pub fn tab_history(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let direction = args.first().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            "usage: tab-history <back|forward>",
            false,
        )
    })?;
    let history = b.call("Page.getNavigationHistory", json!({}))?;
    let entry_id = history_entry_id(&history, direction)?;
    b.call("Page.navigateToHistoryEntry", json!({"entryId":entry_id}))?;
    Ok(format!("Went {direction}."))
}

fn history_entry_id(history: &serde_json::Value, direction: &str) -> Result<i64, Error> {
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
                "usage: tab-history <back|forward>",
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
        .filter_map(|e| {
            let p = e.path();
            p.file_name()
                .and_then(|x| x.to_str())
                .filter(|x| x.starts_with("injected-") && x.ends_with(".js"))?;
            fs::read_to_string(p).ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;
    use serde_json::json;

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
