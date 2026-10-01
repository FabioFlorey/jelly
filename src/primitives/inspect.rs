use crate::primitives::js_helpers::{LAYOUT_VISIBLE_FN, NORMALIZE_TEXT_FN, RENDERED_VISIBLE_FN};
use crate::primitives::{js, missing_target, pretty, target};
use crate::{BrowserSession, Error};
use serde_json::{Value, json};
use std::env;

fn bounded_usize(value: &str, name: &str, max: usize, allow_zero: bool) -> Result<usize, Error> {
    let parsed = value.parse::<usize>().map_err(|_| {
        crate::jelly_error(
            crate::ErrorKind::InvalidArguments,
            format!("{name} must be a non-negative integer"),
            false,
        )
    })?;
    if !allow_zero && parsed == 0 {
        return Err(crate::jelly_error(
            crate::ErrorKind::InvalidArguments,
            format!("{name} must be greater than zero"),
            false,
        ));
    }
    Ok(parsed.min(max))
}

fn page_runtime_enabled() -> bool {
    !env::var("JELLY_PAGE_RUNTIME")
        .ok()
        .is_some_and(|value| matches!(value.trim(), "0" | "false" | "off"))
}

fn eval_page_runtime(b: &mut BrowserSession, expression: &str) -> Result<Value, Error> {
    let value = b.eval(expression)?;
    if !value.is_null() {
        return Ok(value);
    }
    b.eval(crate::browser::PAGE_RUNTIME_BOOTSTRAP)?;
    b.eval(expression)
}

fn dispose_page_runtime(b: &mut BrowserSession) -> Result<(), Error> {
    b.eval(
        "(()=>{const r=globalThis.__jellyRuntimeV1;if(r){r.dispose?.();delete globalThis.__jellyRuntimeV1}return true})()",
    )?;
    Ok(())
}

pub fn snapshot_interactive(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let limit = if let Some(value) = args.first() {
        bounded_usize(value, "snapshot limit", 1000, true)?
    } else if let Ok(value) = env::var("JELLY_SNAPSHOT_LIMIT") {
        bounded_usize(&value, "JELLY_SNAPSHOT_LIMIT", 1000, true)?
    } else {
        0
    };
    let offset = match args.get(1) {
        Some(value) => bounded_usize(value, "snapshot offset", 1_000_000, true)?,
        None => 0,
    };
    if page_runtime_enabled() {
        let expression = crate::browser::snapshot_expression(limit, offset);
        return Ok(pretty(&eval_page_runtime(b, &expression)?));
    }

    dispose_page_runtime(b)?;
    let expression = format!(
        r#"(()=>{{const limit={limit},offset={offset},roles=new Set(['button','link','textbox','checkbox','radio','combobox','option','tab','menuitem','switch','slider','spinbutton','treeitem']);document.querySelectorAll('[data-jelly-ref]').forEach(e=>e.removeAttribute('data-jelly-ref'));const els=[...document.querySelectorAll('a,button,input,textarea,select,[role],[tabindex],[contenteditable=true],[draggable=true],[onclick]')].filter(e=>{{const r=e.getBoundingClientRect(),s=getComputedStyle(e),role=e.getAttribute('role');const interactive=['A','BUTTON','INPUT','TEXTAREA','SELECT'].includes(e.tagName)||roles.has(role)||e.tabIndex>=0||e.isContentEditable||e.draggable||!!e.onclick;return interactive&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&s.opacity!=='0'}});els.forEach((e,i)=>e.setAttribute('data-jelly-ref','e'+(i+1)));const end=limit>0?Math.min(els.length,offset+limit):els.length,out=[];for(let i=Math.min(offset,els.length);i<end;i++){{const e=els[i],ref='e'+(i+1),r=e.getBoundingClientRect(),role=e.getAttribute('role')||({{A:'link',BUTTON:'button',INPUT:(e.type==='checkbox'?'checkbox':e.type==='radio'?'radio':'textbox'),TEXTAREA:'textbox',SELECT:'combobox'}})[e.tagName]||(e.draggable?'draggable':'');const name=(e.getAttribute('aria-label')||e.innerText||e.value||e.placeholder||e.alt||'').replace(/\s+/g,' ').trim().slice(0,160);out.push({{ref:'@'+ref,tag:e.tagName.toLowerCase(),role,name,disabled:!!e.disabled,checked:e.checked??null,draggable:!!e.draggable,x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}})}}return out}})()"#,
        limit = limit,
        offset = offset
    );
    Ok(pretty(&b.eval(&expression)?))
}
pub fn find_interactive(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let query = args.first().ok_or_else(|| {
        crate::jelly_error(
            crate::ErrorKind::InvalidArguments,
            "usage: find-interactive <query> [limit] [offset]",
            false,
        )
    })?;
    let limit = match args.get(1) {
        Some(value) => bounded_usize(value, "search limit", 100, false)?,
        None => 30,
    };
    let offset = match args.get(2) {
        Some(value) => bounded_usize(value, "search offset", 1_000_000, true)?,
        None => 0,
    };
    if page_runtime_enabled() {
        let expression = crate::browser::search_expression(query, limit, offset);
        return Ok(pretty(&eval_page_runtime(b, &expression)?));
    }

    dispose_page_runtime(b)?;
    let expression = format!(
        r#"(()=>{{const q={query},max={limit},offset={offset},roles=new Set(['button','link','textbox','checkbox','radio','combobox','option','tab','menuitem','switch','slider','spinbutton','treeitem']),selector='a,button,input,textarea,select,[role],[tabindex],[contenteditable=true],[draggable=true],[onclick]',norm=v=>(v||'').replace(/\s+/g,' ').trim(),visible=e=>{{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&s.opacity!=='0'}},interactive=e=>{{const role=e.getAttribute('role');return ['A','BUTTON','INPUT','TEXTAREA','SELECT'].includes(e.tagName)||roles.has(role)||e.tabIndex>=0||e.isContentEditable||e.draggable||!!e.onclick}};document.querySelectorAll('[data-jelly-ref]').forEach(e=>e.removeAttribute('data-jelly-ref'));const all=[...document.querySelectorAll(selector)].filter(interactive);all.forEach((e,i)=>e.setAttribute('data-jelly-ref','e'+(i+1)));const rows=[];for(let i=0;i<all.length;i++){{const e=all[i];if(!visible(e))continue;const name=norm(e.getAttribute('aria-label')||e.textContent||e.value||e.placeholder||e.alt||e.getAttribute('title')||''),n=name.toLowerCase(),needle=norm(q).toLowerCase();let match=99;if(n===needle)match=0;else if(n.startsWith(needle))match=1;else if(n.includes(needle))match=2;else continue;const r=e.getBoundingClientRect(),role=e.getAttribute('role')||({{A:'link',BUTTON:'button',INPUT:(e.type==='checkbox'?'checkbox':e.type==='radio'?'radio':'textbox'),TEXTAREA:'textbox',SELECT:'combobox'}})[e.tagName]||(e.draggable?'draggable':'');const inViewport=r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth;rows.push({{match,disabled:e.disabled?1:0,offscreen:inViewport?0:1,order:i,value:{{ref:'@e'+(i+1),tag:e.tagName.toLowerCase(),role,name:name.slice(0,160),disabled:!!e.disabled,checked:e.checked??null,draggable:!!e.draggable,shadow:false,in_viewport:inViewport,x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}}}})}}rows.sort((a,b)=>a.match-b.match||a.disabled-b.disabled||a.offscreen-b.offscreen||a.order-b.order);return rows.slice(offset,offset+max).map(x=>x.value)}})()"#,
        query = js(query),
        limit = limit,
        offset = offset
    );
    Ok(pretty(&b.eval(&expression)?))
}

pub fn read_page(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(
        r#"(() => {
            const root =
                document.querySelector('main,article,[role="main"]') ||
                document.body ||
                document.documentElement;
            const headings = [...document.querySelectorAll('h1,h2,h3')]
                .filter(element => element.offsetParent)
                .map(element => (element.innerText || '').trim())
                .filter(Boolean)
                .slice(0, 30);
            return {
                title:document.title || '',
                url:location.href,
                headings,
                text:(root?.innerText || '').replace(/\n{3,}/g, '\n\n').slice(0, 12000)
            };
        })()"#,
    )?))
}

pub fn inspect_inputs(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(
        r#"(() => [...document.querySelectorAll(
            'input,textarea,select,[contenteditable=true],[role=textbox],[role=combobox],[role=checkbox],[role=radio]'
        )]
            .filter(element => element.offsetParent)
            .map((element, index) => ({
                n:index + 1,
                tag:element.tagName.toLowerCase(),
                type:element.type || element.getAttribute('role') || '',
                label:element.getAttribute('aria-label') || element.labels?.[0]?.innerText || '',
                placeholder:element.placeholder || '',
                name:element.name || '',
                value:element.value || '',
                checked:element.checked ?? null
            })))()"#,
    )?))
}

pub fn inspect_elements(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let q = args.first().map(String::as_str).unwrap_or("*");
    Ok(pretty(&b.eval(&format!(
        r#"(() => [...document.querySelectorAll({})]
            .filter(element => element.offsetParent)
            .slice(0, 100)
            .map((element, index) => {{
                const rect = element.getBoundingClientRect();
                return {{
                    n:index + 1,
                    tag:element.tagName.toLowerCase(),
                    text:(element.innerText || '').trim().slice(0, 200),
                    role:element.getAttribute('role'),
                    label:element.getAttribute('aria-label'),
                    id:element.id,
                    class:element.className?.toString().slice(0, 120),
                    href:element.href || null,
                    src:element.currentSrc || element.src || null,
                    x:Math.round(rect.x),
                    y:Math.round(rect.y),
                    width:Math.round(rect.width),
                    height:Math.round(rect.height)
                }};
            }}))()"#,
        js(q)
    ))?))
}

pub fn element_info(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: element-info <target>")?;
    let v = b.eval(&format!(
        r#"(() => {{
            const element = {};
            if (!element) return null;

            const visible = {RENDERED_VISIBLE_FN};
            const rect = element.getBoundingClientRect();
            const style = getComputedStyle(element);
            const image = element instanceof HTMLImageElement
                ? {{
                    complete:element.complete,
                    natural_width:element.naturalWidth,
                    natural_height:element.naturalHeight
                }}
                : null;

            return {{
                tag:element.tagName.toLowerCase(),
                text:(element.innerText || element.textContent || '').trim(),
                role:element.getAttribute('role'),
                label:element.getAttribute('aria-label'),
                href:element.href || null,
                src:element.currentSrc || element.src || null,
                value:element.value ?? null,
                checked:element.checked ?? null,
                disabled:element.disabled ?? null,
                visible:visible(element),
                rect:{{x:rect.x,y:rect.y,width:rect.width,height:rect.height}},
                image,
                style:{{
                    color:style.color,
                    backgroundColor:style.backgroundColor,
                    borderColor:style.borderColor,
                    font:style.font,
                    display:style.display,
                    visibility:style.visibility,
                    opacity:style.opacity
                }}
            }};
        }})()"#,
        t.js_resolver()
    ))?;
    if v.is_null() {
        return Err(missing_target(&t));
    }
    Ok(pretty(&v))
}

pub fn scroll(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let raw = args.first().map(String::as_str).unwrap_or("down");
    let mut resolved_target = None;
    let e = match raw {
        "down" => "scrollBy(0,innerHeight*.8); 'down'".into(),
        "up" => "scrollBy(0,-innerHeight*.8); 'up'".into(),
        "top" => "scrollTo(0,0); 'top'".into(),
        "bottom" => "scrollTo(0,document.body.scrollHeight); 'bottom'".into(),
        _ => {
            let t = crate::Target::parse(raw)?;
            let resolver = t.js_scroll_resolver();
            resolved_target = Some(t);
            format!(
                r#"(() => {{
                    const element = {resolver};
                    if (!element) return null;
                    element.scrollIntoView({{block:'center'}});
                    return 'ok';
                }})()"#
            )
        }
    };
    let v = b.eval(&e)?;
    if v.is_null() {
        if let Some(target) = resolved_target.as_ref() {
            return Err(missing_target(target));
        }
        return Err(crate::jelly_error(
            crate::ErrorKind::InteractionFailed,
            "scroll failed",
            true,
        ));
    }
    Ok(v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string()))
}

pub fn query_selector(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let q = args.first().ok_or_else(|| {
        crate::jelly_error(
            crate::ErrorKind::InvalidArguments,
            "usage: query-selector <css>",
            false,
        )
    })?;
    Ok(pretty(&b.eval(&format!(
        r#"(() => {{
            const visible = {LAYOUT_VISIBLE_FN};
            const normalizeText = {NORMALIZE_TEXT_FN};
            return [...document.querySelectorAll({})].map((element, index) => {{
                const rect = element.getBoundingClientRect();
                return {{
                    n:index + 1,
                    tag:element.tagName.toLowerCase(),
                    text:normalizeText(element.innerText || element.textContent || '').slice(0, 300),
                    html:element.outerHTML.slice(0, 1000),
                    visible:visible(element),
                    x:rect.x,
                    y:rect.y,
                    width:rect.width,
                    height:rect.height
                }};
            }});
        }})()"#,
        js(q)
    ))?))
}

pub fn get_element(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.is_empty() {
        return Err(crate::jelly_error(
            crate::ErrorKind::InvalidArguments,
            "usage: get-element <target> [text|html|both]",
            false,
        ));
    }
    let t = target(args, 0, "usage: get-element <target> [text|html|both]")?;
    let mode = args.get(1).map(String::as_str).unwrap_or("both");
    let v = b.eval(&format!(
        r#"(() => {{
            const element = {};
            if (!element) return null;
            return {{
                text:(element.innerText || element.textContent || '').trim(),
                html:element.outerHTML
            }};
        }})()"#,
        t.js_resolver()
    ))?;
    if v.is_null() {
        return Err(missing_target(&t));
    }
    Ok(match mode {
        "text" => v["text"].as_str().unwrap_or("").into(),
        "html" => v["html"].as_str().unwrap_or("").into(),
        "both" => pretty(&v),
        _ => {
            return Err(crate::jelly_error(
                crate::ErrorKind::InvalidArguments,
                "mode must be text|html|both",
                false,
            ));
        }
    })
}
pub fn accessibility_tree(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let max = args.first().and_then(|x| x.parse().ok()).unwrap_or(200);
    b.call("Accessibility.enable", json!({}))?;
    let v = b.call("Accessibility.getFullAXTree", json!({}))?;
    let mut out = Vec::new();
    if let Some(nodes) = v["result"]["nodes"].as_array() {
        for n in nodes
            .iter()
            .filter(|n| !n["ignored"].as_bool().unwrap_or(false))
            .take(max)
        {
            let role = n["role"]["value"].as_str().unwrap_or("");
            let name = n["name"]["value"].as_str().unwrap_or("");
            if !role.is_empty() || !name.is_empty() {
                out.push(json!({"nodeId":n["nodeId"],"role":role,"name":name,"value":n["value"]["value"],"description":n["description"]["value"]}))
            }
        }
    }
    Ok(pretty(&Value::Array(out)))
}
pub fn inspect_links(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let v = b.eval(&format!(
        r#"(() => {{
            const visible = {LAYOUT_VISIBLE_FN};
            const normalizeText = {NORMALIZE_TEXT_FN};
            return [...document.querySelectorAll('a[href]')]
                .filter(link => visible(link))
                .map(link => ({{
                    text:normalizeText(link.innerText || link.getAttribute('aria-label') || ''),
                    href:link.href
                }}))
                .filter(link => link.text || link.href);
        }})()"#
    ))?;
    let mut out = Vec::new();
    let links = inspected_links(&v)?;
    for (i, x) in links.iter().enumerate() {
        out.push(format!(
            "{}. {} — {}",
            i + 1,
            x["text"].as_str().unwrap_or(""),
            x["href"].as_str().unwrap_or("")
        ))
    }
    Ok(out.join("\n"))
}

fn inspected_links(value: &Value) -> Result<&[Value], Error> {
    value.as_array().map(Vec::as_slice).ok_or_else(|| {
        crate::jelly_error(
            crate::ErrorKind::Internal,
            "inspect-links browser script returned a non-array result",
            false,
        )
    })
}

pub fn inspect_images(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let v = b.eval(&format!(
        r#"(() => {{
            const visible = {RENDERED_VISIBLE_FN};
            const normalizeText = {NORMALIZE_TEXT_FN};
            return [...document.images]
                .map((image, index) => {{
                    const ref = 'img' + (index + 1);
                    image.setAttribute('data-jelly-ref', ref);
                    const rect = image.getBoundingClientRect();
                    return {{
                        ref:'@' + ref,
                        n:index + 1,
                        alt:normalizeText(image.alt || image.getAttribute('aria-label') || ''),
                        src:image.currentSrc || image.src || '',
                        visible:visible(image),
                        complete:image.complete,
                        natural_width:image.naturalWidth,
                        natural_height:image.naturalHeight,
                        width:Math.round(rect.width),
                        height:Math.round(rect.height)
                    }};
                }})
                .filter(image => image.src);
        }})()"#
    ))?;
    Ok(pretty(&v))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_usize_enforces_limit_contracts() {
        assert_eq!(bounded_usize("0", "limit", 100, true).unwrap(), 0);
        assert!(bounded_usize("0", "limit", 100, false).is_err());
        assert!(bounded_usize("-1", "limit", 100, true).is_err());
        assert!(bounded_usize("nope", "limit", 100, true).is_err());
        assert_eq!(bounded_usize("250", "limit", 100, true).unwrap(), 100);
        assert_eq!(bounded_usize("42", "limit", 100, false).unwrap(), 42);
    }

    #[test]
    fn malformed_link_script_result_is_an_explicit_internal_error() {
        let error = inspected_links(&serde_json::json!({"unexpected":true})).unwrap_err();
        assert_eq!(
            crate::classify_error(error.as_ref()),
            (crate::ErrorKind::Internal, false)
        );
    }
}
