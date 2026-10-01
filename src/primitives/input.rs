use crate::primitives::{arg, js, missing_target, pretty, target};
use crate::{BrowserSession, Error, ErrorKind, Target, jelly_error};
use serde_json::{Value, json};
use std::{thread, time::Duration};

const CLICK_USAGE: &str = "usage: click <target>";
const TYPE_TEXT_USAGE: &str = "usage: type-text <text> [target]";
const FILL_USAGE: &str = "usage: fill <text> [target]";
const PRESS_KEY_USAGE: &str = "usage: press-key <key>";
const SELECT_USAGE: &str = "usage: select <target> <value>";
const CHECK_USAGE: &str = "usage: check <target>";
const DIALOG_USAGE: &str = "usage: dialog <accept|dismiss> [text]";
const DRAG_USAGE: &str = "usage: drag <source> <target|x:N,y:N>";

#[derive(Debug)]
struct TextInputRequest<'a> {
    text: &'a str,
    target: Option<Target>,
}

#[derive(Debug)]
struct SelectRequest<'a> {
    target: Target,
    value: &'a str,
}

#[derive(Debug)]
struct DialogRequest<'a> {
    action: &'a str,
    text: Option<&'a str>,
}

#[derive(Debug)]
struct DragRequest<'a> {
    source: Target,
    source_text: &'a str,
    destination: &'a str,
}

struct ClickPlan {
    x: f64,
    y: f64,
    tag: Value,
    text: Value,
}

pub fn click(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, CLICK_USAGE)?;
    let plan = prepare_click(browser, &target)?;

    dispatch_left_click(browser, plan.x, plan.y)?;

    Ok(pretty(&json!({
        "action":"click",
        "performed":true,
        "target":{"tag":plan.tag,"text":plan.text},
        "point":{"x":plan.x,"y":plan.y}
    })))
}

pub fn type_text(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_text_input(args, TYPE_TEXT_USAGE)?;
    let resolver = textbox_resolver(request.target.as_ref());
    let point = browser.eval(&format!(
        "(()=>{{const e={resolver};if(!e)return {{ok:false,error:'textbox not found'}};if(e.disabled)return {{ok:false,error:'target is disabled'}};if(e.readOnly)return {{ok:false,error:'target is readonly'}};e.scrollIntoView({{block:'center'}});const r=e.getBoundingClientRect();return {{ok:true,x:r.left+r.width/2,y:r.top+r.height/2}}}})()"
    ))?;

    let (x, y) = textbox_point(&point, request.target.as_ref())?;
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    browser.call("Input.insertText", json!({"text":request.text}))?;

    Ok(format!("Typed: {}", request.text))
}

pub fn fill(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_text_input(args, FILL_USAGE)?;
    let resolver = textbox_resolver(request.target.as_ref());
    let value = js(request.text);
    let result = browser.eval(&format!(
        r#"(()=>{{
        const e={resolver};
        if(!e)return {{ok:false,error:'textbox not found'}};
        if(e.disabled)return {{ok:false,error:'target is disabled'}};
        if(e.readOnly)return {{ok:false,error:'target is readonly'}};
        e.scrollIntoView({{block:'center'}});
        e.focus();
        const tag=e.tagName;
        if(tag==='INPUT'||tag==='TEXTAREA'){{
            const proto=tag==='INPUT'?HTMLInputElement.prototype:HTMLTextAreaElement.prototype;
            const setter=Object.getOwnPropertyDescriptor(proto,'value')?.set;
            if(setter)setter.call(e,{value});else e.value={value};
            e.dispatchEvent(new InputEvent('input',{{bubbles:true,inputType:'insertText',data:{value}}}));
            e.dispatchEvent(new Event('change',{{bubbles:true}}));
            return {{ok:true,value:e.value}};
        }}
        if(e.isContentEditable||e.getAttribute('role')==='textbox'){{
            e.textContent={value};
            e.dispatchEvent(new InputEvent('input',{{bubbles:true,inputType:'insertText',data:{value}}}));
            e.dispatchEvent(new Event('change',{{bubbles:true}}));
            return {{ok:true,value:e.textContent}};
        }}
        return {{ok:false,error:'target is not editable'}};
    }})()"#
    ))?;

    validate_textbox_result(&result, request.target.as_ref(), "fill failed")?;
    Ok(format!("Filled: {}", request.text))
}

pub fn press_key(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let key = arg(args, 0, PRESS_KEY_USAGE)?;
    dispatch_key(browser, key)?;
    Ok(format!("Pressed: {key}"))
}

pub fn select(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_select_request(args)?;
    let result = browser.eval(&format!(
        r#"(()=>{{const e={};if(!e)return {{ok:false,error:'target not found'}};if(e.disabled)return {{ok:false,error:'target is disabled'}};const v={};if(e.tagName!=='SELECT')return {{ok:false,error:'target is not select'}};const o=[...e.options].find(o=>o.value===v||o.text.trim()===v);if(!o)return {{ok:false,error:'option not found'}};e.value=o.value;e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return {{ok:true,value:o.value}}}})()"#,
        request.target.js_resolver(),
        js(request.value)
    ))?;

    validate_target_interaction(&result, &request.target, "select failed")?;
    Ok("selected".into())
}

pub fn check(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, CHECK_USAGE)?;
    let result = browser.eval(&format!(
        r#"(()=>{{const e={};if(!e)return {{ok:false,error:'target not found'}};if(e.disabled)return {{ok:false,error:'target is disabled'}};if(e.checked===undefined)return {{ok:false,error:'target not checkable'}};if(!e.checked)e.click();return {{ok:true,checked:!!e.checked}}}})()"#,
        target.js_resolver()
    ))?;

    validate_target_interaction(&result, &target, "check failed")?;
    Ok(result["checked"].to_string())
}

pub fn dialog(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_dialog_request(args)?;
    let mut params = json!({"accept":request.action=="accept"});
    if let Some(text) = request.text {
        params["promptText"] = text.into();
    }

    browser.call("Page.handleJavaScriptDialog", params)?;
    Ok(format!("Dialog {}", request.action))
}

pub fn drag(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_drag_request(args)?;
    let (source_x, source_y) = resolve_drag_target(browser, &request.source, "source")?;

    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":source_x,"y":source_y}),
    )?;
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":source_x,"y":source_y,"button":"left","buttons":1,"clickCount":1}),
    )?;
    for i in 1..=3 {
        browser.call(
            "Input.dispatchMouseEvent",
            json!({
                "type":"mouseMoved",
                "x":source_x+i as f64*3.0,
                "y":source_y+i as f64*2.0,
                "button":"left",
                "buttons":1
            }),
        )?;
        thread::sleep(Duration::from_millis(20));
    }

    let destination = resolve_drag_destination(browser, request.destination, source_x, source_y)?;
    dispatch_drag_path(browser, source_x, source_y, destination.0, destination.1)?;

    Ok(format!(
        "Dragged {} -> {}",
        request.source_text, request.destination
    ))
}

fn parse_text_input<'a>(args: &'a [String], usage: &str) -> Result<TextInputRequest<'a>, Error> {
    let text = arg(args, 0, usage)?;
    let target = if args.len() > 1 {
        Some(target(args, 1, usage)?)
    } else {
        None
    };
    Ok(TextInputRequest { text, target })
}

fn parse_select_request(args: &[String]) -> Result<SelectRequest<'_>, Error> {
    Ok(SelectRequest {
        target: target(args, 0, SELECT_USAGE)?,
        value: arg(args, 1, SELECT_USAGE)?,
    })
}

fn parse_dialog_request(args: &[String]) -> Result<DialogRequest<'_>, Error> {
    let action = arg(args, 0, DIALOG_USAGE)?;
    if !matches!(action, "accept" | "dismiss") {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            DIALOG_USAGE,
            false,
        ));
    }
    Ok(DialogRequest {
        action,
        text: args.get(1).map(String::as_str),
    })
}

fn parse_drag_request(args: &[String]) -> Result<DragRequest<'_>, Error> {
    let source_text = arg(args, 0, DRAG_USAGE)?;
    Ok(DragRequest {
        source: Target::parse(source_text)?,
        source_text,
        destination: arg(args, 1, DRAG_USAGE)?,
    })
}

fn prepare_click(browser: &mut BrowserSession, target: &Target) -> Result<ClickPlan, Error> {
    let result = browser.eval(&format!(
        r#"(()=>{{const e={};if(!e)return {{ok:false,reason:'missing'}};if(e.matches?.(':disabled')||e.getAttribute('aria-disabled')==='true')return {{ok:false,reason:'disabled'}};e.scrollIntoView({{block:'center',inline:'center'}});const r=e.getBoundingClientRect(),s=getComputedStyle(e);if(r.width<=0||r.height<=0||s.display==='none'||s.visibility==='hidden'||s.opacity==='0')return {{ok:false,reason:'hidden'}};return {{ok:true,x:r.left+r.width/2,y:r.top+r.height/2,tag:e.tagName.toLowerCase(),text:(e.innerText||e.value||e.getAttribute('aria-label')||'').replace(/\s+/g,' ').trim().slice(0,160)}}}})()"#,
        target.js_resolver()
    ))?;

    if result["ok"] != true {
        return match result["reason"].as_str() {
            Some("missing") => Err(missing_target(target)),
            Some("disabled") => Err(jelly_error(
                ErrorKind::InteractionFailed,
                "target is disabled",
                false,
            )),
            _ => Err(jelly_error(
                ErrorKind::TargetNotVisible,
                "target exists but is not visible",
                true,
            )),
        };
    }

    let x = point_coordinate(&result, "x", "invalid click x coordinate")?;
    let y = point_coordinate(&result, "y", "invalid click y coordinate")?;
    Ok(ClickPlan {
        x,
        y,
        tag: result["tag"].clone(),
        text: result["text"].clone(),
    })
}

fn dispatch_left_click(browser: &mut BrowserSession, x: f64, y: f64) -> Result<(), Error> {
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":x,"y":y}),
    )?;
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":x,"y":y,"button":"left","buttons":1,"clickCount":1}),
    )?;
    browser.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","buttons":0,"clickCount":1}),
    )?;
    Ok(())
}

fn textbox_resolver(target: Option<&Target>) -> String {
    target.map(Target::js_resolver).unwrap_or_else(|| {
        "[...document.querySelectorAll('input:not([type]),input[type=text],input[type=search],input[type=email],input[type=password],input[type=url],input[type=number],textarea,[role=textbox],[contenteditable=true]')].find(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return !e.disabled&&!e.readOnly&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'})".into()
    })
}

fn textbox_point(result: &Value, target: Option<&Target>) -> Result<(f64, f64), Error> {
    validate_textbox_result(result, target, "textbox not found")?;
    Ok((
        point_coordinate(result, "x", "textbox x missing")?,
        point_coordinate(result, "y", "textbox y missing")?,
    ))
}

fn validate_textbox_result(
    result: &Value,
    target: Option<&Target>,
    fallback: &str,
) -> Result<(), Error> {
    if result["ok"].as_bool().unwrap_or(false) {
        return Ok(());
    }

    let message = result["error"].as_str().unwrap_or(fallback);
    if message == "textbox not found" {
        return Err(target
            .map(missing_target)
            .unwrap_or_else(|| jelly_error(ErrorKind::TargetNotFound, message, true)));
    }
    Err(jelly_error(ErrorKind::InteractionFailed, message, false))
}

fn validate_target_interaction(
    result: &Value,
    target: &Target,
    fallback: &str,
) -> Result<(), Error> {
    if result["ok"].as_bool().unwrap_or(false) {
        return Ok(());
    }

    let message = result["error"].as_str().unwrap_or(fallback);
    if message == "target not found" {
        Err(missing_target(target))
    } else {
        Err(jelly_error(ErrorKind::InteractionFailed, message, false))
    }
}

fn point_coordinate(result: &Value, key: &str, message: &str) -> Result<f64, Error> {
    result[key]
        .as_f64()
        .ok_or_else(|| jelly_error(ErrorKind::InteractionFailed, message, false))
}

fn dispatch_key(browser: &mut BrowserSession, key: &str) -> Result<(), Error> {
    if key == "Enter" {
        for (kind, text) in [("rawKeyDown", None), ("char", Some("\r")), ("keyUp", None)] {
            let mut params = json!({
                "type":kind,
                "key":"Enter",
                "code":"Enter",
                "windowsVirtualKeyCode":13,
                "nativeVirtualKeyCode":13
            });
            if let Some(text) = text {
                params["text"] = json!(text);
                params["unmodifiedText"] = json!(text);
            }
            browser.call("Input.dispatchKeyEvent", params)?;
        }
    } else {
        for kind in ["keyDown", "keyUp"] {
            browser.call(
                "Input.dispatchKeyEvent",
                json!({"type":kind,"key":key,"code":key}),
            )?;
        }
    }
    Ok(())
}

fn resolve_drag_target(
    browser: &mut BrowserSession,
    target: &Target,
    role: &str,
) -> Result<(f64, f64), Error> {
    let value = browser.eval(&format!(
        r#"(()=>{{const e={};if(!e)return null;e.scrollIntoView({{block:'center',inline:'center'}});const r=e.getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}}}})()"#,
        target.js_resolver()
    ))?;
    if value.is_null() {
        return Err(missing_target(target));
    }
    drag_point(&value, role)
}

fn resolve_drag_destination(
    browser: &mut BrowserSession,
    destination: &str,
    source_x: f64,
    source_y: f64,
) -> Result<(f64, f64), Error> {
    if let Some(point) = parse_coords(destination) {
        return Ok(point);
    }

    let target = Target::parse(destination)?;
    match resolve_drag_target(browser, &target, "target") {
        Ok(point) => Ok(point),
        Err(error) => {
            if matches!(
                crate::classify_error(error.as_ref()),
                (ErrorKind::TargetNotFound | ErrorKind::TargetStale, _)
            ) {
                let _ = browser.call(
                    "Input.dispatchMouseEvent",
                    json!({
                        "type":"mouseReleased",
                        "x":source_x,
                        "y":source_y,
                        "button":"left",
                        "buttons":0
                    }),
                );
            }
            Err(error)
        }
    }
}

fn dispatch_drag_path(
    browser: &mut BrowserSession,
    source_x: f64,
    source_y: f64,
    target_x: f64,
    target_y: f64,
) -> Result<(), Error> {
    for i in 1..=16 {
        let progress = i as f64 / 16.0;
        browser.call(
            "Input.dispatchMouseEvent",
            json!({
                "type":"mouseMoved",
                "x":source_x+(target_x-source_x)*progress,
                "y":source_y+(target_y-source_y)*progress,
                "button":"left",
                "buttons":1
            }),
        )?;
        thread::sleep(Duration::from_millis(20));
    }
    browser.call(
        "Input.dispatchMouseEvent",
        json!({
            "type":"mouseReleased",
            "x":target_x,
            "y":target_y,
            "button":"left",
            "buttons":0,
            "clickCount":1
        }),
    )?;
    Ok(())
}

fn drag_point(value: &Value, role: &str) -> Result<(f64, f64), Error> {
    let x = value["x"].as_f64().ok_or_else(|| {
        jelly_error(
            ErrorKind::InteractionFailed,
            format!("drag {role} x coordinate is missing or invalid"),
            false,
        )
    })?;
    let y = value["y"].as_f64().ok_or_else(|| {
        jelly_error(
            ErrorKind::InteractionFailed,
            format!("drag {role} y coordinate is missing or invalid"),
            false,
        )
    })?;
    Ok((x, y))
}

fn parse_coords(value: &str) -> Option<(f64, f64)> {
    let mut x = None;
    let mut y = None;
    for part in value.split(',') {
        let (key, value) = part.trim().split_once(':')?;
        match key.trim() {
            "x" => x = value.trim().parse().ok(),
            "y" => y = value.trim().parse().ok(),
            _ => return None,
        }
    }
    Some((x?, y?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn text_input_parsing_keeps_optional_target_semantics() {
        let args = vec!["hello".into()];
        let request = parse_text_input(&args, TYPE_TEXT_USAGE).unwrap();
        assert_eq!(request.text, "hello");
        assert!(request.target.is_none());

        let args = vec!["hello".into(), "css:#name".into()];
        let request = parse_text_input(&args, TYPE_TEXT_USAGE).unwrap();
        assert_eq!(request.target, Some(Target::Css("#name".into())));
    }

    #[test]
    fn dialog_parsing_rejects_unknown_actions() {
        let args = vec!["later".into()];
        let error = parse_dialog_request(&args).unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
    }

    #[test]
    fn malformed_drag_coordinates_are_typed_interaction_failures() {
        let error = drag_point(&json!({"x":"bad","y":2.0}), "source").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InteractionFailed, false)
        );

        let error = drag_point(&json!({"x":1.0}), "target").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InteractionFailed, false)
        );
    }
}
