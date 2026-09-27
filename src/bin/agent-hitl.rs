use jelly::SCREENSHOT_DIR;
use std::{env, process::Command};

const SCREENSHOT_NAME: &str = "latest.png";

fn screenshot() -> Option<String> {
    let screenshot_bin = env::current_exe().ok()?.parent()?.join("agent-screenshot");
    let path = format!("{SCREENSHOT_DIR}/{SCREENSHOT_NAME}");
    let status = Command::new(screenshot_bin)
        .args(["--output", &path])
        .status()
        .ok()?;
    status.success().then_some(path)
}

fn send_telegram(message: &str, photo: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
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
    cmd.args(["-fsS", "-X", "POST", &url, "-F", &format!("chat_id={chat}")]);
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
    if !cmd.status()?.success() {
        return Err("Telegram HITL request failed".into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    let message = args.join(" ");
    if message.is_empty() {
        return Err("usage: hitl <message>".into());
    }

    let photo = screenshot();
    send_telegram(&message, photo.as_deref())?;
    println!("HITL request sent via Telegram.");
    Ok(())
}
