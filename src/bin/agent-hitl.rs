use jelly::SCREENSHOT_DIR;
use serde_json::{Value, json};
use std::{env, process::Command};

const SCREENSHOT_NAME: &str = "latest.png";

fn screenshot(target: Option<&str>, desktop_fallback: bool) -> Option<String> {
    let screenshot_bin = env::current_exe().ok()?.parent()?.join("agent-screenshot");
    let path = format!("{SCREENSHOT_DIR}/{SCREENSHOT_NAME}");
    let mut command = Command::new(screenshot_bin);
    if let Some(target) = target {
        command.arg(target);
    }
    command.args(["--output", &path]);
    if desktop_fallback {
        command.arg("--desktop-fallback");
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        eprintln!("HITL browser screenshot failed: {}", error.trim());
        return None;
    }
    Some(path)
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
    let mut message_parts = Vec::new();
    let mut screenshot_target = None;
    let mut no_screenshot = false;
    let mut desktop_fallback = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--screenshot-target" => {
                i += 1;
                screenshot_target = Some(
                    args.get(i)
                        .ok_or("--screenshot-target requires a target")?
                        .clone(),
                );
            }
            "--no-screenshot" => no_screenshot = true,
            "--desktop-fallback" => desktop_fallback = true,
            value => message_parts.push(value.to_owned()),
        }
        i += 1;
    }
    let message = message_parts.join(" ");
    if message.is_empty() {
        return Err(
            "usage: hitl <message> [--screenshot-target target] [--no-screenshot] [--desktop-fallback]"
                .into(),
        );
    }

    let photo = if no_screenshot {
        None
    } else {
        screenshot(screenshot_target.as_deref(), desktop_fallback)
    };
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
