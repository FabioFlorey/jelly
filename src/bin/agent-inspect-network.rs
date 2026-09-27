use jelly::{ENDPOINT, NETWORK_DIR};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    process::Command,
};
use tungstenite::{Message, WebSocket, connect};

const PID: &str = "/data/jelly-runtime/network/capture.pid";
const LOG: &str = "/data/jelly-runtime/network/requests.jsonl";

fn recv_id<S: std::io::Read + std::io::Write>(
    ws: &mut WebSocket<S>,
    id: i64,
) -> Result<Value, Box<dyn std::error::Error>> {
    loop {
        let Message::Text(text) = ws.read()? else {
            continue;
        };
        let v: Value = serde_json::from_str(&text)?;
        if v.get("id").and_then(Value::as_i64) == Some(id) {
            return Ok(v);
        }
    }
}

fn capture() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = fs::read_to_string(ENDPOINT)?;
    let (mut ws, _) = connect(endpoint.trim())?;
    ws.send(Message::Text(json!({"id":1,"method":"Target.setAutoAttach","params":{"autoAttach":true,"waitForDebuggerOnStart":false,"flatten":true}}).to_string().into()))?;
    recv_id(&mut ws, 1)?;
    ws.send(Message::Text(
        json!({"id":2,"method":"Target.getTargets"})
            .to_string()
            .into(),
    ))?;
    let targets = recv_id(&mut ws, 2)?;
    let mut sessions = Vec::new();
    let mut next_id = 10i64;
    if let Some(xs) = targets["result"]["targetInfos"].as_array() {
        for x in xs.iter().filter(|x| x["type"] == "page") {
            let Some(target_id) = x["targetId"].as_str() else {
                continue;
            };
            ws.send(Message::Text(json!({"id":next_id,"method":"Target.attachToTarget","params":{"targetId":target_id,"flatten":true}}).to_string().into()))?;
            let r = recv_id(&mut ws, next_id)?;
            if let Some(s) = r["result"]["sessionId"].as_str() {
                sessions.push(s.to_owned());
            }
            next_id += 1;
        }
    }
    for s in &sessions {
        ws.send(Message::Text(
            json!({"id":next_id,"method":"Network.enable","sessionId":s})
                .to_string()
                .into(),
        ))?;
        recv_id(&mut ws, next_id)?;
        next_id += 1;
    }

    let mut out = OpenOptions::new().create(true).append(true).open(LOG)?;
    let mut requests = HashMap::<(String, String), (String, String, String)>::new();
    loop {
        let Message::Text(text) = ws.read()? else {
            continue;
        };
        let v: Value = serde_json::from_str(&text)?;
        if v["method"] == "Target.attachedToTarget" {
            if v["params"]["targetInfo"]["type"] == "page"
                && let Some(s) = v["params"]["sessionId"].as_str()
            {
                ws.send(Message::Text(
                    json!({"id":next_id,"method":"Network.enable","sessionId":s})
                        .to_string()
                        .into(),
                ))?;
                next_id += 1;
            }
            continue;
        }
        let p = &v["params"];
        if v["method"] == "Network.requestWillBeSent" {
            let id = p["requestId"].as_str().unwrap_or("").to_owned();
            let session = v["sessionId"].as_str().unwrap_or("").to_owned();
            requests.insert(
                (session, id),
                (
                    p["request"]["method"].as_str().unwrap_or("").to_owned(),
                    p["request"]["url"].as_str().unwrap_or("").to_owned(),
                    p["type"].as_str().unwrap_or("").to_owned(),
                ),
            );
        } else if v["method"] == "Network.responseReceived" {
            let id = p["requestId"].as_str().unwrap_or("");
            let session = v["sessionId"].as_str().unwrap_or("");
            let Some((method, url, request_type)) =
                requests.get(&(session.to_owned(), id.to_owned()))
            else {
                continue;
            };
            let kind = p["type"].as_str().unwrap_or(request_type);
            let status = p["response"]["status"].as_u64().unwrap_or(0);
            let mime = p["response"]["mimeType"].as_str().unwrap_or("");
            writeln!(
                out,
                "{}",
                json!({"method":method,"status":status,"type":kind,"mime":mime,"url":url})
            )?;
            out.flush()?;
        }
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(NETWORK_DIR)?;
    if let Ok(pid) = fs::read_to_string(PID)
        && Command::new("kill")
            .args(["-0", pid.trim()])
            .status()?
            .success()
    {
        println!("Network inspection already running.");
        return Ok(());
    }
    fs::write(LOG, "")?;
    let exe = env::current_exe()?;
    let child = Command::new(exe).arg("__capture").spawn()?;
    fs::write(PID, child.id().to_string())?;
    println!("Network inspection started.");
    Ok(())
}

fn stop() -> Result<(), Box<dyn std::error::Error>> {
    let pid = fs::read_to_string(PID)?;
    let _ = Command::new("kill").arg(pid.trim()).status();
    let _ = fs::remove_file(PID);
    println!("Network inspection stopped.");
    Ok(())
}

fn show(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let api = args.iter().any(|x| x == "--api");
    let value = |flag: &str| {
        args.windows(2)
            .find(|w| w[0] == flag)
            .map(|w| w[1].as_str())
    };
    let kind = value("--type").map(str::to_lowercase);
    let method = value("--method").map(str::to_uppercase);
    let status = value("--status").and_then(|x| x.parse::<u64>().ok());
    let url = value("--url").unwrap_or("");
    for line in BufReader::new(fs::File::open(LOG)?).lines() {
        let v: Value = serde_json::from_str(&line?)?;
        let t = v["type"].as_str().unwrap_or("");
        let mime = v["mime"].as_str().unwrap_or("");
        if api && !matches!(t, "XHR" | "Fetch") && !mime.contains("json") {
            continue;
        }
        if kind.as_ref().is_some_and(|x| t.to_lowercase() != *x) {
            continue;
        }
        if method
            .as_ref()
            .is_some_and(|x| v["method"].as_str() != Some(x))
        {
            continue;
        }
        if status.is_some_and(|x| v["status"].as_u64() != Some(x)) {
            continue;
        }
        if !v["url"].as_str().unwrap_or("").contains(url) {
            continue;
        }
        println!(
            "{} {} {} {} {}",
            v["method"].as_str().unwrap_or(""),
            v["status"],
            t,
            mime,
            v["url"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("start") => start(),
        Some("stop") => stop(),
        Some("show") => show(&args[1..]),
        Some("__capture") => capture(),
        _ => Err("usage: inspect-network <start|stop|show> [filters]".into()),
    }
}
