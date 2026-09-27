use std::env;

fn main() -> Result<(), jelly::Error> {
    let args: Vec<String> = env::args().skip(1).collect();
    let value = args.first().ok_or_else(|| {
        jelly::jelly_error(
            jelly::ErrorKind::InvalidArguments,
            "usage: verify-artifact <artifact-id|path> [--semantic check...]",
            false,
        )
    })?;
    let mut result = jelly::verify_artifact(value)?;
    if let Some(index) = args.iter().position(|arg| arg == "--semantic") {
        let checks = args[index + 1..].to_vec();
        if checks.is_empty() {
            return Err(jelly::jelly_error(
                jelly::ErrorKind::InvalidArguments,
                "--semantic requires at least one evidence label",
                false,
            ));
        }
        result = jelly::mark_artifact_verified(value, &checks)?;
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
