use crate::primitives::{js, target};
use crate::{BrowserSession, Error};
use serde_json::json;
use std::fs;

pub fn upload(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.len() < 2 {
        return Err("usage: upload <target> <file>".into());
    }
    let t = target(args, 0, "usage: upload <target> <file>")?;
    let file = fs::canonicalize(&args[1])?;
    let file = file.to_string_lossy().to_string();
    let ok=b.eval(&format!("(()=>{{document.querySelectorAll('[data-pbj-upload]').forEach(e=>e.removeAttribute('data-pbj-upload'));const e={};if(!e||e.tagName!=='INPUT'||e.type!=='file')return false;e.setAttribute('data-pbj-upload','1');return true}})()",t.js_resolver()))?;
    if ok.as_bool() != Some(true) {
        return Err("target not found or not a file input".into());
    }
    let result = (|| -> Result<(), Error> {
        let doc = b.call("DOM.getDocument", json!({}))?;
        let root = doc["result"]["root"]["nodeId"]
            .as_i64()
            .ok_or("document root missing")?;
        let q = b.call(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":"[data-pbj-upload='1']"}),
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
    let _=b.eval("document.querySelectorAll('[data-pbj-upload]').forEach(e=>e.removeAttribute('data-pbj-upload'))");
    result?;
    Ok(format!("Uploaded {}", js(&file)))
}
