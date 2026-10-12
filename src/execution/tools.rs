use serde_json::{Value, json};

#[derive(Debug, Clone, Copy)]
pub struct ToolCategorySpec {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub category: &'static str,
}

impl ToolSpec {
    pub fn schema(&self) -> Value {
        json!({
            "name": self.name,
            "kind": "system",
            "description": self.description,
            "usage": self.usage,
            "category": self.category,
        })
    }
}

pub static TOOL_CATEGORIES: &[ToolCategorySpec] = &[
    ToolCategorySpec {
        name: "browser-lifecycle",
        description: "Start and stop the persistent Chromium browser service and run scoped browser tasks.",
    },
    ToolCategorySpec {
        name: "artifacts",
        description: "Capture browser artifacts and inspect or control first-class download lifecycle state.",
    },
    ToolCategorySpec {
        name: "network",
        description: "Capture and inspect browser network traffic.",
    },
    ToolCategorySpec {
        name: "routines",
        description: "Discover, save, run and resume named JSON graph workflows with variables, branches, loops and calls to other routines.",
    },
    ToolCategorySpec {
        name: "hitl",
        description: "Pause workflows for human intervention. Telegram is the current HITL transport.",
    },
];

pub static TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "open-browser",
        description: "Start the persistent Chromium browser service, optionally opening a URL. Headed mode is the default.",
        usage: "open-browser [--headless] [url]",
        category: "browser-lifecycle",
    },
    ToolSpec {
        name: "close-browser",
        description: "Gracefully stop the persistent jelly Chromium browser service and clean its state files.",
        usage: "close-browser",
        category: "browser-lifecycle",
    },
    ToolSpec {
        name: "browser-task",
        description: "Open a headed browser, run one agent tool, and close the browser unless persistence is requested.",
        usage: "browser-task [--persist] <url> <agent-tool> [args...]",
        category: "browser-lifecycle",
    },
    ToolSpec {
        name: "profile-import",
        description: "Copy a closed Chromium user-data directory into Jelly-managed runtime state for session reuse. This does not guarantee anti-bot or CAPTCHA behavior.",
        usage: "profile-import <source> [--force]",
        category: "browser-lifecycle",
    },
    ToolSpec {
        name: "screenshot",
        description: "Capture a single browser-rendered viewport or targeted element. For a multi-action video use record-browser, not repeated screenshots.",
        usage: "screenshot [target] [output] [--output path]",
        category: "artifacts",
    },
    ToolSpec {
        name: "record-browser",
        description: "Start or stop a browser-content MP4. Use steps for captioned action-by-action evidence; continuous streams renderer frames. Use screenshot for one still image.",
        usage: "record-browser start [--mode continuous|steps] [--interval-ms n] [--hold-ms n] | record-browser stop",
        category: "artifacts",
    },
    ToolSpec {
        name: "download",
        description: "Inspect and control first-class browser download lifecycle state, including progress, cancellation, suggested filenames, destination selection, and collision policy.",
        usage: "download <list|status|wait|cancel> [id] [seconds] [destination] [fail|overwrite|uniquify]",
        category: "artifacts",
    },
    ToolSpec {
        name: "downloads",
        description: "List files downloaded into jelly browser download locations. This legacy filesystem view is retained for compatibility; use download for lifecycle state.",
        usage: "downloads",
        category: "artifacts",
    },
    ToolSpec {
        name: "verify-artifact",
        description: "Verify a captured artifact still exists, is nonempty, and has valid format-specific integrity metadata.",
        usage: "verify-artifact <artifact-id|path> [--semantic check...]",
        category: "artifacts",
    },
    ToolSpec {
        name: "wait-download",
        description: "Wait for a completed download newer than a supplied timestamp baseline and register it as an artifact.",
        usage: "wait-download <after-ms> [seconds] [name-contains]",
        category: "artifacts",
    },
    ToolSpec {
        name: "inspect-network",
        description: "Start, stop, or query persistent browser network capture with optional filters.",
        usage: "inspect-network <start|stop|show> [filters]",
        category: "network",
    },
    ToolSpec {
        name: "call-routine",
        description: "Capture action traces, stage deterministic candidates, or save, search, inspect, run and resume JSON routines with auto/hybrid/jelly modes. Capture and save never execute browser operations; use browser-call for a simple one-off batch.",
        usage: "call-routine list | call-routine --capture <start|start-isolated|status [id]|stop id> | call-routine --validate <name> <revision> <fixture-json> | call-routine --versions <name> | call-routine --crystallize <name> <expected-rev> <trace-id> [parameters-json] | call-routine --revise <name> <expected-rev> <graph-json> | call-routine --promote <name> <rev> <expected-rev> | call-routine --search <query> | call-routine --status <resume-id> | call-routine --claim <id> <owner> [seconds] [--takeover <reason>] | call-routine --renew <id> <lease-token> [seconds] | call-routine --release <id> <lease-token> | call-routine describe <name-or-uuid> | call-routine save <graph-json> | call-routine [--handoff-to <profile>] <name-or-uuid> [key=value] | call-routine resume <id> [--lease <token>] [key=value]",
        category: "routines",
    },
    ToolSpec {
        name: "hitl",
        description: "Request a human decision through Telegram when chat is unavailable; attach a browser screenshot or a local MP4 for delivery. This is not browser-event monitoring.",
        usage: "hitl <message> [--video path | --screenshot-target target | --no-screenshot]",
        category: "hitl",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_registry_is_unique_and_categorized() {
        let mut names = TOOLS.iter().map(|tool| tool.name).collect::<Vec<_>>();
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len);
        assert!(TOOLS.iter().all(|tool| !tool.description.trim().is_empty()));
        assert!(
            TOOLS
                .iter()
                .all(|tool| TOOL_CATEGORIES.iter().any(|c| c.name == tool.category))
        );
    }

    #[test]
    fn hitl_tool_uses_telegram_without_exposing_transport_selection() {
        let tool = TOOLS.iter().find(|spec| spec.name == "hitl").unwrap();
        assert_eq!(tool.category, "hitl");
        assert!(tool.usage.starts_with("hitl <message>"));
        assert!(tool.description.contains("Telegram"));
        assert_eq!(tool.schema()["kind"], "system");
        assert!(
            TOOLS
                .iter()
                .all(|spec| spec.name != "send-telegram-message")
        );
    }
}
