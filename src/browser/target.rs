use crate::{Error, ErrorKind, jelly_error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Ref(String),
    Css(String),
    Text(String),
}

impl Target {
    pub fn parse(value: &str) -> Result<Self, Error> {
        if value.trim().is_empty() {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                "target cannot be empty",
                false,
            ));
        }
        if let Some(v) = value.strip_prefix("css:") {
            if v.trim().is_empty() {
                return Err(jelly_error(
                    ErrorKind::InvalidArguments,
                    "css target cannot be empty",
                    false,
                ));
            }
            Ok(Self::Css(v.to_owned()))
        } else if value.starts_with("@e") || value.starts_with("@img") {
            Ok(Self::Ref(value.trim_start_matches('@').to_owned()))
        } else if let Some(v) = value.strip_prefix("text:") {
            Ok(Self::Text(v.to_owned()))
        } else {
            Ok(Self::Text(value.to_owned()))
        }
    }

    pub fn js_resolver(&self) -> String {
        match self {
            Self::Ref(reference) => {
                let reference_js = js(reference);
                if is_legacy_dom_ref(reference) {
                    format!(
                        "(globalThis.__jellyRuntimeV1?.resolveRef({0})||document.querySelector('[data-jelly-ref='+{0}+']'))",
                        reference_js
                    )
                } else {
                    format!("globalThis.__jellyRuntimeV1?.resolveRef({reference_js})||null")
                }
            }
            Self::Css(selector) => format!("document.querySelector({})", js(selector)),
            Self::Text(text) => format!(
                r#"(()=>{{
                    const norm=v=>(v||'').replace(/\s+/g,' ').trim();
                    const q=norm({});
                    const qParts=q.split(' ').filter(Boolean);
                    const runtime=globalThis.__jellyRuntimeV1;
                    const indexed=runtime?.resolveText(q);
                    if(indexed)return indexed;
                    const vis=e=>{{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&s.opacity!=='0'}};
                    const exact=e=>vis(e)&&norm(e.innerText||e.getAttribute('aria-label')||e.getAttribute('alt')||'')===q;
                    const likely=e=>{{
                        const aria=norm(e.getAttribute('aria-label'));
                        const alt=norm(e.getAttribute('alt'));
                        if(aria===q||alt===q)return exact(e);
                        const text=norm(e.textContent);
                        if(text.includes(q))return exact(e);
                        if(e.children?.length&&qParts.every(part=>text.includes(part)))return exact(e);
                        return false;
                    }};
                    const interactive='a[href],button,input,textarea,select,[role=button],[role=link],[role=checkbox],[role=radio],[role=option],[tabindex]';
                    if(!runtime){{for(const e of document.querySelectorAll(interactive))if(exact(e))return e;}}
                    else if(runtime.metrics)runtime.metrics.genericTextFallbacks++;
                    const root=document.documentElement;
                    if(root){{
                        const walker=document.createTreeWalker(root,NodeFilter.SHOW_ELEMENT);
                        for(let e=walker.currentNode;e;e=walker.nextNode()){{
                            if(likely(e)){{if(runtime?.metrics)runtime.metrics.genericTextPrefilterHits++;return e;}}
                        }}
                    }}
                    if(runtime?.metrics)runtime.metrics.genericTextSlowFallbacks++;
                    for(const e of document.querySelectorAll('*'))if(exact(e))return e;
                    if(runtime?.metrics)runtime.metrics.genericTextFallbackMisses++;
                    return null;
                }})()"#,
                js(text)
            ),
        }
    }

    pub fn js_scroll_resolver(&self) -> String {
        match self {
            Self::Text(text) => format!(
                r#"(()=>{{
                    const norm=v=>(v||'').replace(/\s+/g,' ').trim();
                    const q=norm({});
                    const visible=e=>{{
                        if(!e?.isConnected)return false;
                        const r=e.getBoundingClientRect(),s=getComputedStyle(e);
                        return r.width>0&&r.height>0&&s.display!=='none'&&s.visibility!=='hidden'&&s.opacity!=='0';
                    }};
                    const text=e=>norm(e.innerText||e.getAttribute?.('aria-label')||e.getAttribute?.('alt')||'');
                    const interactive=e=>e.matches?.('a[href],button,input,textarea,select,[role=button],[role=link],[role=checkbox],[role=radio],[role=option],[tabindex]')||false;
                    const candidates=[];
                    const seen=new Set();
                    let order=0;
                    const add=e=>{{
                        if(!e||seen.has(e)||!visible(e)||text(e)!==q)return;
                        seen.add(e);
                        const r=e.getBoundingClientRect();
                        const heading=/^H[1-6]$/.test(e.tagName)||e.getAttribute?.('role')==='heading';
                        const inViewport=r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth;
                        candidates.push({{e,heading:heading?0:1,interactive:interactive(e)?1:0,offscreen:inViewport?1:0,order:order++}});
                    }};
                    add(globalThis.__jellyRuntimeV1?.resolveText(q));
                    const root=document.documentElement;
                    if(root){{
                        const walker=document.createTreeWalker(root,NodeFilter.SHOW_ELEMENT);
                        for(let e=walker.currentNode;e;e=walker.nextNode())add(e);
                    }}
                    candidates.sort((a,b)=>
                        a.heading-b.heading||
                        a.interactive-b.interactive||
                        a.offscreen-b.offscreen||
                        a.order-b.order
                    );
                    return candidates[0]?.e||null;
                }})()"#,
                js(text)
            ),
            _ => self.js_resolver(),
        }
    }
}

fn is_legacy_dom_ref(reference: &str) -> bool {
    ["e", "img"].iter().any(|prefix| {
        reference
            .strip_prefix(prefix)
            .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()))
    })
}

pub fn js(s: &str) -> String {
    serde_json::to_string(s).expect("string serialization")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_targets() {
        assert_eq!(Target::parse("@e12").unwrap(), Target::Ref("e12".into()));
        assert_eq!(Target::parse("@img3").unwrap(), Target::Ref("img3".into()));
        assert_eq!(
            Target::parse("css:#save").unwrap(),
            Target::Css("#save".into())
        );
        assert_eq!(
            Target::parse("text:Save").unwrap(),
            Target::Text("Save".into())
        );
        assert_eq!(Target::parse("Save").unwrap(), Target::Text("Save".into()));
    }

    #[test]
    fn refs_and_text_prefer_the_page_runtime_with_legacy_fallbacks() {
        let legacy_reference = Target::parse("@e12").unwrap().js_resolver();
        assert!(legacy_reference.contains("__jellyRuntimeV1?.resolveRef"));
        assert!(legacy_reference.contains("data-jelly-ref"));

        let image_reference = Target::parse("@img3").unwrap().js_resolver();
        assert!(image_reference.contains("data-jelly-ref"));

        let runtime_reference = Target::parse("@eabc123-7").unwrap().js_resolver();
        assert!(runtime_reference.contains("__jellyRuntimeV1?.resolveRef"));
        assert!(!runtime_reference.contains("data-jelly-ref"));

        let text = Target::parse("text:Save").unwrap().js_resolver();
        assert!(text.contains("const runtime=globalThis.__jellyRuntimeV1"));
        assert!(text.contains("runtime?.resolveText"));
        assert!(text.contains("genericTextFallbacks"));
        assert!(text.contains(r#"replace(/\s+/g,' ')"#));
        assert!(text.contains("querySelectorAll('*')"));
    }

    #[test]
    fn scroll_text_resolver_prefers_section_content_over_duplicate_navigation_text() {
        let scroll = Target::parse("text:References")
            .unwrap()
            .js_scroll_resolver();
        assert!(scroll.contains("heading:heading?0:1"));
        assert!(scroll.contains("interactive:interactive(e)?1:0"));
        assert!(scroll.contains("offscreen:inViewport?1:0"));
        assert!(scroll.contains("s.opacity!=='0'"));
    }
}
