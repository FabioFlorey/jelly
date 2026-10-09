//! Configuration and repository maintenance commands.
#[path = "../../scripts/maintenance/architecture.rs"]
mod architecture;
#[path = "../../scripts/maintenance/docs.rs"]
mod docs;
#[path = "../../scripts/maintenance/env.rs"]
mod environment;
#[path = "../../scripts/maintenance/guidance.rs"]
mod guidance;
#[path = "../../scripts/maintenance/ranking.rs"]
mod ranking;

use std::{env, error::Error, net::TcpListener, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let command = args
        .next()
        .ok_or("usage: jelly-maint <env|config|check|ports> ...")?;
    match command.as_str() {
        "env" => {
            let mode = args.next().ok_or("expected env read|write")?;
            let path = PathBuf::from(args.next().ok_or("expected .env path")?);
            if args.next().is_some() {
                return Err("too many arguments".into());
            }
            match mode.as_str() {
                "read" => environment::read_command(&path)?,
                "write" => environment::write_command(&path)?,
                "write-rows" => environment::write_rows_command(&path)?,
                _ => return Err("expected env read|write".into()),
            }
        }
        "config" => {
            let mode = args.next().ok_or("expected config set")?;
            if mode != "set" {
                return Err("expected config set".into());
            }
            let path = PathBuf::from(args.next().ok_or("expected config path")?);
            let section = args.next().ok_or("expected section")?;
            let key = args.next().ok_or("expected key")?;
            let value = args.next().ok_or("expected value")?;
            if args.next().is_some() {
                return Err("too many arguments".into());
            }
            environment::config_set(&path, &section, &key, &value)?;
        }
        "check" => match args
            .next()
            .ok_or("expected docs|architecture|security|agent-guidance")?
            .as_str()
        {
            "docs" => docs::check(&root())?,
            "architecture" => architecture::check(&root())?,
            "security" => environment::test_security()?,
            "agent-guidance" => guidance::run(&root(), args.collect())?,
            _ => return Err("unknown check type".into()),
        },
        "test" => match args.next().ok_or("expected ranking")?.as_str() {
            "ranking" => ranking::run(&root())?,
            _ => return Err("unknown test type".into()),
        },
        "ports" => {
            if args.next().is_some() {
                return Err("ports has no arguments".into());
            }
            let a = TcpListener::bind("127.0.0.1:0")?;
            let b = TcpListener::bind("127.0.0.1:0")?;
            println!("{} {}", a.local_addr()?.port(), b.local_addr()?.port());
        }
        _ => return Err("unknown jelly-maint command".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("jelly-maint: {error}");
        std::process::exit(2);
    }
}
