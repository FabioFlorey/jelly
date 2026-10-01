use crate::primitives::pretty;
use crate::{BrowserSession, Error, ErrorKind, INJECTION_DIR, jelly_error};
use std::fs;

pub fn evaluate_js(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.is_empty() {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: evaluate-js <expression>",
            false,
        ));
    }
    Ok(pretty(&b.eval(&args.join(" "))?))
}
pub fn inject_js(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let mut a = args.to_vec();
    let persistent = if let Some(i) = a.iter().position(|x| x == "--persistent") {
        a.remove(i);
        true
    } else {
        false
    };
    let source = if let Some(i) = a.iter().position(|x| x == "--file") {
        let path = a.get(i + 1).ok_or_else(|| {
            jelly_error(ErrorKind::InvalidArguments, "--file requires path", false)
        })?;
        read_injection_file(path)?
    } else {
        a.join(" ")
    };
    if source.trim().is_empty() {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "usage: inject-js [--persistent] [--file path | <script>]",
            false,
        ));
    }
    let id = if persistent {
        fs::create_dir_all(INJECTION_DIR).map_err(|error| {
            jelly_error(
                ErrorKind::Internal,
                format!("cannot create injection directory {INJECTION_DIR}: {error}"),
                false,
            )
        })?;
        let id = format!("jelly-{}", std::process::id());
        let path = format!("{INJECTION_DIR}/injected-{id}.js");
        fs::write(&path, &source).map_err(|error| {
            jelly_error(
                ErrorKind::Internal,
                format!("cannot persist injection script {path}: {error}"),
                false,
            )
        })?;
        Some(id)
    } else {
        None
    };
    b.eval(&format!("(()=>{{\n{source}\n}})()"))?;
    Ok(pretty(
        &serde_json::json!({"injected":true,"persistent":persistent,"identifier":id}),
    ))
}

fn read_injection_file(path: &str) -> Result<String, Error> {
    fs::read_to_string(path).map_err(|error| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("cannot read injection file {path}: {error}"),
            false,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn unreadable_injection_file_is_invalid_arguments() {
        let error = read_injection_file("/definitely/not/a/jelly/injection.js").unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::InvalidArguments, false)
        );
    }
}
