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
        } else if value.starts_with("@e") {
            Ok(Self::Ref(value.trim_start_matches('@').to_owned()))
        } else if let Some(v) = value.strip_prefix("text:") {
            Ok(Self::Text(v.to_owned()))
        } else {
            Ok(Self::Text(value.to_owned()))
        }
    }

    pub fn js_resolver(&self) -> String {
        match self {
            Self::Ref(reference) => format!(
                "document.querySelector('[data-jelly-ref='+{}+']')",
                js(reference)
            ),
            Self::Css(selector) => format!("document.querySelector({})", js(selector)),
            Self::Text(text) => format!(
                r#"(()=>{{const q={};const vis=e=>{{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'}};const matches=e=>vis(e)&&(e.innerText||e.getAttribute('aria-label')||e.getAttribute('alt')||'').trim()===q;const interactive='a[href],button,input,textarea,select,[role=button],[role=link],[role=checkbox],[role=radio],[role=option],[tabindex]';return [...document.querySelectorAll(interactive)].find(matches)||[...document.querySelectorAll('*')].find(matches)||null}})()"#,
                js(text)
            ),
        }
    }
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
}
