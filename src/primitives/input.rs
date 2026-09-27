use crate::primitives::{js, target};
use crate::{BrowserSession, Error};
use serde_json::json;
use std::{thread, time::Duration};

pub fn click(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, "usage: click <target>")?;
    let p=b.eval(&format!(r#"(()=>{{const e={};if(!e)return null;e.scrollIntoView({{block:'center',inline:'center'}});const r=e.getBoundingClientRect(),s=getComputedStyle(e);if(r.width<=0||r.height<=0||s.display==='none'||s.visibility==='hidden')return null;return {{x:r.left+r.width/2,y:r.top+r.height/2,tag:e.tagName.toLowerCase(),text:(e.innerText||e.value||e.getAttribute('aria-label')||'').replace(/\s+/g,' ').trim().slice(0,160)}}}})()"#,target.js_resolver()))?;
    if p.is_null() {
        return Err("element not found or not visible".into());
    }
    let x = p["x"].as_f64().ok_or("invalid x")?;
    let y = p["y"].as_f64().ok_or("invalid y")?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":x,"y":y}),
    )?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":x,"y":y,"button":"left","buttons":1,"clickCount":1}),
    )?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","buttons":0,"clickCount":1}),
    )?;
    Ok(format!(
        "Clicked {}: {}",
        p["tag"].as_str().unwrap_or("element"),
        p["text"].as_str().unwrap_or("")
    ))
}

pub fn type_text(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let text = args.first().ok_or("usage: type-text <text> [target]")?;
    let find = if args.len() > 1 {
        target(args, 1, "usage: type-text <text> [target]")?.js_resolver()
    } else {
        "[...document.querySelectorAll('input:not([type]),input[type=text],input[type=search],input[type=email],input[type=password],input[type=url],input[type=number],textarea,[role=textbox],[contenteditable=true]')].find(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'})".into()
    };
    let p=b.eval(&format!("(()=>{{const e={find};if(!e)return null;e.scrollIntoView({{block:'center'}});const r=e.getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}}}})()"))?;
    if p.is_null() {
        return Err("textbox not found".into());
    }
    let x = p["x"].as_f64().ok_or("textbox x missing")?;
    let y = p["y"].as_f64().ok_or("textbox y missing")?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
    )?;
    b.call("Input.insertText", json!({"text":text}))?;
    Ok(format!("Typed: {text}"))
}

pub fn fill(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let text = args.first().ok_or("usage: fill <text> [target]")?;
    let find = if args.len() > 1 {
        target(args, 1, "usage: fill <text> [target]")?.js_resolver()
    } else {
        "[...document.querySelectorAll('input:not([type]),input[type=text],input[type=search],input[type=email],input[type=password],input[type=url],input[type=number],textarea,[role=textbox],[contenteditable=true]')].find(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'})".into()
    };
    let value = js(text);
    let result = b.eval(&format!(r#"(()=>{{
        const e={find};
        if(!e)return {{ok:false,error:'textbox not found'}};
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
    }})()"#))?;
    if !result["ok"].as_bool().unwrap_or(false) {
        return Err(result["error"]
            .as_str()
            .unwrap_or("fill failed")
            .to_owned()
            .into());
    }
    Ok(format!("Filled: {text}"))
}

pub fn press_key(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let key = args.first().ok_or("usage: press-key <key>")?;
    if key == "Enter" {
        for (kind, text) in [("rawKeyDown", None), ("char", Some("\r")), ("keyUp", None)] {
            let mut p = json!({"type":kind,"key":"Enter","code":"Enter","windowsVirtualKeyCode":13,"nativeVirtualKeyCode":13});
            if let Some(t) = text {
                p["text"] = json!(t);
                p["unmodifiedText"] = json!(t)
            }
            b.call("Input.dispatchKeyEvent", p)?;
        }
    } else {
        for kind in ["keyDown", "keyUp"] {
            b.call(
                "Input.dispatchKeyEvent",
                json!({"type":kind,"key":key,"code":key}),
            )?;
        }
    }
    Ok(format!("Pressed: {key}"))
}

pub fn select(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err("usage: select <target> <value>".into());
    }
    let t = target(args, 0, "usage: select <target> <value>")?;
    let v=b.eval(&format!(r#"(()=>{{const e={};if(!e)return {{ok:false,error:'target not found'}};const v={};if(e.tagName!=='SELECT')return {{ok:false,error:'target is not select'}};const o=[...e.options].find(o=>o.value===v||o.text.trim()===v);if(!o)return {{ok:false,error:'option not found'}};e.value=o.value;e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return {{ok:true,value:o.value}}}})()"#,t.js_resolver(),js(&args[1])))?;
    if !v["ok"].as_bool().unwrap_or(false) {
        return Err(v["error"]
            .as_str()
            .unwrap_or("select failed")
            .to_owned()
            .into());
    }
    Ok("selected".into())
}

pub fn check(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: check <target>")?;
    let v=b.eval(&format!(r#"(()=>{{const e={};if(!e)return {{ok:false,error:'target not found'}};if(e.checked===undefined)return {{ok:false,error:'target not checkable'}};if(!e.checked)e.click();return {{ok:true,checked:!!e.checked}}}})()"#,t.js_resolver()))?;
    if !v["ok"].as_bool().unwrap_or(false) {
        return Err(v["error"]
            .as_str()
            .unwrap_or("check failed")
            .to_owned()
            .into());
    }
    Ok(v["checked"].to_string())
}

pub fn dialog(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let action = args
        .first()
        .ok_or("usage: dialog <accept|dismiss> [text]")?;
    if action != "accept" && action != "dismiss" {
        return Err("usage: dialog <accept|dismiss> [text]".into());
    }
    let mut p = json!({"accept":action=="accept"});
    if let Some(t) = args.get(1) {
        p["promptText"] = t.clone().into()
    }
    b.call("Page.handleJavaScriptDialog", p)?;
    Ok(format!("Dialog {action}"))
}

pub fn drag(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err("usage: drag <source> <target|x:N,y:N>".into());
    }
    let source = target(args, 0, "usage: drag <source> <target|x:N,y:N>")?;
    let dest = &args[1];
    let sp=b.eval(&format!(r#"(()=>{{const s={};if(!s)return null;s.scrollIntoView({{block:'center',inline:'center'}});const r=s.getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}}}})()"#,source.js_resolver()))?;
    if sp.is_null() {
        return Err("source not found".into());
    }
    let (sx, sy) = (
        sp["x"].as_f64().ok_or("invalid source x")?,
        sp["y"].as_f64().ok_or("invalid source y")?,
    );
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseMoved","x":sx,"y":sy}),
    )?;
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mousePressed","x":sx,"y":sy,"button":"left","buttons":1,"clickCount":1}),
    )?;
    for i in 1..=3 {
        b.call("Input.dispatchMouseEvent",json!({"type":"mouseMoved","x":sx+i as f64*3.0,"y":sy+i as f64*2.0,"button":"left","buttons":1}))?;
        thread::sleep(Duration::from_millis(20))
    }
    let (tx, ty) = if let Some(v) = parse_coords(dest) {
        v
    } else {
        let t = crate::Target::parse(dest)?;
        let tp=b.eval(&format!(r#"(()=>{{const t={};if(!t)return null;t.scrollIntoView({{block:'center',inline:'center'}});const r=t.getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}}}})()"#,t.js_resolver()))?;
        if tp.is_null() {
            let _ = b.call(
                "Input.dispatchMouseEvent",
                json!({"type":"mouseReleased","x":sx,"y":sy,"button":"left","buttons":0}),
            );
            return Err("target not found".into());
        }
        (
            tp["x"].as_f64().ok_or("invalid target x")?,
            tp["y"].as_f64().ok_or("invalid target y")?,
        )
    };
    for i in 1..=16 {
        let k = i as f64 / 16.0;
        b.call("Input.dispatchMouseEvent",json!({"type":"mouseMoved","x":sx+(tx-sx)*k,"y":sy+(ty-sy)*k,"button":"left","buttons":1}))?;
        thread::sleep(Duration::from_millis(20))
    }
    b.call(
        "Input.dispatchMouseEvent",
        json!({"type":"mouseReleased","x":tx,"y":ty,"button":"left","buttons":0,"clickCount":1}),
    )?;
    Ok(format!("Dragged {} -> {dest}", args[0]))
}
fn parse_coords(s: &str) -> Option<(f64, f64)> {
    let mut x = None;
    let mut y = None;
    for part in s.split(',') {
        let (k, v) = part.trim().split_once(':')?;
        match k.trim() {
            "x" => x = v.trim().parse().ok(),
            "y" => y = v.trim().parse().ok(),
            _ => return None,
        }
    }
    Some((x?, y?))
}
