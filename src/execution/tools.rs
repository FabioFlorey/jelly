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
        description: "Capture screenshots and inspect files downloaded by the browser.",
    },
    ToolCategorySpec {
        name: "network",
        description: "Capture and inspect browser network traffic.",
    },
    ToolCategorySpec {
        name: "routines",
        description: "Run and resume reusable multi-step workflows.",
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
        name: "screenshot",
        description: "Capture the active Chromium viewport or one targeted element to an image file.",
        usage: "screenshot [target] [output] [--output path]",
        category: "artifacts",
    },
    ToolSpec {
        name: "downloads",
        description: "List files downloaded into jelly browser download locations.",
        usage: "downloads",
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
        description: "Execute a named routine or resume a suspended routine continuation.",
        usage: "call-routine <name> [key=value] | call-routine resume <id> [key=value]",
        category: "routines",
    },
    ToolSpec {
        name: "hitl",
        description: "Request human intervention through Telegram and suspend the surrounding workflow for later continuation.",
        usage: "hitl <message>",
        category: "hitl",
    },
];

pub fn lookup_tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|spec| spec.name == name)
}

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
        let tool = lookup_tool("hitl").unwrap();
        assert_eq!(tool.category, "hitl");
        assert_eq!(tool.usage, "hitl <message>");
        assert!(tool.description.contains("Telegram"));
        assert_eq!(tool.schema()["kind"], "system");
        assert!(lookup_tool("send-telegram-message").is_none());
    }
}
