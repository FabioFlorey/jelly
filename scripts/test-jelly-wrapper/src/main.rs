use serde_json::Value;
use std::{
    collections::HashSet,
    env,
    fs::{self, File},
    io::{self, IsTerminal, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
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
    interactive: bool,
    color: bool,
    icons: bool,
    animation: bool,
}

fn config_value(root: &Path, section: &str, key: &str) -> Option<String> {
    let path = root.join("config/jelly.toml");
    let text = fs::read_to_string(path).ok()?;
    let mut current = "";
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            current = line.trim_matches(['[', ']']);
            continue;
        }
        if current != section {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() == key {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

fn config_bool(root: &Path, section: &str, key: &str) -> bool {
    let value = config_value(root, section, key)
        .unwrap_or_else(|| panic!("missing {section}.{key} in config/jelly.toml"));
    match value.to_ascii_lowercase().as_str() {
        "true" => true,
        "false" => false,
        other => panic!("{section}.{key} must be true or false; got {other}"),
    }
}

impl Theme {
    fn detect(root: &Path) -> Self {
        let interactive = io::stdout().is_terminal();
        Self {
            interactive,
            color: interactive && env::var_os("NO_COLOR").is_none(),
            icons: config_bool(root, "ui", "icons"),
            animation: config_bool(root, "ui", "animation"),
        }
    }

    fn paint(self, text: &str, code: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn info(self) -> &'static str {
        if self.icons { "›" } else { ">" }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("test-jelly wrapper must live under <repo>/scripts/test-jelly-wrapper")
        .to_path_buf()
}

fn terminal_width(theme: Theme) -> usize {
    if !theme.interactive {
        return 100;
    }

    let tty_width = File::open("/dev/tty").ok().and_then(|tty| {
        Command::new("stty")
            .arg("size")
            .stdin(Stdio::from(tty))
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|value| value.split_whitespace().nth(1)?.parse::<usize>().ok())
            .filter(|width| *width >= 40)
    });

    tty_width
        .or_else(|| {
            env::var("COLUMNS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|width| *width >= 40)
        })
        .or_else(|| {
            Command::new("tput")
                .arg("cols")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .and_then(|value| value.trim().parse::<usize>().ok())
                .filter(|width| *width >= 40)
        })
        .unwrap_or(100)
}

fn center_line(line: &str, width: usize) -> String {
    let len = line.chars().count();
    if len >= width {
        return line.to_owned();
    }
    format!("{}{}", " ".repeat((width - len) / 2), line)
}

fn center_block(block: &str, width: usize) -> String {
    let block_width = block
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let padding = width.saturating_sub(block_width) / 2;
    let prefix = " ".repeat(padding);
    block
        .lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn center_lines(block: &str, width: usize) -> String {
    block
        .lines()
        .map(|line| center_line(line, width))
        .collect::<Vec<_>>()
        .join("\n")
}

fn logo_text(root: &Path) -> String {
    let configured =
        config_value(root, "ui", "logo").expect("missing ui.logo in config/jelly.toml");
    fs::read_to_string(root.join(configured))
        .unwrap_or_else(|_| "JELLY".to_owned())
        .trim_end()
        .to_owned()
}

fn logo_animation_enabled(theme: Theme) -> bool {
    theme.interactive
        && theme.animation
        && env::var("TERM").map(|term| term != "dumb").unwrap_or(true)
}

#[allow(clippy::needless_range_loop)] // Column-major animation intentionally indexes the row-major canvas.
fn drip_frame(logo: &str, frame: usize, frames: usize) -> String {
    let rows = logo
        .lines()
        .map(|line| line.chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let height = rows.len().max(1);
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut canvas = vec![vec![' '; width]; height];

    for col in 0..width {
        let delay = (col.wrapping_mul(7) + col / 3) % 4;
        let progress = frame.saturating_sub(delay);
        let reveal_rows = ((progress + 1) * (height + 2) / frames.max(1)).min(height);
        let mut lowest_revealed = None;
        let mut has_more = false;

        for row in 0..height {
            let ch = rows
                .get(row)
                .and_then(|line| line.get(col))
                .copied()
                .unwrap_or(' ');
            if row < reveal_rows {
                if ch != ' ' {
                    canvas[row][col] = ch;
                    lowest_revealed = Some(row);
                }
            } else if ch != ' ' {
                has_more = true;
            }
        }

        if has_more {
            let drip_row = lowest_revealed.map(|row| row + 1).unwrap_or(0);
            if drip_row < height && canvas[drip_row][col] == ' ' {
                canvas[drip_row][col] = if (col + frame).is_multiple_of(3) {
                    '▒'
                } else {
                    '░'
                };
            }
        }
    }

    canvas
        .into_iter()
        .map(|line| line.into_iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn animate_logo(root: &Path, theme: Theme) {
    if !logo_animation_enabled(theme) {
        return;
    }

    let logo = logo_text(root);
    let terminal = terminal_width(theme);
    let logo_width = logo
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    if logo_width == 0 || logo_width > terminal {
        return;
    }

    let height = logo.lines().count();
    let frames = 12;
    let mut stdout = io::stdout();
    let _ = writeln!(stdout, "\x1b[?25l");

    for frame in 0..frames {
        if frame > 0 {
            let _ = write!(stdout, "\x1b[{height}A");
        }
        let rendered = center_block(&drip_frame(&logo, frame, frames), terminal);
        for line in rendered.lines() {
            let _ = writeln!(stdout, "\x1b[2K{}", theme.paint(line, "38;2;255;193;7;1"));
        }
        let _ = stdout.flush();
        thread::sleep(Duration::from_millis(28));
    }

    let _ = write!(stdout, "\x1b[{height}A");
    for _ in 0..height {
        let _ = writeln!(stdout, "\x1b[2K");
    }
    let _ = write!(stdout, "\x1b[{height}A\x1b[?25h");
    let _ = stdout.flush();
}

fn banner(root: &Path, theme: Theme) -> String {
    animate_logo(root, theme);
    let width = terminal_width(theme);
    let logo = center_block(&logo_text(root), width);
    let subtitle = center_line("browser instrumentation for agents", width);
    format!(
        "\n{}\n\n{}\n",
        theme.paint(&logo, "38;2;255;193;7;1"),
        theme.paint(&subtitle, "2")
    )
}

fn centered_heading(theme: Theme, title: &str) -> String {
    theme.paint(&center_line(title, terminal_width(theme)), "1")
}

fn centered_description(theme: Theme, text: &str) -> String {
    let width = terminal_width(theme);
    let content_width = width.saturating_sub(12).clamp(40, 76);
    let lines = wrap_cell(text, content_width);
    let block = lines.join("\n");
    theme.paint(&center_lines(&block, width), "2")
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

fn style_table(
    theme: Theme,
    table: &str,
    bold_first_column: bool,
    bold_name_column: bool,
) -> String {
    let centered = center_block(table, terminal_width(theme));
    if !theme.color {
        return centered;
    }

    let lines = centered.lines().collect::<Vec<_>>();
    let name_range = if bold_name_column {
        lines.first().and_then(|header| {
            let start = header.find("NAME")?;
            let end = header.find("DESCRIPTION").unwrap_or(header.len());
            Some((start, end))
        })
    } else {
        None
    };

    lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 1 {
                return theme.paint(line, "2");
            }
            if index == 0 {
                return (*line).to_owned();
            }

            let mut rendered = (*line).to_owned();
            if let Some((start, end)) = name_range {
                let chars = rendered.chars().collect::<Vec<_>>();
                if start < chars.len() {
                    let end = end.min(chars.len());
                    let cell = chars[start..end].iter().collect::<String>();
                    let leading = cell.chars().take_while(|ch| ch.is_whitespace()).count();
                    let trailing = cell
                        .chars()
                        .rev()
                        .take_while(|ch| ch.is_whitespace())
                        .count();
                    let content_end = cell.chars().count().saturating_sub(trailing);
                    if leading < content_end {
                        let prefix = chars[..start].iter().collect::<String>();
                        let suffix = chars[end..].iter().collect::<String>();
                        let left = cell.chars().take(leading).collect::<String>();
                        let content = cell
                            .chars()
                            .skip(leading)
                            .take(content_end - leading)
                            .collect::<String>();
                        let right = cell.chars().skip(content_end).collect::<String>();
                        rendered = format!(
                            "{prefix}{left}{}{right}{suffix}",
                            theme.paint(&content, "1")
                        );
                    }
                }
            }

            for status in ["PASS", "FAIL", "ENABLED", "DISABLED", "SKIPPED"] {
                let code = match status {
                    "PASS" | "ENABLED" => "32;1",
                    "FAIL" => "31;1",
                    "DISABLED" | "SKIPPED" => "33;1",
                    _ => "1",
                };
                rendered = rendered.replace(
                    &format!(" {status} "),
                    &format!(" {} ", theme.paint(status, code)),
                );
            }
            if bold_first_column {
                let leading = rendered.chars().take_while(|ch| ch.is_whitespace()).count();
                let rest = &rendered[leading..];
                if let Some(end) = rest.find(char::is_whitespace) {
                    let first = &rest[..end];
                    rendered = format!(
                        "{}{}{}",
                        " ".repeat(leading),
                        theme.paint(first, "1"),
                        &rest[end..]
                    );
                }
            }
            rendered
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_catalog(root: &Path) -> Result<Vec<Value>, i32> {
    let output = Command::new("bash")
        .arg(root.join("tests/suite/run.sh"))
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
        println!("{}", banner(root, theme));
        println!("{}", centered_heading(theme, "Tests · catalog"));
        println!();
        println!(
            "{}",
            centered_description(
                theme,
                "Canonical behavioral-test metadata. Read-only view; no tests are executed."
            )
        );
        println!();
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

    let mut headers = vec!["ID", "GROUP", "KIND", "STATUS", "NAME", "DESCRIPTION"];
    let mut keys = vec!["id", "group", "kind", "enabled", "name", "description"];
    if full {
        headers.extend(["INPUT", "EXPECTED"]);
        keys.extend(["input", "expected_output"]);
    }
    let table_rows = rows
        .iter()
        .map(|row| {
            keys.iter()
                .map(|key| {
                    if *key == "enabled" {
                        if field(row, key) == "yes" {
                            "ENABLED".to_owned()
                        } else {
                            "DISABLED".to_owned()
                        }
                    } else {
                        field(row, key)
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let content_width = |index: usize, minimum: usize, maximum: usize| {
        table_rows
            .iter()
            .filter_map(|row| row.get(index))
            .map(|cell| cell.chars().count())
            .fold(headers[index].chars().count(), usize::max)
            .clamp(minimum, maximum)
    };

    let id_width = content_width(0, 7, 10);
    let group_width = content_width(1, 5, 14);
    let kind_width = content_width(2, 4, 10);
    let status_width = content_width(3, 6, 8);
    let technical_width = id_width + group_width + kind_width + status_width;
    let column_count = if full { 8 } else { 6 };
    let gap_width = (column_count - 1) * 2;
    let terminal = terminal_width(theme);
    let target_width = terminal.saturating_sub(8).min(if full { 220 } else { 180 });
    let flexible = target_width.saturating_sub(technical_width + gap_width);

    let widths = if full {
        let name = (flexible * 22 / 100).max(20);
        let description = (flexible * 30 / 100).max(28);
        let input = (flexible * 24 / 100).max(22);
        let expected = flexible.saturating_sub(name + description + input).max(22);
        vec![
            id_width,
            group_width,
            kind_width,
            status_width,
            name,
            description,
            input,
            expected,
        ]
    } else {
        let name = (flexible * 38 / 100).max(24);
        let description = flexible.saturating_sub(name).max(34);
        vec![
            id_width,
            group_width,
            kind_width,
            status_width,
            name,
            description,
        ]
    };
    let enabled = rows
        .iter()
        .filter(|row| field(row, "enabled") == "yes")
        .count();
    let disabled = rows.len().saturating_sub(enabled);
    let mut review = format!(
        "{} test{} · {} enabled",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        enabled
    );
    if disabled > 0 {
        review.push_str(&format!(" · {} disabled", disabled));
    }
    println!(
        "{}",
        theme.paint(&center_line(&review, terminal_width(theme)), "1")
    );
    println!();
    println!(
        "{}",
        style_table(
            theme,
            &render_table(&headers, &table_rows, Some(widths), true),
            true,
            true
        )
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

    println!("{}", banner(root, theme));
    println!("{}", centered_heading(theme, "Tests · rust"));
    println!();
    println!(
        "{}",
        centered_description(
            theme,
            "Rust unit and target-level checks executed through Cargo with the original exit status preserved."
        )
    );
    println!();
    println!(
        "{}",
        theme.paint(&center_line(&display.join(" "), terminal_width(theme)), "2")
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
        let passed: u64 = summaries.iter().map(|row| row.passed).sum();
        let failed: u64 = summaries.iter().map(|row| row.failed).sum();
        let ignored: u64 = summaries.iter().map(|row| row.ignored).sum();
        let tests = passed + failed + ignored;
        let review = format!(
            "{tests} test{} · {passed} passed · {failed} failed{}",
            if tests == 1 { "" } else { "s" },
            if ignored > 0 {
                format!(" · {ignored} ignored")
            } else {
                String::new()
            }
        );
        let review_code = if failed == 0 { "32;1" } else { "31;1" };
        println!(
            "{}",
            theme.paint(&center_line(&review, terminal_width(theme)), review_code)
        );
        println!();
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
                ),
                false,
                false
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

fn clear_screen(theme: Theme) {
    if theme.interactive {
        print!("\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
    }
}

fn main() -> ExitCode {
    let root = repo_root();
    let theme = Theme::detect(&root);
    let args = env::args().skip(1).collect::<Vec<_>>();

    let before_separator = args
        .iter()
        .position(|arg| arg == "--")
        .map(|index| &args[..index])
        .unwrap_or(&args);

    if !before_separator.iter().any(|arg| arg == "--json") {
        clear_screen(theme);
    }

    if before_separator
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        println!("{}", banner(&root, theme));
        println!("{}", centered_heading(theme, "Tests · help"));
        println!();
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
