use crate::primitives::{js, missing_target, target};
use crate::{BrowserSession, Error, ErrorKind, jelly_error};
use serde_json::json;
use std::fs;

pub fn upload(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: upload <target> <file>",
            false,
        ));
    }
    let t = target(args, 0, "usage: upload <target> <file>")?;
    let file = fs::canonicalize(&args[1])?;
    let file = file.to_string_lossy().to_string();
    let state=b.eval(&format!("(()=>{{document.querySelectorAll('[data-jelly-upload]').forEach(e=>e.removeAttribute('data-jelly-upload'));const e={};if(!e)return {{ok:false,error:'target not found'}};if(e.tagName!=='INPUT'||e.type!=='file')return {{ok:false,error:'target is not a file input'}};if(e.disabled)return {{ok:false,error:'target is disabled'}};e.setAttribute('data-jelly-upload','1');return {{ok:true}}}})()",t.js_resolver()))?;
    if state["ok"] != true {
        let message = state["error"].as_str().unwrap_or("upload failed");
        if message == "target not found" {
            return Err(missing_target(&t));
        }
        return Err(jelly_error(ErrorKind::InteractionFailed, message, false));
    }
    let result = (|| -> Result<(), Error> {
        let doc = b.call("DOM.getDocument", json!({}))?;
        let root = doc["result"]["root"]["nodeId"]
            .as_i64()
            .ok_or("document root missing")?;
        let q = b.call(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":"[data-jelly-upload='1']"}),
        )?;
        let node = q["result"]["nodeId"]
            .as_i64()
            .filter(|x| *x != 0)
            .ok_or("file input node not found")?;
        b.call(
            "DOM.setFileInputFiles",
            json!({"nodeId":node,"files":[file]}),
        )?;
        Ok(())
    })();
    let _=b.eval("document.querySelectorAll('[data-jelly-upload]').forEach(e=>e.removeAttribute('data-jelly-upload'))");
    result?;
    Ok(format!("Uploaded {}", js(&file)))
}
