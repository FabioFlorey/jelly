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
    let file = canonical_upload_path(&args[1])?;
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
        let root = doc["result"]["root"]["nodeId"].as_i64().ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "DOM.getDocument response is missing root.nodeId",
                false,
            )
        })?;
        let q = b.call(
            "DOM.querySelector",
            json!({"nodeId":root,"selector":"[data-jelly-upload='1']"}),
        )?;
        let node = q["result"]["nodeId"]
            .as_i64()
            .filter(|x| *x != 0)
            .ok_or_else(|| missing_target(&t))?;
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

fn canonical_upload_path(path: &str) -> Result<String, Error> {
    fs::canonicalize(path)
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|error| {
            jelly_error(
                ErrorKind::InvalidArguments,
                format!("upload file is not accessible: {path}: {error}"),
                false,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn inaccessible_upload_path_is_invalid_arguments() {
        let error = canonical_upload_path("/definitely/not/a/jelly/upload/file").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
    }
}
