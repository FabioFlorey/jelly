use crate::primitives::{arg, js, missing_target, target};
use crate::{BrowserSession, Error, ErrorKind, Target, jelly_error};
use serde_json::json;
use std::fs;

const UPLOAD_USAGE: &str = "usage: upload <target> <file>";
const UPLOAD_MARKER_SELECTOR: &str = "[data-jelly-upload='1']";

struct UploadRequest {
    target: Target,
    file: String,
}

pub fn upload(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let request = parse_upload_request(args)?;

    mark_upload_target(browser, &request.target)?;
    let result = set_file_input(browser, &request.target, &request.file);
    clear_upload_marker(browser);
    result?;

    Ok(format!("Uploaded {}", js(&request.file)))
}

fn parse_upload_request(args: &[String]) -> Result<UploadRequest, Error> {
    let target = target(args, 0, UPLOAD_USAGE)?;
    let file = canonical_upload_path(arg(args, 1, UPLOAD_USAGE)?)?;
    Ok(UploadRequest { target, file })
}

fn mark_upload_target(browser: &mut BrowserSession, target: &Target) -> Result<(), Error> {
    let state = browser.eval(&format!(
        "(()=>{{document.querySelectorAll('[data-jelly-upload]').forEach(e=>e.removeAttribute('data-jelly-upload'));const e={};if(!e)return {{ok:false,error:'target not found'}};if(e.tagName!=='INPUT'||e.type!=='file')return {{ok:false,error:'target is not a file input'}};if(e.disabled)return {{ok:false,error:'target is disabled'}};e.setAttribute('data-jelly-upload','1');return {{ok:true}}}})()",
        target.js_resolver()
    ))?;
    if state["ok"] == true {
        return Ok(());
    }

    let message = state["error"].as_str().unwrap_or("upload failed");
    if message == "target not found" {
        Err(missing_target(target))
    } else {
        Err(jelly_error(ErrorKind::InteractionFailed, message, false))
    }
}

fn set_file_input(browser: &mut BrowserSession, target: &Target, file: &str) -> Result<(), Error> {
    let document = browser.call("DOM.getDocument", json!({}))?;
    let root_node_id = document_root_node_id(&document)?;

    let query = browser.call(
        "DOM.querySelector",
        json!({"nodeId":root_node_id,"selector":UPLOAD_MARKER_SELECTOR}),
    )?;
    let node_id = upload_node_id(&query).ok_or_else(|| missing_target(target))?;

    browser.call(
        "DOM.setFileInputFiles",
        json!({"nodeId":node_id,"files":[file]}),
    )?;
    Ok(())
}

fn clear_upload_marker(browser: &mut BrowserSession) {
    let _ = browser.eval(
        "document.querySelectorAll('[data-jelly-upload]').forEach(e=>e.removeAttribute('data-jelly-upload'))",
    );
}

fn document_root_node_id(document: &serde_json::Value) -> Result<i64, Error> {
    document["result"]["root"]["nodeId"]
        .as_i64()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "DOM.getDocument response is missing root.nodeId",
                false,
            )
        })
}

fn upload_node_id(query: &serde_json::Value) -> Option<i64> {
    query["result"]["nodeId"].as_i64().filter(|id| *id != 0)
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

    #[test]
    fn dom_upload_helpers_validate_result_shapes() {
        let error = document_root_node_id(&json!({"result":{}})).unwrap_err();
        assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
        assert_eq!(upload_node_id(&json!({"result":{"nodeId":0}})), None);
        assert_eq!(upload_node_id(&json!({"result":{"nodeId":42}})), Some(42));
    }
}
