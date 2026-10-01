use crate::primitives::js_helpers::{NORMALIZE_TEXT_FN, RENDERED_VISIBLE_FN};
use crate::primitives::{missing_target, pretty, target};
use crate::{BrowserSession, Error, ErrorKind, Target, jelly_error};
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};

fn target_state(b: &mut BrowserSession, target: &Target) -> Result<Value, Error> {
    b.eval(&format!(
        r#"(() => {{
            const element = {};
            if (!element) return {{exists:false}};

            const visible = {RENDERED_VISIBLE_FN};
            const normalizeText = {NORMALIZE_TEXT_FN};
            const rect = element.getBoundingClientRect();
            const image = element instanceof HTMLImageElement
                ? {{
                    complete:element.complete,
                    natural_width:element.naturalWidth,
                    natural_height:element.naturalHeight,
                    src:element.currentSrc || element.src || ''
                }}
                : null;

            return {{
                exists:true,
                visible:visible(element),
                tag:element.tagName.toLowerCase(),
                text:normalizeText(
                    element.innerText ||
                    element.textContent ||
                    element.getAttribute('aria-label') ||
                    element.getAttribute('alt') ||
                    ''
                ),
                rect:{{x:rect.x,y:rect.y,width:rect.width,height:rect.height}},
                image
            }};
        }})()"#,
        target.js_resolver()
    ))
}

pub fn assert_url(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let expected = args.first().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            "usage: assert-url <expected> [exact|contains]",
            false,
        )
    })?;
    let mode = args.get(1).map(String::as_str).unwrap_or("exact");
    let current = b.eval("location.href")?.as_str().unwrap_or("").to_owned();
    let matches = match mode {
        "exact" => current == *expected,
        "contains" => current.contains(expected),
        _ => {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                "mode must be exact|contains",
                false,
            ));
        }
    };
    if !matches {
        return Err(jelly_error(
            ErrorKind::ConditionFailed,
            format!("URL assertion failed: expected {mode} {expected:?}, got {current:?}"),
            true,
        ));
    }
    Ok(pretty(
        &json!({"expected":expected,"actual":current,"match":mode,"matched":true}),
    ))
}

pub fn assert_title(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let expected = args.first().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            "usage: assert-title <expected> [exact|contains]",
            false,
        )
    })?;
    let mode = args.get(1).map(String::as_str).unwrap_or("contains");
    let current = b
        .eval("document.title||''")?
        .as_str()
        .unwrap_or("")
        .to_owned();
    let matches = match mode {
        "exact" => current == *expected,
        "contains" => current.contains(expected),
        _ => {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                "mode must be exact|contains",
                false,
            ));
        }
    };
    if !matches {
        return Err(jelly_error(
            ErrorKind::ConditionFailed,
            format!("title assertion failed: expected {mode} {expected:?}, got {current:?}"),
            true,
        ));
    }
    Ok(pretty(
        &json!({"expected":expected,"actual":current,"match":mode,"matched":true}),
    ))
}

pub fn assert_visible(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: assert-visible <target>")?;
    let state = target_state(b, &t)?;
    if state["exists"] != true {
        return Err(missing_target(&t));
    }
    if state["visible"] != true {
        return Err(jelly_error(
            ErrorKind::TargetNotVisible,
            "target exists but is not visible",
            true,
        ));
    }
    Ok(pretty(&state))
}

pub fn assert_text(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: assert-text <target> <text> [exact|contains]",
            false,
        ));
    }
    let t = target(
        args,
        0,
        "usage: assert-text <target> <text> [exact|contains]",
    )?;
    let expected = &args[1];
    let mode = args.get(2).map(String::as_str).unwrap_or("contains");
    let state = target_state(b, &t)?;
    if state["exists"] != true {
        return Err(missing_target(&t));
    }
    let actual = state["text"].as_str().unwrap_or("");
    let matches = match mode {
        "exact" => actual == expected,
        "contains" => actual.contains(expected),
        _ => {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                "mode must be exact|contains",
                false,
            ));
        }
    };
    if !matches {
        return Err(jelly_error(
            ErrorKind::ConditionFailed,
            format!("text assertion failed: expected {mode} {expected:?}, got {actual:?}"),
            true,
        ));
    }
    Ok(pretty(
        &json!({"target":state,"expected":expected,"match":mode,"matched":true}),
    ))
}

pub fn assert_image_ready(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: assert-image-ready <target>")?;
    let state = target_state(b, &t)?;
    if state["exists"] != true {
        return Err(missing_target(&t));
    }
    if state["visible"] != true {
        return Err(jelly_error(
            ErrorKind::TargetNotVisible,
            "image exists but is not visible",
            true,
        ));
    }
    let image = state.get("image").filter(|v| !v.is_null()).ok_or_else(|| {
        jelly_error(
            ErrorKind::ConditionFailed,
            "target is not an image element",
            false,
        )
    })?;
    let ready = image["complete"] == true
        && image["natural_width"].as_u64().unwrap_or(0) > 0
        && image["natural_height"].as_u64().unwrap_or(0) > 0;
    if !ready {
        return Err(jelly_error(
            ErrorKind::ConditionFailed,
            "image exists but is not fully loaded with nonzero natural dimensions",
            true,
        ));
    }
    Ok(pretty(&state))
}

pub fn wait_for(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: wait-for <exists|visible|hidden|text|url|title|image-ready> <value> [seconds]",
            false,
        ));
    }
    let condition = args[0].as_str();
    let value = &args[1];
    let seconds = args
        .get(2)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10);
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds);
    loop {
        let (met, evidence) = match condition {
            "exists" | "visible" | "hidden" | "image-ready" => {
                let t = Target::parse(value)?;
                let state = target_state(b, &t)?;
                let met = match condition {
                    "exists" => state["exists"] == true,
                    "visible" => state["exists"] == true && state["visible"] == true,
                    "hidden" => state["exists"] != true || state["visible"] != true,
                    "image-ready" => {
                        let image = &state["image"];
                        state["exists"] == true
                            && state["visible"] == true
                            && !image.is_null()
                            && image["complete"] == true
                            && image["natural_width"].as_u64().unwrap_or(0) > 0
                            && image["natural_height"].as_u64().unwrap_or(0) > 0
                    }
                    _ => false,
                };
                (met, state)
            }
            "text" => {
                let actual = b
                    .eval("document.body?.innerText||''")?
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                (
                    actual.contains(value),
                    json!({"text_present":actual.contains(value)}),
                )
            }
            "url" => {
                let actual = b.eval("location.href")?.as_str().unwrap_or("").to_owned();
                (actual.contains(value), json!({"actual":actual}))
            }
            "title" => {
                let actual = b
                    .eval("document.title||''")?
                    .as_str()
                    .unwrap_or("")
                    .to_owned();
                (actual.contains(value), json!({"actual":actual}))
            }
            _ => {
                return Err(jelly_error(
                    ErrorKind::InvalidArguments,
                    "condition must be exists|visible|hidden|text|url|title|image-ready",
                    false,
                ));
            }
        };
        if met {
            return Ok(pretty(&json!({
                "condition":condition,
                "value":value,
                "elapsed_ms":started.elapsed().as_millis(),
                "evidence":evidence
            })));
        }
        if Instant::now() >= deadline {
            return Err(jelly_error(
                ErrorKind::ConditionTimeout,
                format!(
                    "timed out after {seconds}s waiting for {condition} {value:?}; last evidence: {}",
                    evidence
                ),
                true,
            ));
        }
        thread::sleep(Duration::from_millis(200));
    }
}
