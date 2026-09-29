use crate::primitives::{missing_target, pretty, target};
use crate::{BrowserSession, Error, ErrorKind, jelly_error};

const HIGHLIGHT_KEY: &str = "__jellyHighlight";

pub fn highlight(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(args, 0, "usage: highlight <target> [label]")?;
    let label = args.get(1).map(String::as_str).unwrap_or("");
    let resolver = target.js_resolver();
    let label = serde_json::to_string(label).expect("label serialization");

    let value = b.eval(&format!(
        r##"(() => {{
            const target = {resolver};
            if (!target) return null;

            const rect = target.getBoundingClientRect();
            const computed = getComputedStyle(target);
            const visible = rect.width > 0
                && rect.height > 0
                && computed.display !== "none"
                && computed.visibility !== "hidden";
            if (!visible) {{
                return {{ visible: false }};
            }}

            const existing = window.{key};
            if (existing && typeof existing.cleanup === "function") {{
                existing.cleanup();
            }}

            const overlay = document.createElement("div");
            overlay.setAttribute("data-jelly-highlight", "true");

            const set = (node, name, value) => node.style.setProperty(name, value, "important");
            set(overlay, "position", "fixed");
            set(overlay, "pointer-events", "none");
            set(overlay, "z-index", "2147483647");
            set(overlay, "box-sizing", "border-box");
            set(overlay, "border", "2px solid #FFC107");
            set(overlay, "border-radius", "5px");
            set(overlay, "background", "rgba(255, 193, 7, 0.035)");
            set(overlay, "box-shadow", "0 0 0 1px rgba(255, 193, 7, 0.20), 0 0 10px rgba(255, 193, 7, 0.22)");
            set(overlay, "margin", "0");
            set(overlay, "padding", "0");
            set(overlay, "transition", "none");

            const labelText = {label};
            if (labelText) {{
                const badge = document.createElement("div");
                badge.textContent = labelText;
                set(badge, "position", "absolute");
                set(badge, "left", "0");
                set(badge, "bottom", "calc(100% + 7px)");
                set(badge, "max-width", "min(420px, 80vw)");
                set(badge, "padding", "4px 7px");
                set(badge, "border-radius", "4px");
                set(badge, "background", "rgba(13, 13, 11, 0.82)");
                set(badge, "color", "#FFC107");
                set(badge, "font", "600 12px/1.25 system-ui, sans-serif");
                set(badge, "white-space", "nowrap");
                set(badge, "overflow", "hidden");
                set(badge, "text-overflow", "ellipsis");
                set(badge, "box-shadow", "0 1px 4px rgba(0, 0, 0, 0.22)");
                overlay.appendChild(badge);
            }}

            let frame = 0;
            const cleanup = () => {{
                if (frame) cancelAnimationFrame(frame);
                overlay.remove();
                if (window.{key} && window.{key}.overlay === overlay) {{
                    delete window.{key};
                }}
            }};
            const update = () => {{
                if (!target.isConnected) {{
                    cleanup();
                    return;
                }}
                const next = target.getBoundingClientRect();
                const style = getComputedStyle(target);
                if (
                    next.width <= 0
                    || next.height <= 0
                    || style.display === "none"
                    || style.visibility === "hidden"
                ) {{
                    set(overlay, "display", "none");
                }} else {{
                    set(overlay, "display", "block");
                    set(overlay, "left", `${{next.left - 3}}px`);
                    set(overlay, "top", `${{next.top - 3}}px`);
                    set(overlay, "width", `${{next.width + 6}}px`);
                    set(overlay, "height", `${{next.height + 6}}px`);
                }}
                frame = requestAnimationFrame(update);
            }};

            (document.documentElement || document.body).appendChild(overlay);
            window.{key} = {{ overlay, target, cleanup }};
            update();

            return {{
                highlighted: true,
                visible: true,
                label: labelText || null,
                geometry: {{
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height
                }}
            }};
        }})()"##,
        resolver = resolver,
        label = label,
        key = HIGHLIGHT_KEY,
    ))?;

    if value.is_null() {
        return Err(missing_target(&target));
    }
    if value["visible"] == false {
        return Err(jelly_error(
            ErrorKind::TargetNotVisible,
            "target is not visibly rendered",
            true,
        ));
    }

    Ok(pretty(&value))
}

pub fn clear_highlight(b: &mut BrowserSession, _args: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(&format!(
        r#"(() => {{
            const current = window.{key};
            if (!current || typeof current.cleanup !== "function") {{
                document.querySelectorAll("[data-jelly-highlight='true']").forEach(node => node.remove());
                delete window.{key};
                return {{ cleared: false }};
            }}
            current.cleanup();
            return {{ cleared: true }};
        }})()"#,
        key = HIGHLIGHT_KEY,
    ))?))
}
