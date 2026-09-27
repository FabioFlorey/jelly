use crate::{ACTIVE_TARGET, ENDPOINT, Error, PAGE_TARGET};
use serde_json::{Value, json};
use std::{fs, io};
use tungstenite::{Message, WebSocket, connect};

pub struct BrowserSession {
    ws: WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>,
    session: String,
    target_id: String,
    id: i64,
}

impl BrowserSession {
    pub fn connect() -> Result<Self, Error> {
        let ep = fs::read_to_string(ENDPOINT)?;
        let (mut ws, _) = connect(ep.trim())?;
        send(&mut ws, 1, "Target.getTargets", None, json!({}))?;
        let v = recv(&mut ws, 1)?;
        check_error(&v)?;
        let preferred = fs::read_to_string(ACTIVE_TARGET)
            .ok()
            .or_else(|| fs::read_to_string(PAGE_TARGET).ok());
        let tid = v["result"]["targetInfos"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|x| {
                        x["type"] == "page"
                            && preferred
                                .as_deref()
                                .is_some_and(|id| x["targetId"].as_str() == Some(id.trim()))
                    })
                    .or_else(|| {
                        a.iter().find(|x| {
                            x["type"] == "page"
                                && x["url"].as_str().is_some_and(|u| u.starts_with("http"))
                        })
                    })
            })
            .and_then(|x| x["targetId"].as_str())
            .ok_or("no web page target")?
            .to_owned();
        send(
            &mut ws,
            2,
            "Target.activateTarget",
            None,
            json!({"targetId":tid}),
        )?;
        check_error(&recv(&mut ws, 2)?)?;
        send(
            &mut ws,
            3,
            "Target.attachToTarget",
            None,
            json!({"targetId":tid,"flatten":true}),
        )?;
        let v = recv(&mut ws, 3)?;
        check_error(&v)?;
        Ok(Self {
            ws,
            session: v["result"]["sessionId"]
                .as_str()
                .ok_or("attach failed")?
                .into(),
            target_id: tid,
            id: 3,
        })
    }

    pub fn target_id(&self) -> &str {
        &self.target_id
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
        self.id += 1;
        let id = self.id;
        send(&mut self.ws, id, method, None, params)?;
        let v = recv(&mut self.ws, id)?;
        check_error(&v)?;
        Ok(v)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, Error> {
        self.id += 1;
        let id = self.id;
        send(&mut self.ws, id, method, Some(&self.session), params)?;
        let v = recv(&mut self.ws, id)?;
        check_error(&v)?;
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
            return Err(msg.to_owned().into());
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
    ws.send(Message::Text(v.to_string().into()))?;
    Ok(())
}
fn recv<S: io::Read + io::Write>(ws: &mut WebSocket<S>, id: i64) -> Result<Value, Error> {
    loop {
        if let Message::Text(t) = ws.read()? {
            let v: Value = serde_json::from_str(&t)?;
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(v);
            }
        }
    }
}
fn check_error(v: &Value) -> Result<(), Error> {
    if let Some(e) = v.get("error") {
        return Err(format!(
            "CDP {}: {}",
            e["code"],
            e["message"].as_str().unwrap_or("unknown error")
        )
        .into());
    }
    Ok(())
}
