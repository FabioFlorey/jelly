use super::perf;
use crate::{ACTIVE_TARGET, DOWNLOAD_DIR, ENDPOINT, Error, ErrorKind, PAGE_TARGET, jelly_error};
use serde_json::{Value, json};
use std::{
    env, fs, io,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, connect};

pub struct BrowserSession {
    ws: WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    session: String,
    target_id: String,
    id: i64,
}

impl BrowserSession {
    pub fn connect() -> Result<Self, Error> {
        let started = Instant::now();
        let ep = fs::read_to_string(ENDPOINT).map_err(|error| {
            browser_unavailable(format!("failed to read CDP endpoint: {error}"))
        })?;
        let (mut ws, _) = connect(ep.trim())
            .map_err(|error| browser_unavailable(format!("failed to connect to CDP: {error}")))?;
        let timeout_secs = env::var("JELLY_CDP_TIMEOUT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(30);
        if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
            let timeout = Some(Duration::from_secs(timeout_secs));
            stream.set_read_timeout(timeout).map_err(|error| {
                browser_unavailable(format!("failed to configure CDP read timeout: {error}"))
            })?;
            stream.set_write_timeout(timeout).map_err(|error| {
                browser_unavailable(format!("failed to configure CDP write timeout: {error}"))
            })?;
        }
        send(&mut ws, 1, "Target.getTargets", None, json!({}))?;
        let v = recv(&mut ws, 1)?;
        check_error(&v)?;
        let preferred = [
            fs::read_to_string(ACTIVE_TARGET).ok(),
            fs::read_to_string(PAGE_TARGET).ok(),
        ];
        let target_info = v["result"]["targetInfos"]
            .as_array()
            .and_then(|targets| {
                preferred
                    .iter()
                    .flatten()
                    .find_map(|id| {
                        targets.iter().find(|target| {
                            target["type"] == "page"
                                && target["targetId"].as_str() == Some(id.trim())
                        })
                    })
                    .or_else(|| {
                        targets.iter().find(|target| {
                            target["type"] == "page"
                                && target["url"]
                                    .as_str()
                                    .is_some_and(|url| url.starts_with("http"))
                        })
                    })
            })
            .ok_or_else(|| browser_unavailable("no web page target"))?;
        let tid = target_info["targetId"]
            .as_str()
            .ok_or("page target has no targetId")?
            .to_owned();
        let mut download_params = json!({
            "behavior":"allowAndName",
            "downloadPath":DOWNLOAD_DIR,
            "eventsEnabled":true
        });
        if let Some(context_id) = target_info["browserContextId"].as_str() {
            download_params["browserContextId"] = json!(context_id);
        }
        send(
            &mut ws,
            2,
            "Browser.setDownloadBehavior",
            None,
            download_params,
        )?;
        check_error(&recv(&mut ws, 2)?)?;
        send(
            &mut ws,
            3,
            "Target.activateTarget",
            None,
            json!({"targetId":tid}),
        )?;
        check_error(&recv(&mut ws, 3)?)?;
        send(
            &mut ws,
            4,
            "Target.attachToTarget",
            None,
            json!({"targetId":tid,"flatten":true}),
        )?;
        let v = recv(&mut ws, 4)?;
        check_error(&v)?;
        fs::write(ACTIVE_TARGET, &tid)?;
        let session = Self {
            ws,
            session: v["result"]["sessionId"]
                .as_str()
                .ok_or("attach failed")?
                .into(),
            target_id: tid,
            id: 4,
        };
        perf::record("browser.connect", started.elapsed().as_millis(), None);
        Ok(session)
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
    }

    pub fn sync_active_target(&mut self) -> Result<(), Error> {
        let Ok(active_target) = fs::read_to_string(ACTIVE_TARGET) else {
            return Ok(());
        };
        let active_target = active_target.trim();
        if active_target.is_empty() || active_target == self.target_id {
            return Ok(());
        }
        self.switch_target(active_target)
    }

    pub fn switch_target(&mut self, target_id: &str) -> Result<(), Error> {
        self.id += 1;
        let id = self.id;
        send(
            &mut self.ws,
            id,
            "Target.activateTarget",
            None,
            json!({"targetId":target_id}),
        )?;
        check_error(&recv(&mut self.ws, id)?)?;
        self.id += 1;
        let id = self.id;
        send(
            &mut self.ws,
            id,
            "Target.attachToTarget",
            None,
            json!({"targetId":target_id,"flatten":true}),
        )?;
        let v = recv(&mut self.ws, id)?;
        check_error(&v)?;
        self.session = v["result"]["sessionId"]
            .as_str()
            .ok_or("attach failed")?
            .to_owned();
        self.target_id = target_id.to_owned();
        fs::write(ACTIVE_TARGET, target_id)?;
        Ok(())
    }

    pub fn browser_call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let started = Instant::now();
        self.id += 1;
        let id = self.id;
        send(&mut self.ws, id, method, None, params)?;
        let v = recv(&mut self.ws, id)?;
        check_error(&v)?;
        perf::record(
            "cdp.browser_call",
            started.elapsed().as_millis(),
            Some(method),
        );
        Ok(v)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        let started = Instant::now();
        self.id += 1;
        let id = self.id;
        send(&mut self.ws, id, method, Some(&self.session), params)?;
        let v = recv(&mut self.ws, id)?;
        check_error(&v)?;
        perf::record("cdp.call", started.elapsed().as_millis(), Some(method));
        Ok(v)
    }

    pub fn eval(&mut self, expr: &str) -> Result<Value, Error> {
        let v = self.call(
            "Runtime.evaluate",
            json!({"expression":expr,"returnByValue":true,"awaitPromise":true}),
        )?;
        if let Some(ex) = v["result"]["exceptionDetails"].as_object() {
            let msg = ex
                .get("exception")
                .and_then(|e| e["description"].as_str())
                .or_else(|| ex.get("text").and_then(Value::as_str))
                .unwrap_or("JavaScript evaluation failed");
            return Err(jelly_error(ErrorKind::JavascriptFailed, msg, false));
        }
        Ok(v["result"]["result"]
            .get("value")
            .cloned()
            .unwrap_or(Value::Null))
    }
}

fn send<S: io::Read + io::Write>(
    ws: &mut WebSocket<S>,
    id: i64,
    method: &str,
    session: Option<&str>,
    params: Value,
) -> Result<(), Error> {
    let mut v = json!({"id":id,"method":method,"params":params});
    if let Some(s) = session {
        v["sessionId"] = s.into()
    }
    ws.send(Message::Text(v.to_string().into()))
        .map_err(|error| browser_unavailable(format!("failed to send CDP message: {error}")))?;
    Ok(())
}
fn recv<S: io::Read + io::Write>(ws: &mut WebSocket<S>, id: i64) -> Result<Value, Error> {
    loop {
        let message = match ws.read() {
            Ok(message) => message,
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                return Err(jelly_error(
                    ErrorKind::BrowserUnavailable,
                    "timed out waiting for a CDP response",
                    true,
                ));
            }
            Err(error) => {
                return Err(browser_unavailable(format!(
                    "failed while reading a CDP response: {error}"
                )));
            }
        };
        if let Message::Text(t) = message {
            let v: Value = serde_json::from_str(&t)?;
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(v);
            }
        }
    }
}
fn browser_unavailable(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::BrowserUnavailable, message, true)
}

fn invalid_session_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "session closed",
        "no session with given id",
        "session with given id not found",
        "target closed",
        "no target with given id",
        "inspected target navigated or closed",
        "not attached to an active page",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn check_error(v: &Value) -> Result<(), Error> {
    if let Some(e) = v.get("error") {
        let code = &e["code"];
        let message = e["message"].as_str().unwrap_or("unknown error");
        let rendered = format!("CDP {code}: {message}");
        if invalid_session_message(message) {
            return Err(browser_unavailable(rendered));
        }
        return Err(jelly_error(ErrorKind::Internal, rendered, false));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn closed_session_cdp_errors_are_retryable_browser_failures() {
        let error = check_error(&json!({
            "error": {"code": -32001, "message": "Session with given id not found."}
        }))
        .unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::BrowserUnavailable, true)
        );
    }

    #[test]
    fn ordinary_cdp_errors_remain_internal_and_non_retryable() {
        let error = check_error(&json!({
            "error": {"code": -32601, "message": "Method not found"}
        }))
        .unwrap_err();
        assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
    }
}
