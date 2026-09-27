use crate::{BrowserSession, Error, Target};
use serde_json::{Value, json};

pub type PrimitiveHandler = fn(&mut BrowserSession, &[String]) -> Result<String, Error>;

#[derive(Debug, Clone, Copy)]
pub enum ArgKind {
    String,
    Target,
    Integer,
}

#[derive(Debug, Clone, Copy)]
pub struct ArgSpec {
    pub name: &'static str,
    pub kind: ArgKind,
    pub required: bool,
}
impl ArgSpec {
    pub const fn req(name: &'static str, kind: ArgKind) -> Self {
        Self {
            name,
            kind,
            required: true,
        }
    }
    pub const fn opt(name: &'static str, kind: ArgKind) -> Self {
        Self {
            name,
            kind,
            required: false,
        }
    }
    pub const fn type_name(&self) -> &'static str {
        match self.kind {
            ArgKind::String => "string",
            ArgKind::Target => "target",
            ArgKind::Integer => "integer",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CategorySpec {
    pub name: &'static str,
    pub description: &'static str,
}

pub static CATEGORIES: &[CategorySpec] = &[
    CategorySpec {
        name: "input",
        description: "Interact with page controls through clicks, typing, keys, selection, checks, dialogs, and drag-and-drop.",
    },
    CategorySpec {
        name: "navigation",
        description: "Navigate documents, wait for browser conditions, and move through page history.",
    },
    CategorySpec {
        name: "inspect",
        description: "Observe page content, elements, controls, links, images, layout, and accessibility state.",
    },
    CategorySpec {
        name: "tabs",
        description: "List, open, switch, and close browser tabs while preserving the active session.",
    },
    CategorySpec {
        name: "files",
        description: "Move local files into browser-controlled file inputs.",
    },
    CategorySpec {
        name: "script",
        description: "Evaluate or inject JavaScript as an explicit browser-page escape hatch.",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct PrimitiveSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub usage: &'static str,
    pub category: &'static str,
    pub args: &'static [ArgSpec],
    pub max_args: Option<usize>,
    pub handler: PrimitiveHandler,
}

impl PrimitiveSpec {
    pub fn validate(&self, args: &[String]) -> Result<(), Error> {
        let required = self.args.iter().filter(|a| a.required).count();
        if args.len() < required || self.max_args.is_some_and(|max| args.len() > max) {
            return Err(format!("usage: {}", self.usage).into());
        }
        for (value, spec) in args.iter().zip(self.args.iter()) {
            match spec.kind {
                ArgKind::Target => {
                    Target::parse(value)?;
                }
                ArgKind::Integer => {
                    value
                        .parse::<u64>()
                        .map_err(|_| format!("{} must be an integer", spec.name))?;
                }
                ArgKind::String => {}
            }
        }
        Ok(())
    }
    pub fn schema(&self) -> Value {
        json!({"name":self.name,"description":self.description,"usage":self.usage,"category":self.category,"arguments":self.args.iter().map(|a|json!({"name":a.name,"type":a.type_name(),"required":a.required})).collect::<Vec<_>>(),"variadic":self.max_args.is_none()})
    }
}

macro_rules! primitives {
    ($( $name:literal => $handler:path, description:$description:literal, usage:$usage:literal, category:$category:literal, args:$args:expr, max:$max:expr; )+)=>{
        pub static PRIMITIVES:&[PrimitiveSpec]=&[$(PrimitiveSpec{name:$name,description:$description,usage:$usage,category:$category,args:$args,max_args:$max,handler:$handler},)+];
        pub fn lookup(name:&str)->Option<&'static PrimitiveSpec>{PRIMITIVES.iter().find(|spec|spec.name==name)}
    };
}

use ArgKind::{Integer, String as Str, Target as Tgt};
primitives! {
    "click" => crate::primitives::input::click, description:"Click a visible element using a stable ref, CSS selector, or exact visible text.", usage:"click <target>", category:"input", args:&[ArgSpec::req("target",Tgt)], max:Some(1);
    "type-text" => crate::primitives::input::type_text, description:"Type text into a visible editable element, optionally targeting a specific element.", usage:"type-text <text> [target]", category:"input", args:&[ArgSpec::req("text",Str),ArgSpec::opt("target",Tgt)], max:Some(2);
    "fill" => crate::primitives::input::fill, description:"Replace the current contents of a visible editable element and dispatch input/change events.", usage:"fill <text> [target]", category:"input", args:&[ArgSpec::req("text",Str),ArgSpec::opt("target",Tgt)], max:Some(2);
    "press-key" => crate::primitives::input::press_key, description:"Dispatch a keyboard key to the currently focused page element.", usage:"press-key <key>", category:"input", args:&[ArgSpec::req("key",Str)], max:Some(1);
    "select" => crate::primitives::input::select, description:"Select an option in a native select element by value or visible option text.", usage:"select <target> <value>", category:"input", args:&[ArgSpec::req("target",Tgt),ArgSpec::req("value",Str)], max:Some(2);
    "check" => crate::primitives::input::check, description:"Ensure a checkbox or other checkable element is checked.", usage:"check <target>", category:"input", args:&[ArgSpec::req("target",Tgt)], max:Some(1);
    "dialog" => crate::primitives::input::dialog, description:"Accept or dismiss the active JavaScript dialog, optionally supplying prompt text.", usage:"dialog <accept|dismiss> [text]", category:"input", args:&[ArgSpec::req("action",Str),ArgSpec::opt("text",Str)], max:Some(2);
    "drag" => crate::primitives::input::drag, description:"Drag an element to another element or explicit viewport coordinates using pointer events.", usage:"drag <source> <target|x:N,y:N>", category:"input", args:&[ArgSpec::req("source",Tgt),ArgSpec::req("destination",Str)], max:Some(2);

    "wait" => crate::primitives::navigation::wait, description:"Wait until text, CSS, visibility, URL, disappearance, or a JavaScript condition is satisfied.", usage:"wait <text|css|visible|url|gone|js> <value> [seconds]", category:"navigation", args:&[ArgSpec::req("condition",Str),ArgSpec::req("value",Str),ArgSpec::opt("seconds",Integer)], max:Some(3);
    "navigate" => crate::primitives::navigation::navigate, description:"Navigate the active tab to a URL and restore persistent injected scripts.", usage:"navigate <url>", category:"navigation", args:&[ArgSpec::req("url",Str)], max:Some(1);
    "tab-history" => crate::primitives::navigation::tab_history, description:"Move the active tab backward or forward in its navigation history.", usage:"tab-history <back|forward>", category:"navigation", args:&[ArgSpec::req("direction",Str)], max:Some(1);

    "snapshot-interactive" => crate::primitives::inspect::snapshot_interactive, description:"Return visible interactive elements and assign stable @eN references for subsequent actions.", usage:"snapshot-interactive", category:"inspect", args:&[], max:Some(0);
    "read-page" => crate::primitives::inspect::read_page, description:"Return the active page title, URL, headings, and main readable text.", usage:"read-page", category:"inspect", args:&[], max:Some(0);
    "inspect-inputs" => crate::primitives::inspect::inspect_inputs, description:"List visible form controls with type, label, placeholder, name, value, and checked state.", usage:"inspect-inputs", category:"inspect", args:&[], max:Some(0);
    "inspect-elements" => crate::primitives::inspect::inspect_elements, description:"Inspect up to 100 visible elements matching a CSS selector with text, attributes, and geometry.", usage:"inspect-elements [css]", category:"inspect", args:&[ArgSpec::opt("css",Str)], max:Some(1);
    "element-info" => crate::primitives::inspect::element_info, description:"Return detailed content, attributes, state, geometry, and computed style for one target.", usage:"element-info <target>", category:"inspect", args:&[ArgSpec::req("target",Tgt)], max:Some(1);
    "scroll" => crate::primitives::inspect::scroll, description:"Scroll the page by direction or bring a target element into the center of the viewport.", usage:"scroll [down|up|top|bottom|target]", category:"inspect", args:&[ArgSpec::opt("destination",Str)], max:Some(1);
    "query-selector" => crate::primitives::inspect::query_selector, description:"Return matching DOM elements for a CSS selector with text, HTML, visibility, and geometry.", usage:"query-selector <css>", category:"inspect", args:&[ArgSpec::req("css",Str)], max:Some(1);
    "get-element" => crate::primitives::inspect::get_element, description:"Return the text, outer HTML, or both for a single target element.", usage:"get-element <target> [text|html|both]", category:"inspect", args:&[ArgSpec::req("target",Tgt),ArgSpec::opt("mode",Str)], max:Some(2);
    "accessibility-tree" => crate::primitives::inspect::accessibility_tree, description:"Return a compact view of non-ignored nodes from the page accessibility tree.", usage:"accessibility-tree [max]", category:"inspect", args:&[ArgSpec::opt("max",Integer)], max:Some(1);
    "inspect-links" => crate::primitives::inspect::inspect_links, description:"List visible links with normalized visible text and resolved URLs.", usage:"inspect-links", category:"inspect", args:&[], max:Some(0);
    "inspect-images" => crate::primitives::inspect::inspect_images, description:"List visible images with alt text, resolved source URL, and rendered dimensions.", usage:"inspect-images", category:"inspect", args:&[], max:Some(0);

    "tabs" => crate::primitives::tabs::tabs, description:"List browser page targets with target ID, title, and URL.", usage:"tabs", category:"tabs", args:&[], max:Some(0);
    "switch-tab" => crate::primitives::tabs::switch_tab, description:"Activate and attach to a tab by target ID or matching title or URL.", usage:"switch-tab <id|title|url>", category:"tabs", args:&[ArgSpec::req("query",Str)], max:Some(1);
    "close-tab" => crate::primitives::tabs::close_tab, description:"Close the active tab and reattach the session to a remaining page target when available.", usage:"close-tab", category:"tabs", args:&[], max:Some(0);
    "open-in-new-tab" => crate::primitives::tabs::open_in_new_tab, description:"Open the link or image URL represented by a target in a new active tab.", usage:"open-in-new-tab <target>", category:"tabs", args:&[ArgSpec::req("target",Tgt)], max:Some(1);

    "upload" => crate::primitives::files::upload, description:"Set a local file on a targeted HTML file input using the DOM protocol.", usage:"upload <target> <file>", category:"files", args:&[ArgSpec::req("target",Tgt),ArgSpec::req("file",Str)], max:Some(2);
    "evaluate-js" => crate::primitives::script::evaluate_js, description:"Evaluate a JavaScript expression in the active page and return its by-value result.", usage:"evaluate-js <expression>", category:"script", args:&[ArgSpec::req("expression",Str)], max:None;
    "inject-js" => crate::primitives::script::inject_js, description:"Execute a JavaScript body now, optionally persisting it across jelly navigations.", usage:"inject-js [--persistent] [--file path | <script>]", category:"script", args:&[ArgSpec::req("script",Str)], max:None;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_is_unique_and_self_describing() {
        let mut names = PRIMITIVES.iter().map(|p| p.name).collect::<Vec<_>>();
        let len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), len);
        assert!(PRIMITIVES.iter().all(|p| !p.description.trim().is_empty()));
        assert!(
            PRIMITIVES
                .iter()
                .all(|p| CATEGORIES.iter().any(|c| c.name == p.category))
        );
        let click = lookup("click").unwrap();
        assert_eq!(click.category, "input");
        assert_eq!(click.args[0].name, "target");
        assert_eq!(click.schema()["description"], click.description);
    }
    #[test]
    fn generic_validation_checks_arity_and_types() {
        assert!(lookup("click").unwrap().validate(&[]).is_err());
        assert!(
            lookup("click")
                .unwrap()
                .validate(&["css:#ok".into()])
                .is_ok()
        );
        assert!(
            lookup("wait")
                .unwrap()
                .validate(&["text".into(), "x".into(), "nope".into()])
                .is_err()
        );
        assert!(
            lookup("read-page")
                .unwrap()
                .validate(&["extra".into()])
                .is_err()
        );
    }
}
