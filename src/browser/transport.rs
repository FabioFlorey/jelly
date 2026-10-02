use crate::{Error, ErrorKind, cdp_error, jelly_error};
use serde_json::{Value, json};
use std::{fs, io, time::Duration};
use tungstenite::{Message, WebSocket, connect};

type CdpSocket = WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

pub(crate) struct CdpTransport {
    ws: CdpSocket,
    next_id: i64,
}

pub(crate) struct CdpExchange {
    pub(crate) response: Value,
    pub(crate) notifications: Vec<CdpNotification>,
}

pub(crate) struct CdpNotification {
    pub(crate) value: Value,
    pub(crate) wire_bytes: usize,
}

impl CdpTransport {
    pub(crate) fn connect(endpoint_path: &str) -> Result<Self, Error> {
        let endpoint = fs::read_to_string(endpoint_path).map_err(|error| {
            browser_unavailable(format!("failed to read CDP endpoint: {error}"))
        })?;
        let (mut ws, _) = connect(endpoint.trim())
            .map_err(|error| browser_unavailable(format!("failed to connect to CDP: {error}")))?;

        configure_socket_timeout(&mut ws)?;

        Ok(Self { ws, next_id: 0 })
    }

    pub(crate) fn request(
        &mut self,
        method: &str,
        session_id: Option<&str>,
        params: Value,
    ) -> Result<CdpExchange, Error> {
        self.next_id += 1;
        let id = self.next_id;
        send(&mut self.ws, id, method, session_id, params)?;

        let mut notifications = Vec::new();
        loop {
            let (text, wire_bytes) = read_text_message(&mut self.ws)?;
            match classify_cdp_text(&text, id)? {
                IncomingCdp::Response(response) => {
                    return Ok(CdpExchange {
                        response,
                        notifications,
                    });
                }
                IncomingCdp::Notification(value) => {
                    notifications.push(CdpNotification { value, wire_bytes });
                }
            }
        }
    }
}

fn configure_socket_timeout(ws: &mut CdpSocket) -> Result<(), Error> {
    let timeout_secs = crate::config::config().browser.cdp_timeout_secs.max(1);

    if let tungstenite::stream::MaybeTlsStream::Plain(stream) = ws.get_mut() {
        let timeout = Some(Duration::from_secs(timeout_secs));
        stream.set_read_timeout(timeout).map_err(|error| {
            browser_unavailable(format!("failed to configure CDP read timeout: {error}"))
        })?;
        stream.set_write_timeout(timeout).map_err(|error| {
            browser_unavailable(format!("failed to configure CDP write timeout: {error}"))
        })?;
    }
    Ok(())
}

enum IncomingCdp {
    Response(Value),
    Notification(Value),
}

fn classify_cdp_text(text: &str, expected_id: i64) -> Result<IncomingCdp, Error> {
    let value: Value = serde_json::from_str(text).map_err(|error| {
        jelly_error(
            ErrorKind::Internal,
            format!("failed to parse CDP JSON message: {error}"),
            false,
        )
    })?;

    if let Some(id) = value.get("id").and_then(Value::as_i64) {
        if id == expected_id {
            return Ok(IncomingCdp::Response(value));
        }
        return Err(jelly_error(
            ErrorKind::Internal,
            format!(
                "received unexpected CDP response id {id} while waiting for response id {expected_id}"
            ),
            false,
        ));
    }

    if value.get("method").and_then(Value::as_str).is_some() {
        return Ok(IncomingCdp::Notification(value));
    }

    Err(jelly_error(
        ErrorKind::Internal,
        "received CDP message with neither response id nor notification method",
        false,
    ))
}

fn read_text_message<S: io::Read + io::Write>(
    ws: &mut WebSocket<S>,
) -> Result<(String, usize), Error> {
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

        match message {
            Message::Text(text) => {
                let text = text.to_string();
                let wire_bytes = text.len();
                return Ok((text, wire_bytes));
            }
            Message::Close(frame) => {
                return Err(browser_unavailable(format!(
                    "CDP websocket closed while waiting for a response{}",
                    frame
                        .map(|frame| format!(": {}", frame.reason))
                        .unwrap_or_default()
                )));
            }
            Message::Binary(_) => {
                return Err(jelly_error(
                    ErrorKind::Internal,
                    "received unexpected binary CDP websocket message",
                    false,
                ));
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
        }
    }
}

fn send<S: io::Read + io::Write>(
    ws: &mut WebSocket<S>,
    id: i64,
    method: &str,
    session_id: Option<&str>,
    params: Value,
) -> Result<(), Error> {
    let mut value = json!({"id":id,"method":method,"params":params});
    if let Some(session_id) = session_id {
        value["sessionId"] = session_id.into();
    }
    ws.send(Message::Text(value.to_string().into()))
        .map_err(|error| browser_unavailable(format!("failed to send CDP message: {error}")))?;
    Ok(())
}

pub(crate) fn browser_unavailable(message: impl Into<String>) -> Error {
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

pub(crate) fn check_error(value: &Value, method: &str) -> Result<(), Error> {
    let Some(error) = value.get("error") else {
        return Ok(());
    };
    let error = error.as_object().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            format!("malformed CDP error response for {method}: error must be an object"),
            false,
        )
    })?;
    let code = error.get("code").and_then(Value::as_i64).ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            format!("malformed CDP error response for {method}: code must be an integer"),
            false,
        )
    })?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                format!("malformed CDP error response for {method}: message must be a string"),
                false,
            )
        })?;
    let data = error.get("data").cloned();

    if invalid_session_message(message) {
        return Err(browser_unavailable(format!(
            "CDP {method} failed ({code}): {message}"
        )));
    }

    Err(cdp_error(method, code, message, data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify_error;

    #[test]
    fn notification_before_response_is_classified_without_loss() {
        match classify_cdp_text(
            r#"{"method":"Page.loadEventFired","sessionId":"session-1","params":{"timestamp":12.5}}"#,
            42,
        )
        .unwrap()
        {
            IncomingCdp::Notification(value) => {
                assert_eq!(value["method"], "Page.loadEventFired");
                assert_eq!(value["sessionId"], "session-1");
            }
            IncomingCdp::Response(_) => panic!("notification classified as response"),
        }

        match classify_cdp_text(r#"{"id":42,"result":{"ok":true}}"#, 42).unwrap() {
            IncomingCdp::Response(value) => assert_eq!(value["result"]["ok"], true),
            IncomingCdp::Notification(_) => panic!("response classified as notification"),
        }
    }

    #[test]
    fn unexpected_response_ids_are_not_silently_discarded() {
        let error = match classify_cdp_text(r#"{"id":41,"result":{}}"#, 42) {
            Ok(_) => panic!("unexpected response id should fail"),
            Err(error) => error,
        };
        assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
        assert!(error.to_string().contains("unexpected CDP response id 41"));
    }

    #[test]
    fn closed_session_cdp_errors_are_retryable_browser_failures() {
        let error = check_error(
            &json!({
                "error": {"code": -32001, "message": "Session with given id not found."}
            }),
            "Runtime.evaluate",
        )
        .unwrap_err();
        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::BrowserUnavailable, true)
        );
        assert!(
            error
                .to_string()
                .contains("CDP Runtime.evaluate failed (-32001)")
        );
    }

    #[test]
    fn ordinary_cdp_errors_are_structured_and_non_retryable() {
        let error = check_error(
            &json!({
                "error": {
                    "code": -32601,
                    "message": "Method not found",
                    "data": {"domain":"Runtime"}
                }
            }),
            "Runtime.missing",
        )
        .unwrap_err();

        assert_eq!(
            classify_error(error.as_ref()),
            (ErrorKind::CdpFailed, false)
        );
        let cdp = error
            .downcast_ref::<crate::CdpError>()
            .expect("ordinary CDP protocol failure must retain CdpError");
        assert_eq!(cdp.method(), "Runtime.missing");
        assert_eq!(cdp.code(), -32601);
        assert_eq!(cdp.protocol_message(), "Method not found");
        assert_eq!(cdp.data(), Some(&json!({"domain":"Runtime"})));
    }

    #[test]
    fn malformed_cdp_error_envelopes_are_internal_contract_failures() {
        for response in [
            json!({"error":"not-an-object"}),
            json!({"error":{"message":"missing code"}}),
            json!({"error":{"code":-32601}}),
        ] {
            let error = check_error(&response, "Runtime.evaluate").unwrap_err();
            assert_eq!(classify_error(error.as_ref()), (ErrorKind::Internal, false));
            assert!(error.to_string().contains("malformed CDP error response"));
        }
    }
}
