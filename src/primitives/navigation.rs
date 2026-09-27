use crate::primitives::js;
use crate::{BrowserSession, Error, INJECTION_DIR};
use serde_json::json;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

pub fn wait(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err("usage: wait <text|css|visible|url|gone|js> <value> [seconds]".into());
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
            _ => return Err("condition must be text|css|visible|url|gone|js".into()),
        };
        match b.eval(&e) {
            Ok(v) if v.as_bool() == Some(true) => return Ok("Condition met.".into()),
            Ok(_) | Err(_) => {}
        }
        if start.elapsed() > Duration::from_secs(secs) {
            return Err("wait timed out".into());
        }
        thread::sleep(Duration::from_millis(250))
    }
}

pub fn navigate(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let url = args.first().ok_or("usage: navigate <url>")?;
    for source in injections() {
        b.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":source}),
        )?;
    }
    let v = b.call("Page.navigate", json!({"url":url}))?;
    if let Some(e) = v["result"]["errorText"].as_str() {
        return Err(e.to_owned().into());
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
            break;
        }
        thread::sleep(Duration::from_millis(50))
    }
    for source in injections() {
        let _ = b.eval(&format!("(()=>{{\n{source}\n}})()"));
    }
    Ok(format!("Navigating to {url}"))
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
