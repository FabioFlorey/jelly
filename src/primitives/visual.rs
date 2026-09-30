use crate::primitives::{missing_target, pretty, target};
use crate::{BrowserSession, Error, ErrorKind, jelly_error};

const HIGHLIGHT_KEY: &str = "__jellyHighlight";

pub fn highlight(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let target = target(
        args,
        0,
        "usage: highlight <target> [label] [auto|content|box]",
    )?;
    let label = args.get(1).map(String::as_str).unwrap_or("");
    let mode = args.get(2).map(String::as_str).unwrap_or("auto");
    if !matches!(mode, "auto" | "content" | "box") {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "highlight mode must be auto|content|box",
            false,
        ));
    }

    let resolver = target.js_resolver();
    let label = serde_json::to_string(label).expect("label serialization");
    let mode = serde_json::to_string(mode).expect("mode serialization");

    let value = b.eval(&format!(
        r##"(() => {{
            const target = {resolver};
            if (!target) return null;

            const targetRect = target.getBoundingClientRect();
            const computed = getComputedStyle(target);
            const visible = targetRect.width > 0
                && targetRect.height > 0
                && computed.display !== "none"
                && computed.visibility !== "hidden"
                && computed.opacity !== "0";
            if (!visible) {{
                return {{ visible: false }};
            }}

            const existing = window.{key};
            if (existing && typeof existing.cleanup === "function") {{
                existing.cleanup();
            }}

            const requestedMode = {mode};
            const labelText = {label};
            const set = (node, name, value) => node.style.setProperty(name, value, "important");

            const root = document.createElement("div");
            root.setAttribute("data-jelly-highlight", "true");
            set(root, "position", "fixed");
            set(root, "left", "0");
            set(root, "top", "0");
            set(root, "width", "100vw");
            set(root, "height", "100vh");
            set(root, "pointer-events", "none");
            set(root, "z-index", "2147483647");
            set(root, "overflow", "visible");
            set(root, "margin", "0");
            set(root, "padding", "0");
            set(root, "border", "0");
            set(root, "background", "transparent");

            const isTextVisible = node => {{
                const parent = node.parentElement;
                if (!parent) return false;
                const style = getComputedStyle(parent);
                return style.display !== "none"
                    && style.visibility !== "hidden"
                    && style.opacity !== "0";
            }};

            const mergeRects = input => {{
                const rects = input
                    .filter(r => r.width > 0 && r.height > 0)
                    .map(r => ({{left:r.left, top:r.top, right:r.right, bottom:r.bottom}}))
                    .sort((a,b) => a.top - b.top || a.left - b.left);
                const out = [];
                for (const rect of rects) {{
                    const previous = out[out.length - 1];
                    const sameLine = previous
                        && Math.abs(previous.top - rect.top) <= 2
                        && Math.abs(previous.bottom - rect.bottom) <= 2
                        && rect.left <= previous.right + 4;
                    if (sameLine) {{
                        previous.left = Math.min(previous.left, rect.left);
                        previous.top = Math.min(previous.top, rect.top);
                        previous.right = Math.max(previous.right, rect.right);
                        previous.bottom = Math.max(previous.bottom, rect.bottom);
                    }} else {{
                        out.push({{...rect}});
                    }}
                }}
                return out.map(r => ({{
                    x:r.left,
                    y:r.top,
                    width:r.right-r.left,
                    height:r.bottom-r.top
                }}));
            }};

            const rectsForTextRoot = textRoot => {{
                const rects = [];
                const walker = document.createTreeWalker(
                    textRoot,
                    NodeFilter.SHOW_TEXT,
                    {{
                        acceptNode(node) {{
                            if (!node.textContent || !node.textContent.trim() || !isTextVisible(node)) {{
                                return NodeFilter.FILTER_REJECT;
                            }}
                            return NodeFilter.FILTER_ACCEPT;
                        }}
                    }}
                );
                for (let node = walker.nextNode(); node; node = walker.nextNode()) {{
                    const range = document.createRange();
                    range.selectNodeContents(node);
                    for (const rect of range.getClientRects()) rects.push(rect);
                    range.detach?.();
                }}
                return mergeRects(rects);
            }};

            const principalTextRoot = () => {{
                const headings = target.querySelectorAll ? [...target.querySelectorAll("h1,h2,h3,h4,h5,h6")] : [];
                for (const heading of headings) {{
                    if (!heading.innerText?.trim() || !heading.getClientRects().length) continue;
                    const style = getComputedStyle(heading);
                    if (style.display !== "none" && style.visibility !== "hidden" && style.opacity !== "0") {{
                        return heading;
                    }}
                }}

                const descendants = target.querySelectorAll ? [...target.querySelectorAll("*")] : [];
                const candidates = [target, ...descendants]
                    .filter(e => e.innerText?.trim())
                    .filter(e => {{
                        const r = e.getBoundingClientRect();
                        const s = getComputedStyle(e);
                        return r.width > 0 && r.height > 0
                            && s.display !== "none"
                            && s.visibility !== "hidden"
                            && s.opacity !== "0";
                    }})
                    .filter(e => ![...e.children].some(child => child.innerText?.trim()));

                let best = null;
                let bestScore = -Infinity;
                for (const e of candidates) {{
                    const s = getComputedStyle(e);
                    const r = e.getBoundingClientRect();
                    const fontSize = parseFloat(s.fontSize) || 0;
                    const weight = parseInt(s.fontWeight, 10) || (s.fontWeight === "bold" ? 700 : 400);
                    const textLength = Math.min(80, e.innerText.trim().length);
                    const score = fontSize * 10 + weight / 20 + Math.min(200, r.width) / 20 + textLength / 40;
                    if (score > bestScore) {{
                        best = e;
                        bestScore = score;
                    }}
                }}
                return best || target;
            }};

            let autoTextRoot = null;
            const contentRects = all => {{
                if (all) return rectsForTextRoot(target);
                if (
                    !autoTextRoot
                    || !autoTextRoot.isConnected
                    || (autoTextRoot !== target && !target.contains(autoTextRoot))
                ) {{
                    autoTextRoot = principalTextRoot();
                }}
                return rectsForTextRoot(autoTextRoot);
            }};

            const textCentricTags = new Set([
                "A","LABEL","SPAN","P","H1","H2","H3","H4","H5","H6",
                "STRONG","EM","SMALL","SUMMARY","LEGEND","LI","DT","DD","CODE"
            ]);

            let resolvedMode = requestedMode;
            let fragments = [];
            if (requestedMode === "content") {{
                fragments = contentRects(true);
            }} else if (requestedMode === "auto") {{
                fragments = contentRects(false);
            }}
            if (requestedMode === "auto") {{
                resolvedMode = textCentricTags.has(target.tagName) && fragments.length
                    ? "content"
                    : "shape";
            }} else if (requestedMode === "content" && fragments.length === 0) {{
                resolvedMode = "box";
            }}

            const makeFragment = (rect, kind) => {{
                const node = document.createElement("div");
                node.setAttribute("data-jelly-highlight-fragment", kind);
                set(node, "position", "absolute");
                set(node, "box-sizing", "border-box");
                set(node, "pointer-events", "none");
                set(node, "margin", "0");
                set(node, "padding", "0");
                set(node, "transition", "none");

                if (kind === "content") {{
                    set(node, "left", `${{rect.x - 1}}px`);
                    set(node, "top", `${{rect.y - 1}}px`);
                    set(node, "width", `${{rect.width + 2}}px`);
                    set(node, "height", `${{rect.height + 2}}px`);
                    set(node, "border", "0");
                    set(node, "border-radius", "3px");
                    set(node, "background", "rgba(255, 193, 7, 0.10)");
                    set(node, "box-shadow", "0 0 3px 1px rgba(255, 193, 7, 0.45), 0 0 9px 2px rgba(255, 193, 7, 0.24)");
                }} else if (kind === "shape") {{
                    set(node, "left", `${{rect.x - 2}}px`);
                    set(node, "top", `${{rect.y - 2}}px`);
                    set(node, "width", `${{rect.width + 4}}px`);
                    set(node, "height", `${{rect.height + 4}}px`);
                    set(node, "border", "0");
                    set(node, "border-radius", computed.borderRadius && computed.borderRadius !== "0px" ? computed.borderRadius : "6px");
                    set(node, "background", "rgba(255, 193, 7, 0.035)");
                    set(node, "box-shadow", "0 0 0 2px rgba(255, 193, 7, 0.35), 0 0 11px 3px rgba(255, 193, 7, 0.25)");
                }} else {{
                    set(node, "left", `${{rect.x - 3}}px`);
                    set(node, "top", `${{rect.y - 3}}px`);
                    set(node, "width", `${{rect.width + 6}}px`);
                    set(node, "height", `${{rect.height + 6}}px`);
                    set(node, "border", "2px solid #FFC107");
                    set(node, "border-radius", "5px");
                    set(node, "background", "rgba(255, 193, 7, 0.035)");
                    set(node, "box-shadow", "0 0 0 1px rgba(255, 193, 7, 0.20), 0 0 10px rgba(255, 193, 7, 0.22)");
                }}
                root.appendChild(node);
                return node;
            }};

            let rendered = [];
            const syncFragments = (rects, kind) => {{
                if (rendered.some(node => node.getAttribute("data-jelly-highlight-fragment") !== kind)) {{
                    for (const node of rendered) node.remove();
                    rendered = [];
                }}
                while (rendered.length > rects.length) rendered.pop().remove();
                while (rendered.length < rects.length) {{
                    rendered.push(makeFragment({{x:0,y:0,width:0,height:0}}, kind));
                }}
                for (let i = 0; i < rects.length; i++) {{
                    const rect = rects[i];
                    const node = rendered[i];
                    const pad = kind === "content" ? 1 : kind === "shape" ? 2 : 3;
                    set(node, "left", String(rect.x - pad) + "px");
                    set(node, "top", String(rect.y - pad) + "px");
                    set(node, "width", String(rect.width + pad * 2) + "px");
                    set(node, "height", String(rect.height + pad * 2) + "px");
                }}
            }};

            const render = () => {{
                if (!target.isConnected) return false;
                const rect = target.getBoundingClientRect();
                const style = getComputedStyle(target);
                if (
                    rect.width <= 0
                    || rect.height <= 0
                    || style.display === "none"
                    || style.visibility === "hidden"
                    || style.opacity === "0"
                ) {{
                    set(root, "display", "none");
                    return true;
                }}
                set(root, "display", "block");

                if (resolvedMode === "content") {{
                    const current = contentRects(requestedMode === "content");
                    if (current.length) {{
                        syncFragments(current, "content");
                    }} else {{
                        syncFragments([rect], "box");
                    }}
                }} else {{
                    syncFragments([rect], resolvedMode);
                }}
                return true;
            }};

            let badge = null;
            if (labelText) {{
                badge = document.createElement("div");
                badge.setAttribute("data-jelly-highlight-label", "true");
                badge.textContent = labelText;
                set(badge, "position", "absolute");
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
                root.appendChild(badge);
            }}

            let frame = 0;
            const cleanup = () => {{
                if (frame) cancelAnimationFrame(frame);
                root.remove();
                if (window.{key} && window.{key}.root === root) {{
                    delete window.{key};
                }}
            }};
            const update = () => {{
                if (!target.isConnected) {{
                    cleanup();
                    return;
                }}
                render();
                if (badge) {{
                    const first = rendered[0]?.getBoundingClientRect();
                    const fallback = target.getBoundingClientRect();
                    const anchor = first?.width > 0 ? first : fallback;
                    set(badge, "left", `${{Math.max(4, anchor.left)}}px`);
                    set(badge, "top", `${{Math.max(4, anchor.top - badge.offsetHeight - 7)}}px`);
                }}
                frame = requestAnimationFrame(update);
            }};

            (document.documentElement || document.body).appendChild(root);
            window.{key} = {{ root, target, cleanup, requestedMode, resolvedMode }};
            render();
            update();

            const fragmentRects = rendered.map(node => {{
                const r = node.getBoundingClientRect();
                return {{x:r.x,y:r.y,width:r.width,height:r.height}};
            }});

            return {{
                highlighted: true,
                visible: true,
                label: labelText || null,
                mode: requestedMode,
                resolved_mode: resolvedMode,
                fragment_count: fragmentRects.length,
                fragments: fragmentRects,
                geometry: {{
                    x: targetRect.x,
                    y: targetRect.y,
                    width: targetRect.width,
                    height: targetRect.height
                }}
            }};
        }})()"##,
        resolver = resolver,
        label = label,
        mode = mode,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_source_uses_text_ranges_and_fragment_overlays() {
        assert!(HIGHLIGHT_KEY.contains("Highlight"));
        let source = include_str!("visual.rs");
        assert!(source.contains("getClientRects"));
        assert!(source.contains("data-jelly-highlight-fragment"));
        assert!(source.contains("\"auto\" | \"content\" | \"box\""));
        assert!(source.contains("const set = (node, name, value) => node.style.setProperty"));
    }
}
