use jelly::{
    ErrorKind, cancel_download, download_record, download_records, jelly_error, wait_download,
};
use serde_json::json;
use std::env;

fn usage() -> jelly::Error {
    jelly_error(
        ErrorKind::InvalidArguments,
        "usage: download <list|status|wait|cancel> [id] [seconds] [destination] [fail|overwrite|uniquify]",
        false,
    )
}

fn main() -> Result<(), jelly::Error> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let action = args.first().map(String::as_str).ok_or_else(usage)?;

    let value = match action {
        "list" if args.len() == 1 => json!({
            "downloads": download_records()?
        }),
        "status" if args.len() == 2 => serde_json::to_value(download_record(&args[1])?)?,
        "cancel" if args.len() == 2 => serde_json::to_value(cancel_download(&args[1])?)?,
        "wait" if (2..=5).contains(&args.len()) => {
            let seconds = args
                .get(2)
                .map(|value| value.parse::<u64>())
                .transpose()
                .map_err(|_| {
                    jelly_error(
                        ErrorKind::InvalidArguments,
                        "download wait seconds must be an integer",
                        false,
                    )
                })?
                .unwrap_or(30);
            let destination = args.get(3).map(String::as_str);
            let collision = args.get(4).map(String::as_str).unwrap_or("fail");
            serde_json::to_value(wait_download(&args[1], seconds, destination, collision)?)?
        }
        _ => return Err(usage()),
    };

    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}
