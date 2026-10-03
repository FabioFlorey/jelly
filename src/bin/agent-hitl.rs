use jelly::{
    SCREENSHOT_DIR,
    config::{TelegramHitlFormatConfig, config},
};
use serde_json::{Value, json};
use std::{env, path::Path, process::Command};

const SCREENSHOT_NAME: &str = "latest.png";

fn screenshot(target: Option<&str>) -> Option<String> {
    let screenshot_bin = env::current_exe().ok()?.parent()?.join("agent-screenshot");
    let path = format!("{SCREENSHOT_DIR}/{SCREENSHOT_NAME}");
    let mut command = Command::new(screenshot_bin);
    if let Some(target) = target {
        command.arg(target);
    }
    command.args(["--output", &path]);
    let output = command.output().ok()?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        eprintln!("HITL browser screenshot failed: {}", error.trim());
        return None;
    }
    Some(path)
}

fn escape_telegram_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn render_telegram_message(format: &TelegramHitlFormatConfig, message: &str) -> String {
    let mut header = Vec::new();
    if !format.title.is_empty() {
        header.push(format!("<b>{}</b>", escape_telegram_html(&format.title)));
    }
    if !format.subtitle.is_empty() {
        header.push(escape_telegram_html(&format.subtitle));
    }

    let mut body = Vec::new();
    if !format.prefix.is_empty() {
        body.push(escape_telegram_html(&format.prefix));
    }
    body.push(escape_telegram_html(message));
    if !format.suffix.is_empty() {
        body.push(escape_telegram_html(&format.suffix));
    }

    let mut blocks = Vec::new();
    if !header.is_empty() {
        blocks.push(header.join("\n"));
    }
    blocks.push(body.join("\n"));
    if !format.signature.is_empty() {
        blocks.push(format.signature.clone());
    }
    blocks.join("\n\n")
}

fn send_telegram(
    message: &str,
    photo: Option<&str>,
    video: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    if photo.is_some() && video.is_some() {
        return Err("Telegram HITL accepts either a photo or a video, not both".into());
    }

    dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/.env")).ok();
    let token =
        env::var("JELLY_TELEGRAM_BOT_TOKEN").map_err(|_| "JELLY_TELEGRAM_BOT_TOKEN is not set")?;
    let chat =
        env::var("JELLY_TELEGRAM_CHAT_ID").map_err(|_| "JELLY_TELEGRAM_CHAT_ID is not set")?;

    let formatted_message = render_telegram_message(&config().hitl.formats.telegram, message);
    let (method, media_field, media_path) = if let Some(path) = video {
        ("sendVideo", Some("video"), Some(path))
    } else if let Some(path) = photo {
        ("sendPhoto", Some("photo"), Some(path))
    } else {
        ("sendMessage", None, None)
    };
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let mut cmd = Command::new("curl");
    cmd.args([
        "-fsS",
        "--connect-timeout",
        "10",
        "--max-time",
        "60",
        "-X",
        "POST",
        &url,
        "-F",
        &format!("chat_id={chat}"),
        "--form-string",
        "parse_mode=HTML",
    ]);
    if let (Some(field), Some(path)) = (media_field, media_path) {
        cmd.args([
            "-F",
            &format!("{field}=@{path}"),
            "--form-string",
            &format!("caption={formatted_message}"),
        ]);
    } else {
        cmd.args(["--form-string", &format!("text={formatted_message}")]);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        return Err("Telegram HITL transport failed".into());
    }
    telegram_result(&output.stdout, photo.is_some(), video.is_some())
}

fn telegram_result(
    bytes: &[u8],
    photo: bool,
    video: bool,
) -> Result<Value, Box<dyn std::error::Error>> {
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
        "photo": photo,
        "video": video
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut message_parts = Vec::new();
    let mut screenshot_target = None;
    let mut video = None;
    let mut no_screenshot = false;
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
            "--video" => {
                i += 1;
                video = Some(args.get(i).ok_or("--video requires a path")?.clone());
            }
            "--no-screenshot" => no_screenshot = true,
            value if value.starts_with("--") => {
                return Err(format!("unknown option: {value}").into());
            }
            value => message_parts.push(value.to_owned()),
        }
        i += 1;
    }
    let message = message_parts.join(" ");
    if message.is_empty() {
        return Err(
            "usage: hitl <message> [--video path | --screenshot-target target | --no-screenshot]"
                .into(),
        );
    }

    if video.is_some() && screenshot_target.is_some() {
        return Err("--video and --screenshot-target cannot be used together".into());
    }
    if let Some(path) = video.as_deref()
        && !Path::new(path).is_file()
    {
        return Err(format!("HITL video does not exist: {path}").into());
    }

    let photo = if video.is_some() || no_screenshot {
        None
    } else {
        screenshot(screenshot_target.as_deref())
    };
    let result = send_telegram(&message, photo.as_deref(), video.as_deref())?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telegram_message_uses_configured_regions_and_bold_title() {
        let format = TelegramHitlFormatConfig {
            title: "Jelly & Co".into(),
            subtitle: "Browser instrumentation for agents".into(),
            prefix: "Review:".into(),
            suffix: "Reply when complete.".into(),
            signature: "⏺️ Recorded with <b>Jelly</b>".into(),
        };
        assert_eq!(
            render_telegram_message(&format, "Approve <this> & continue"),
            "<b>Jelly &amp; Co</b>\nBrowser instrumentation for agents\n\nReview:\nApprove &lt;this&gt; &amp; continue\nReply when complete.\n\n⏺️ Recorded with <b>Jelly</b>"
        );
    }

    #[test]
    fn telegram_result_requires_api_success_and_message_id() {
        let ok =
            telegram_result(br#"{"ok":true,"result":{"message_id":42}}"#, true, false).unwrap();
        assert_eq!(ok["accepted"], true);
        assert_eq!(ok["message_id"], 42);
        assert_eq!(ok["photo"], true);
        assert_eq!(ok["video"], false);

        let video =
            telegram_result(br#"{"ok":true,"result":{"message_id":43}}"#, false, true).unwrap();
        assert_eq!(video["photo"], false);
        assert_eq!(video["video"], true);

        let rejected =
            telegram_result(br#"{"ok":false,"description":"bad request"}"#, false, false)
                .unwrap_err()
                .to_string();
        assert!(rejected.contains("bad request"));

        let missing = telegram_result(br#"{"ok":true,"result":{}}"#, false, false)
            .unwrap_err()
            .to_string();
        assert!(missing.contains("message_id"));
    }
}
