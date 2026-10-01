use serde_json::Value;
use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const HELP: &str = r#"Usage:
  cargo test-jelly [CARGO_TEST_ARGS...] [-- LIBTEST_ARGS...]
  cargo test-jelly --catalog [CATALOG_OPTIONS]

Run mode:
  Runs the real `cargo test` command, forwards all arguments unchanged, then
  renders one summary row per Rust test target. The exit code is exactly the
  exit code returned by `cargo test`.

Catalog mode:
  Reads Jelly's canonical behavioral-test metadata from tests/suite/run.sh
  without executing tests.

Options:
  -h, --help          Show this help.
  --raw               Also print the captured cargo test output.
  --catalog           Show Jelly behavioral tests and their metadata.
  --id ID             Catalog: select one stable test ID (repeatable).
  --group GROUP       Catalog: filter by group (repeatable).
  --kind KIND         Catalog: filter by kind (repeatable).
  --full              Catalog: also show input and expected output.
  --json              Catalog: emit canonical filtered JSON instead of a table.

Examples:
  cargo test-jelly
  cargo test-jelly --all-targets
  cargo test-jelly --lib error::tests
  cargo test-jelly --all-targets -- --nocapture
  cargo test-jelly --catalog
  cargo test-jelly --catalog --group semantic
  cargo test-jelly --catalog --id QLT-003 --full
  cargo test-jelly --catalog --json

Notes:
  * `--all-targets`, `--lib`, test filters, feature flags, etc. are cargo
    arguments and are forwarded unchanged.
  * Arguments after `--` are libtest arguments and are forwarded unchanged.
  * Use `cargo test-jelly -- --help` to ask libtest for its own help.
  * The metadata fields come from the same `jt_register` entries that execute
    tests: id, group, name, description, preconditions, input, expected_output,
    kind, and enabled.
"#;

#[derive(Debug)]
struct RustSummary {
    target: String,
    passed: u64,
    failed: u64,
    ignored: u64,
    measured: u64,
    filtered: u64,
    duration: String,
    status: String,
}

#[derive(Clone, Copy)]
struct Theme {
    color: bool,
    icons: bool,
}

impl Theme {
    fn detect() -> Self {
        Self {
            color: io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none(),
            icons: env::var("JELLY_NO_ICONS")
                .map(|value| !value.eq_ignore_ascii_case("true"))
                .unwrap_or(true),
        }
    }

    fn paint(self, text: &str, code: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn brand(self) -> &'static str {
        if self.icons { "◆" } else { "*" }
    }

    fn info(self) -> &'static str {
        if self.icons { "›" } else { ">" }
    }

    fn ok(self) -> &'static str {
        if self.icons { "✓" } else { "+" }
    }

    fn fail(self) -> &'static str {
        if self.icons { "✕" } else { "x" }
    }
}

fn repo_root() -> PathBuf {
    env::var_os("JELLY_REPO_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("test-jelly wrapper must live under <repo>/scripts/test-jelly-wrapper")
                .to_path_buf()
        })
}

fn banner(root: &Path, theme: Theme, mode: &str, detail: &str) -> String {
    let logo = fs::read_to_string(root.join("assets/quickstart-full-logo.txt"))
        .unwrap_or_else(|_| "JELLY".to_owned())
        .trim_end()
        .to_owned();
    let subtitle = format!(
        "{}  browser instrumentation for agents  ·  {mode}",
        theme.brand()
    );
    let mut lines = vec![
        String::new(),
        theme.paint(&logo, "38;2;255;193;7;1"),
        String::new(),
        theme.paint(&subtitle, "38;2;255;193;7;1"),
    ];
    if !detail.is_empty() {
        lines.push(theme.paint(&format!("   {detail}"), "2"));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn section_title(theme: Theme, title: &str) -> String {
    let marker = if theme.icons { "◇" } else { ">" };
    theme.paint(&format!("{marker}  {title}"), "38;2;255;193;7;1")
}

fn target_label(raw: &str) -> String {
    let raw = raw.trim();
    if raw == "unittests src/main.rs" {
        return "bin:jelly".into();
    }
    if let Some(rest) = raw.strip_prefix("unittests scripts/") {
        return format!("bin:{}", rest.trim_end_matches(".rs").replace('_', "-"));
    }
    for (prefix, label) in [
        ("unittests src/lib.rs", "lib"),
        ("unittests src/bin/", "bin:"),
        ("tests/", "test:"),
        ("benches/", "bench:"),
        ("examples/", "example:"),
    ] {
        if let Some(rest) = raw.strip_prefix(prefix) {
            if rest.is_empty() {
                return label.into();
            }
            return format!("{label}{}", rest.trim_end_matches(".rs"));
        }
    }
    raw.into()
}

fn parse_summary_line(line: &str, current: &str) -> Option<RustSummary> {
    let rest = line.trim().strip_prefix("test result: ")?;
    let (status, rest) = rest.split_once(". ")?;
    let parts = rest.split("; ").collect::<Vec<_>>();
    if parts.len() != 6 {
        return None;
    }

    fn leading_u64(value: &str) -> Option<u64> {
        value.split_whitespace().next()?.parse().ok()
    }

    Some(RustSummary {
        target: current.to_owned(),
        status: status.to_owned(),
        passed: leading_u64(parts[0])?,
        failed: leading_u64(parts[1])?,
        ignored: leading_u64(parts[2])?,
        measured: leading_u64(parts[3])?,
        filtered: leading_u64(parts[4])?,
        duration: parts[5]
            .strip_prefix("finished in ")?
            .split_whitespace()
            .next()?
            .to_owned(),
    })
}

fn parse_rust_summaries(output: &str) -> Vec<RustSummary> {
    let mut summaries = Vec::new();
    let mut current = "unknown".to_owned();
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(doc) = trimmed.strip_prefix("Doc-tests ") {
            current = format!("doc:{doc}");
            continue;
        }
        if let Some(running) = trimmed.strip_prefix("Running ") {
            let target = running
                .split_once(" (")
                .map(|(value, _)| value)
                .unwrap_or(running);
            current = target_label(target);
            continue;
        }
        if let Some(summary) = parse_summary_line(trimmed, &current) {
            summaries.push(summary);
        }
    }
    summaries
}

fn wrap_cell(value: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    for source_line in value.lines().chain((value.is_empty()).then_some("")) {
        let mut line = source_line.trim();
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        while line.chars().count() > width {
            let mut split_byte = line.len();
            let mut last_space = None;
            for (count, (byte, ch)) in line.char_indices().enumerate() {
                if count >= width {
                    split_byte = last_space.unwrap_or(byte);
                    break;
                }
                if ch.is_whitespace() {
                    last_space = Some(byte);
                }
            }
            if split_byte == 0 {
                split_byte = line
                    .char_indices()
                    .nth(width)
                    .map(|(byte, _)| byte)
                    .unwrap_or(line.len());
            }
            let (head, tail) = line.split_at(split_byte);
            out.push(head.trim().to_owned());
            line = tail.trim_start();
        }
        out.push(line.to_owned());
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn render_table(
    headers: &[&str],
    rows: &[Vec<String>],
    widths: Option<Vec<usize>>,
    row_gap: bool,
) -> String {
    let widths = widths.unwrap_or_else(|| {
        headers
            .iter()
            .enumerate()
            .map(|(index, header)| {
                rows.iter()
                    .filter_map(|row| row.get(index))
                    .map(|cell| cell.chars().count())
                    .fold(header.chars().count(), usize::max)
            })
            .collect()
    });
    let gap = "  ";

    fn format_cells(cells: &[String], widths: &[usize], gap: &str) -> String {
        let mut rendered = String::new();
        for (index, width) in widths.iter().copied().enumerate() {
            if index > 0 {
                rendered.push_str(gap);
            }
            let cell = cells.get(index).map(String::as_str).unwrap_or("");
            rendered.push_str(cell);
            let pad = width.saturating_sub(cell.chars().count());
            rendered.push_str(&" ".repeat(pad));
        }
        rendered.trim_end().to_owned()
    }

    let header_cells = headers
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<Vec<_>>();
    let mut lines = vec![format_cells(&header_cells, &widths, gap)];
    lines.push(
        widths
            .iter()
            .map(|width| "─".repeat(*width))
            .collect::<Vec<_>>()
            .join(gap),
    );

    for (row_index, row) in rows.iter().enumerate() {
        let wrapped = row
            .iter()
            .enumerate()
            .map(|(index, value)| wrap_cell(value, widths[index]))
            .collect::<Vec<_>>();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        for line_index in 0..height {
            let cells = wrapped
                .iter()
                .map(|lines| lines.get(line_index).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            lines.push(format_cells(&cells, &widths, gap));
        }
        if row_gap && row_index + 1 < rows.len() {
            lines.push(String::new());
        }
    }
    lines.join("\n")
}

fn style_table(theme: Theme, table: &str) -> String {
    if !theme.color {
        return table.to_owned();
    }
    table
        .lines()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                return theme.paint(line, "38;2;255;193;7;1");
            }
            if index == 1 {
                return theme.paint(line, "38;2;255;193;7");
            }
            line.replace(" PASS ", &format!(" {} ", theme.paint("PASS", "32;1")))
                .replace(" FAIL ", &format!(" {} ", theme.paint("FAIL", "31;1")))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_catalog(root: &Path) -> Result<Vec<Value>, i32> {
    let output = Command::new(root.join("tests/suite/run.sh"))
        .args(["--catalog", "--json"])
        .current_dir(root)
        .output()
        .map_err(|error| {
            eprintln!("test-jelly: failed to execute catalog command: {error}");
            127
        })?;
    if !output.status.success() {
        let _ = io::stderr().write_all(&output.stderr);
        return Err(output.status.code().unwrap_or(1));
    }
    let value: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        eprintln!("test-jelly: invalid catalog JSON: {error}");
        2
    })?;
    value.as_array().cloned().ok_or_else(|| {
        eprintln!("test-jelly: catalog root must be an array");
        2
    })
}

fn field(row: &Value, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(value)) => {
            if *value {
                "yes".into()
            } else {
                "no".into()
            }
        }
        Some(Value::Null) | None => String::new(),
        Some(value) => value.to_string(),
    }
}

fn run_catalog(root: &Path, theme: Theme, args: &[String]) -> i32 {
    let mut ids = Vec::new();
    let mut groups = Vec::new();
    let mut kinds = Vec::new();
    let mut full = false;
    let mut as_json = false;

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return 0;
            }
            "--full" => full = true,
            "--json" => as_json = true,
            "--id" | "--group" | "--kind" => {
                if index + 1 >= args.len() {
                    eprintln!("test-jelly: {} requires a value", args[index]);
                    return 2;
                }
                let value = args[index + 1].clone();
                match args[index].as_str() {
                    "--id" => ids.push(value),
                    "--group" => groups.push(value),
                    _ => kinds.push(value),
                }
                index += 1;
            }
            other => {
                eprintln!("test-jelly: unknown catalog option: {other}");
                return 2;
            }
        }
        index += 1;
    }

    if !as_json {
        println!(
            "{}",
            banner(
                root,
                theme,
                "test catalog",
                "canonical jt_register metadata · read-only · no tests executed"
            )
        );
        println!("{}", section_title(theme, "Behavioral test catalog"));
    }

    let mut rows = match load_catalog(root) {
        Ok(rows) => rows,
        Err(code) => return code,
    };

    if !ids.is_empty() {
        let known = rows
            .iter()
            .map(|row| field(row, "id"))
            .collect::<HashSet<_>>();
        let missing = ids
            .iter()
            .filter(|id| !known.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            eprintln!(
                "test-jelly: unknown catalog test ID(s): {}",
                missing.join(", ")
            );
            return 2;
        }
        rows.retain(|row| ids.contains(&field(row, "id")));
    }
    if !groups.is_empty() {
        rows.retain(|row| groups.contains(&field(row, "group")));
    }
    if !kinds.is_empty() {
        rows.retain(|row| kinds.contains(&field(row, "kind")));
    }

    if as_json {
        match serde_json::to_string_pretty(&rows) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("test-jelly: failed to serialize catalog JSON: {error}");
                return 2;
            }
        }
        return 0;
    }

    if rows.is_empty() {
        println!(
            "{}",
            theme.paint(
                &format!(
                    "{}  No Jelly behavioral tests match the selected filters.",
                    theme.info()
                ),
                "2"
            )
        );
        return 0;
    }

    let mut headers = vec![
        "ID",
        "GROUP",
        "KIND",
        "ENABLED",
        "NAME",
        "DESCRIPTION",
        "PRECONDITIONS",
    ];
    let mut keys = vec![
        "id",
        "group",
        "kind",
        "enabled",
        "name",
        "description",
        "preconditions",
    ];
    if full {
        headers.extend(["INPUT", "EXPECTED"]);
        keys.extend(["input", "expected_output"]);
    }
    let table_rows = rows
        .iter()
        .map(|row| keys.iter().map(|key| field(row, key)).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let widths = if full {
        vec![9, 12, 9, 7, 28, 24, 24, 24, 24]
    } else {
        vec![9, 12, 9, 7, 28, 36, 36]
    };
    println!(
        "{}",
        style_table(
            theme,
            &render_table(&headers, &table_rows, Some(widths), true)
        )
    );
    println!(
        "{}",
        theme.paint(&format!("\n{}  {} test(s)", theme.info(), rows.len()), "2")
    );
    0
}

fn split_wrapper_args(args: &[String]) -> (Vec<String>, bool) {
    let mut raw = false;
    let mut forwarded = Vec::new();
    let mut after_separator = false;
    for arg in args {
        if arg == "--" {
            after_separator = true;
            forwarded.push(arg.clone());
        } else if !after_separator && arg == "--raw" {
            raw = true;
        } else {
            forwarded.push(arg.clone());
        }
    }
    (forwarded, raw)
}

fn capture_cargo_test(root: &Path, args: &[String]) -> io::Result<(i32, String)> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = env::temp_dir().join(format!(
        "jelly-test-jelly-{}-{nonce}.log",
        std::process::id()
    ));
    let stdout = File::create(&path)?;
    let stderr = stdout.try_clone()?;
    let status = Command::new("cargo")
        .arg("test")
        .args(args)
        .current_dir(root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()?;
    let output = fs::read_to_string(&path).unwrap_or_default();
    let _ = fs::remove_file(path);
    Ok((status.code().unwrap_or(1), output))
}

fn run_rust_tests(root: &Path, theme: Theme, args: &[String]) -> i32 {
    let (forwarded, raw) = split_wrapper_args(args);
    let mut display = vec!["cargo".to_owned(), "test".to_owned()];
    display.extend(forwarded.iter().cloned());

    println!(
        "{}",
        banner(
            root,
            theme,
            "test console",
            "tabular cargo test frontend · exact cargo exit status preserved"
        )
    );
    println!("{}", section_title(theme, "Rust test run"));
    println!(
        "{}",
        theme.paint(&format!("{}  {}", theme.info(), display.join(" ")), "2")
    );
    println!();

    let (code, output) = match capture_cargo_test(root, &forwarded) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("test-jelly: failed to run cargo test: {error}");
            return 127;
        }
    };
    let summaries = parse_rust_summaries(&output);

    if summaries.is_empty() {
        println!("No libtest summary rows were produced (cargo exit={code}).");
    } else {
        let rows = summaries
            .iter()
            .map(|summary| {
                vec![
                    summary.target.clone(),
                    if summary.status.eq_ignore_ascii_case("ok") {
                        "PASS".into()
                    } else {
                        summary.status.to_uppercase()
                    },
                    summary.passed.to_string(),
                    summary.failed.to_string(),
                    summary.ignored.to_string(),
                    summary.measured.to_string(),
                    summary.filtered.to_string(),
                    summary.duration.clone(),
                ]
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            style_table(
                theme,
                &render_table(
                    &[
                        "TARGET", "STATUS", "PASS", "FAIL", "IGNORED", "MEASURED", "FILTERED",
                        "TIME"
                    ],
                    &rows,
                    None,
                    false
                )
            )
        );
        let passed: u64 = summaries.iter().map(|row| row.passed).sum();
        let failed: u64 = summaries.iter().map(|row| row.failed).sum();
        let ignored: u64 = summaries.iter().map(|row| row.ignored).sum();
        let icon = if code == 0 { theme.ok() } else { theme.fail() };
        let total = format!(
            "{icon}  TOTAL  passed={passed} failed={failed} ignored={ignored} targets={} exit={code}",
            summaries.len()
        );
        println!(
            "{}",
            theme.paint(
                &format!("\n{total}"),
                if code == 0 { "32;1" } else { "31;1" }
            )
        );
    }

    if (raw || code != 0 || summaries.is_empty()) && !output.trim().is_empty() {
        println!("\nCargo output:");
        println!("{}", output.trim_end());
    }
    code
}

fn exit_code(code: i32) -> ExitCode {
    if (0..=255).contains(&code) {
        ExitCode::from(code as u8)
    } else {
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let root = repo_root();
    let theme = Theme::detect();
    let args = env::args().skip(1).collect::<Vec<_>>();

    let before_separator = args
        .iter()
        .position(|arg| arg == "--")
        .map(|index| &args[..index])
        .unwrap_or(&args);
    if before_separator
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!(
            "{}",
            banner(
                &root,
                theme,
                "test console",
                "Rust tests + behavioral metadata"
            )
        );
        println!("{}", section_title(theme, "Help"));
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }

    let code = if args.first().is_some_and(|arg| arg == "--catalog") {
        run_catalog(&root, theme, &args[1..])
    } else {
        run_rust_tests(&root, theme, &args)
    };
    exit_code(code)
}
