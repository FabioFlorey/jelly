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
    let d = args.first().ok_or("usage: tab-history <back|forward>")?;
    let h = b.call("Page.getNavigationHistory", json!({}))?;
    let cur = h["result"]["currentIndex"]
        .as_i64()
        .ok_or("no history index")?;
    let es = h["result"]["entries"]
        .as_array()
        .ok_or("no history entries")?;
    let wanted = match d.as_str() {
        "back" => cur - 1,
        "forward" => cur + 1,
        _ => return Err("usage: tab-history <back|forward>".into()),
    };
    if wanted < 0 || wanted >= es.len() as i64 {
        return Err(format!("cannot go {d}").into());
    }
    let id = es[wanted as usize]["id"]
        .as_i64()
        .ok_or("bad history entry")?;
    b.call("Page.navigateToHistoryEntry", json!({"entryId":id}))?;
    Ok(format!("Went {d}."))
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
