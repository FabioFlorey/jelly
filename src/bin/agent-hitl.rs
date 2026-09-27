use jelly::SCREENSHOT_DIR;
use serde_json::{Value, json};
use std::{env, process::Command};

const SCREENSHOT_NAME: &str = "latest.png";

fn screenshot() -> Option<String> {
    let screenshot_bin = env::current_exe().ok()?.parent()?.join("agent-screenshot");
    let path = format!("{SCREENSHOT_DIR}/{SCREENSHOT_NAME}");
    let status = Command::new(screenshot_bin)
        .args(["--output", &path])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    status.success().then_some(path)
}

fn send_telegram(message: &str, photo: Option<&str>) -> Result<Value, Box<dyn std::error::Error>> {
    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();
    let token =
        env::var("JELLY_TELEGRAM_BOT_TOKEN").map_err(|_| "JELLY_TELEGRAM_BOT_TOKEN is not set")?;
    let chat =
        env::var("JELLY_TELEGRAM_CHAT_ID").map_err(|_| "JELLY_TELEGRAM_CHAT_ID is not set")?;

    let method = if photo.is_some() {
        "sendPhoto"
    } else {
        "sendMessage"
    };
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fsS",
        "--connect-timeout",
        "10",
        "--max-time",
        "30",
        "-X",
        "POST",
        &url,
        "-F",
        &format!("chat_id={chat}"),
    ]);
    if let Some(path) = photo {
        cmd.args([
            "-F",
            &format!("photo=@{path}"),
            "--form-string",
            &format!("caption={message}"),
        ]);
    } else {
        cmd.args(["--form-string", &format!("text={message}")]);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        return Err("Telegram HITL transport failed".into());
    }
    telegram_result(&output.stdout, photo.is_some())
}

fn telegram_result(bytes: &[u8], photo: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let response: Value = serde_json::from_slice(bytes)
        .map_err(|_| "Telegram HITL returned an invalid JSON response")?;
    if response["ok"] != true {
        let description = response["description"]
            .as_str()
            .unwrap_or("Telegram rejected the request");
        return Err(format!("Telegram HITL rejected request: {description}").into());
    }
    let message_id = response["result"]["message_id"]
        .as_i64()
        .ok_or("Telegram response did not include a message_id")?;

    Ok(json!({
        "transport": "telegram",
        "accepted": true,
        "message_id": message_id,
        "photo": photo
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let message = args.join(" ");
    if message.is_empty() {
        return Err("usage: hitl <message>".into());
    }

    let photo = screenshot();
    let result = send_telegram(&message, photo.as_deref())?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_result_requires_api_success_and_message_id() {
        let ok = telegram_result(br#"{"ok":true,"result":{"message_id":42}}"#, true).unwrap();
        assert_eq!(ok["accepted"], true);
        assert_eq!(ok["message_id"], 42);
        assert_eq!(ok["photo"], true);

        let rejected = telegram_result(br#"{"ok":false,"description":"bad request"}"#, false)
            .unwrap_err()
            .to_string();
        assert!(rejected.contains("bad request"));

        let missing = telegram_result(br#"{"ok":true,"result":{}}"#, false)
            .unwrap_err()
            .to_string();
        assert!(missing.contains("message_id"));
    }
}
