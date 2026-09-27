use crate::primitives::{js, pretty, target};
use crate::{BrowserSession, Error};
use serde_json::{Value, json};

pub fn snapshot_interactive(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(r#"(()=>{const roles=new Set(['button','link','textbox','checkbox','radio','combobox','option','tab','menuitem','switch','slider','spinbutton','treeitem']);document.querySelectorAll('[data-pbj-ref]').forEach(e=>e.removeAttribute('data-pbj-ref'));const els=[...document.querySelectorAll('a,button,input,textarea,select,[role],[tabindex],[contenteditable=true],[draggable=true],[onclick]')].filter(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e),role=e.getAttribute('role');const interactive=['A','BUTTON','INPUT','TEXTAREA','SELECT'].includes(e.tagName)||roles.has(role)||e.tabIndex>=0||e.isContentEditable||e.draggable||!!e.onclick;return interactive&&r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'});return els.map((e,i)=>{const ref='e'+(i+1);e.setAttribute('data-pbj-ref',ref);const r=e.getBoundingClientRect();const role=e.getAttribute('role')||({A:'link',BUTTON:'button',INPUT:(e.type==='checkbox'?'checkbox':e.type==='radio'?'radio':'textbox'),TEXTAREA:'textbox',SELECT:'combobox'})[e.tagName]||(e.draggable?'draggable':'');const name=(e.getAttribute('aria-label')||e.innerText||e.value||e.placeholder||e.alt||'').replace(/\s+/g,' ').trim().slice(0,160);return {ref:'@'+ref,tag:e.tagName.toLowerCase(),role,name,disabled:!!e.disabled,checked:e.checked??null,draggable:!!e.draggable,x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}})})()"#)?))
}
pub fn read_page(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(r#"(()=>{const root=document.querySelector('main,article,[role="main"]')||document.body||document.documentElement;return {title:document.title||'',url:location.href,headings:[...document.querySelectorAll('h1,h2,h3')].filter(e=>e.offsetParent).map(e=>(e.innerText||'').trim()).filter(Boolean).slice(0,30),text:(root?.innerText||'').replace(/\n{3,}/g,'\n\n').slice(0,12000)}})()"#)?))
}
pub fn inspect_inputs(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    Ok(pretty(&b.eval(r#"(()=>[...document.querySelectorAll('input,textarea,select,[contenteditable=true],[role=textbox],[role=combobox],[role=checkbox],[role=radio]')].filter(e=>e.offsetParent).map((e,i)=>({n:i+1,tag:e.tagName.toLowerCase(),type:e.type||e.getAttribute('role')||'',label:e.getAttribute('aria-label')||e.labels?.[0]?.innerText||'',placeholder:e.placeholder||'',name:e.name||'',value:e.value||'',checked:e.checked??null})))()"#)?))
}
pub fn inspect_elements(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let q = args.first().map(String::as_str).unwrap_or("*");
    Ok(pretty(&b.eval(&format!(r#"(()=>[...document.querySelectorAll({})].filter(e=>e.offsetParent).slice(0,100).map((e,i)=>{{const r=e.getBoundingClientRect();return {{n:i+1,tag:e.tagName.toLowerCase(),text:(e.innerText||'').trim().slice(0,200),role:e.getAttribute('role'),label:e.getAttribute('aria-label'),id:e.id,class:e.className?.toString().slice(0,120),href:e.href||null,src:e.currentSrc||e.src||null,x:Math.round(r.x),y:Math.round(r.y),width:Math.round(r.width),height:Math.round(r.height)}}}}))()"#,js(q)))?))
}
pub fn element_info(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = target(args, 0, "usage: element-info <target>")?;
    let v=b.eval(&format!(r#"(()=>{{const e={};if(!e)return null;const r=e.getBoundingClientRect(),s=getComputedStyle(e);return {{tag:e.tagName.toLowerCase(),text:(e.innerText||'').trim(),role:e.getAttribute('role'),label:e.getAttribute('aria-label'),href:e.href||null,src:e.currentSrc||e.src||null,value:e.value??null,checked:e.checked??null,disabled:e.disabled??null,rect:{{x:r.x,y:r.y,width:r.width,height:r.height}},style:{{color:s.color,backgroundColor:s.backgroundColor,borderColor:s.borderColor,font:s.font,display:s.display,visibility:s.visibility}}}}}})()"#,t.js_resolver()))?;
    if v.is_null() {
        return Err("element not found".into());
    }
    Ok(pretty(&v))
}
pub fn scroll(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let t = args.first().map(String::as_str).unwrap_or("down");
    let e = match t {
        "down" => "scrollBy(0,innerHeight*.8); 'down'".into(),
        "up" => "scrollBy(0,-innerHeight*.8); 'up'".into(),
        "top" => "scrollTo(0,0); 'top'".into(),
        "bottom" => "scrollTo(0,document.body.scrollHeight); 'bottom'".into(),
        _ => {
            let t = crate::Target::parse(t)?;
            format!(
                "(()=>{{const e={};if(!e)return null;e.scrollIntoView({{block:'center'}});return 'ok'}})()",
                t.js_resolver()
            )
        }
    };
    let v = b.eval(&e)?;
    if v.is_null() {
        return Err("element not found".into());
    }
    Ok(v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string()))
}
pub fn query_selector(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let q = args.first().ok_or("usage: query-selector <css>")?;
    Ok(pretty(&b.eval(&format!(r#"(()=>[...document.querySelectorAll({})].map((e,i)=>{{const r=e.getBoundingClientRect();return {{n:i+1,tag:e.tagName.toLowerCase(),text:(e.innerText||e.textContent||'').replace(/\s+/g,' ').trim().slice(0,300),html:e.outerHTML.slice(0,1000),visible:r.width>0&&r.height>0&&getComputedStyle(e).visibility!=='hidden'&&getComputedStyle(e).display!=='none',x:r.x,y:r.y,width:r.width,height:r.height}}}}))()"#,js(q)))?))
}
pub fn get_element(b: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    if args.is_empty() {
        return Err("usage: get-element <target> [text|html|both]".into());
    }
    let t = target(args, 0, "usage: get-element <target> [text|html|both]")?;
    let mode = args.get(1).map(String::as_str).unwrap_or("both");
    let v=b.eval(&format!(r#"(()=>{{const e={};if(!e)return null;return {{text:(e.innerText||e.textContent||'').trim(),html:e.outerHTML}}}})()"#,t.js_resolver()))?;
    if v.is_null() {
        return Err("element not found".into());
    }
    Ok(match mode {
        "text" => v["text"].as_str().unwrap_or("").into(),
        "html" => v["html"].as_str().unwrap_or("").into(),
        "both" => pretty(&v),
        _ => return Err("mode must be text|html|both".into()),
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
    let v=b.eval(r#"(()=>[...document.querySelectorAll('a[href]')].filter(a=>{const r=a.getBoundingClientRect(),s=getComputedStyle(a);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'}).map(a=>({text:(a.innerText||a.getAttribute('aria-label')||'').replace(/\s+/g,' ').trim(),href:a.href})).filter(x=>x.text||x.href))()"#)?;
    let mut out = Vec::new();
    for (i, x) in v
        .as_array()
        .ok_or("failed to inspect links")?
        .iter()
        .enumerate()
    {
        out.push(format!(
            "{}. {} — {}",
            i + 1,
            x["text"].as_str().unwrap_or(""),
            x["href"].as_str().unwrap_or("")
        ))
    }
    Ok(out.join("\n"))
}
pub fn inspect_images(b: &mut BrowserSession, _: &[String]) -> Result<String, Error> {
    let v=b.eval(r#"(()=>[...document.images].filter(img=>{const r=img.getBoundingClientRect(),s=getComputedStyle(img);return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&(img.currentSrc||img.src)}).map((img,i)=>({n:i+1,alt:(img.alt||img.getAttribute('aria-label')||'').replace(/\s+/g,' ').trim(),src:img.currentSrc||img.src,width:Math.round(img.getBoundingClientRect().width),height:Math.round(img.getBoundingClientRect().height)})))()"#)?;
    let mut out = Vec::new();
    for x in v.as_array().ok_or("failed to inspect images")? {
        out.push(format!(
            "{}. {} [{}x{}] — {}",
            x["n"],
            x["alt"].as_str().unwrap_or(""),
            x["width"],
            x["height"],
            x["src"].as_str().unwrap_or("")
        ))
    }
    Ok(out.join("\n"))
}
