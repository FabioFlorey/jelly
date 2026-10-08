use crate::{BrowserSession, Error, ErrorKind, jelly_error, sanitize_url};
use serde_json::{Map, Value, json};

fn active_url(browser: &mut BrowserSession) -> Result<String, Error> {
    let history = browser.call("Page.getNavigationHistory", json!({}))?;
    let index = history["result"]["currentIndex"].as_u64().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "Page.getNavigationHistory response is missing currentIndex",
            false,
        )
    })? as usize;
    history["result"]["entries"]
        .as_array()
        .and_then(|entries| entries.get(index))
        .and_then(|entry| entry["url"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "Page.getNavigationHistory response is missing the active URL",
                false,
            )
        })
}

fn scoped_url(browser: &mut BrowserSession, value: Option<&str>) -> Result<String, Error> {
    let url = match value {
        Some(url) => url.to_owned(),
        None => active_url(browser)?,
    };
    url::Url::parse(&url).map_err(|error| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("invalid cookie URL {url:?}: {error}"),
            false,
        )
    })?;
    Ok(url)
}

fn object_arg(value: &str, usage: &str) -> Result<Map<String, Value>, Error> {
    let value: Value = serde_json::from_str(value).map_err(|_| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("usage: {usage}"),
            false,
        )
    })?;
    value.as_object().cloned().ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("usage: {usage}"),
            false,
        )
    })
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    operation: &str,
) -> Result<(), Error> {
    let mut unknown = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(jelly_error(
        ErrorKind::InvalidArguments,
        format!(
            "{operation} has unsupported field{}: {}",
            if unknown.len() == 1 { "" } else { "s" },
            unknown.join(", ")
        ),
        false,
    ))
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    name: &str,
    operation: &str,
) -> Result<&'a str, Error> {
    object.get(name).and_then(Value::as_str).ok_or_else(|| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("{operation} requires string field {name}"),
            false,
        )
    })
}

pub fn cookies(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let url = scoped_url(browser, args.first().map(String::as_str))?;
    let response = browser.call("Network.getCookies", json!({"urls":[url]}))?;
    let cookies = response["result"]["cookies"]
        .as_array()
        .cloned()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "Network.getCookies response is missing cookies",
                false,
            )
        })?;
    Ok(crate::primitives::pretty(&json!({
        "url": sanitize_url(&url),
        "cookies": cookies
    })))
}

pub fn set_cookie(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let usage = "set-cookie <cookie-json>";
    let mut cookie = object_arg(crate::primitives::arg(args, 0, usage)?, usage)?;
    reject_unknown(
        &cookie,
        &[
            "name", "value", "url", "domain", "path", "secure", "httpOnly", "sameSite", "expires",
        ],
        "set-cookie",
    )?;
    required_string(&cookie, "name", "set-cookie")?;
    required_string(&cookie, "value", "set-cookie")?;

    if let Some(value) = cookie.get("url")
        && !value.is_string()
    {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "set-cookie field url must be a string",
            false,
        ));
    }
    for name in ["domain", "path", "sameSite"] {
        if let Some(value) = cookie.get(name)
            && !value.is_string()
        {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                format!("set-cookie field {name} must be a string"),
                false,
            ));
        }
    }
    for name in ["secure", "httpOnly"] {
        if let Some(value) = cookie.get(name)
            && !value.is_boolean()
        {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                format!("set-cookie field {name} must be a boolean"),
                false,
            ));
        }
    }
    if let Some(value) = cookie.get("expires")
        && !value.is_number()
    {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "set-cookie field expires must be a number",
            false,
        ));
    }
    if let Some(value) = cookie.get("sameSite").and_then(Value::as_str)
        && !matches!(value, "Strict" | "Lax" | "None")
    {
        return Err(jelly_error(
            ErrorKind::InvalidArguments,
            "set-cookie sameSite must be Strict, Lax, or None",
            false,
        ));
    }
    if !cookie.contains_key("url") && !cookie.contains_key("domain") {
        cookie.insert("url".into(), Value::String(active_url(browser)?));
    }

    let response = browser.call("Network.setCookie", Value::Object(cookie.clone()))?;
    if response["result"]["success"].as_bool() == Some(false) {
        return Err(jelly_error(
            ErrorKind::InteractionFailed,
            "Chromium rejected the cookie",
            false,
        ));
    }
    Ok(crate::primitives::pretty(&json!({
        "set": true,
        "name": cookie["name"],
        "scope": cookie.get("url").and_then(|value| value.as_str().map(sanitize_url))
            .or_else(|| cookie.get("domain").and_then(Value::as_str).map(str::to_owned))
    })))
}

pub fn delete_cookie(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let usage = "delete-cookie <selector-json>";
    let mut selector = object_arg(crate::primitives::arg(args, 0, usage)?, usage)?;
    reject_unknown(
        &selector,
        &["name", "url", "domain", "path"],
        "delete-cookie",
    )?;
    required_string(&selector, "name", "delete-cookie")?;
    for name in ["url", "domain", "path"] {
        if let Some(value) = selector.get(name)
            && !value.is_string()
        {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                format!("delete-cookie field {name} must be a string"),
                false,
            ));
        }
    }
    if !selector.contains_key("url") && !selector.contains_key("domain") {
        selector.insert("url".into(), Value::String(active_url(browser)?));
    }
    browser.call("Network.deleteCookies", Value::Object(selector.clone()))?;
    Ok(crate::primitives::pretty(&json!({
        "deleted": true,
        "name": selector["name"]
    })))
}

pub fn clear_cookies(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let url = scoped_url(browser, args.first().map(String::as_str))?;
    let response = browser.call("Network.getCookies", json!({"urls":[url]}))?;
    let cookies = response["result"]["cookies"]
        .as_array()
        .cloned()
        .ok_or_else(|| {
            jelly_error(
                ErrorKind::Internal,
                "Network.getCookies response is missing cookies",
                false,
            )
        })?;
    let mut deleted = 0usize;
    for cookie in &cookies {
        let Some(name) = cookie["name"].as_str() else {
            continue;
        };
        let Some(domain) = cookie["domain"].as_str() else {
            continue;
        };
        let path = cookie["path"].as_str().unwrap_or("/");
        browser.call(
            "Network.deleteCookies",
            json!({"name":name,"domain":domain,"path":path}),
        )?;
        deleted += 1;
    }
    Ok(crate::primitives::pretty(&json!({
        "url": sanitize_url(&url),
        "deleted": deleted
    })))
}

fn storage_origin(browser: &mut BrowserSession) -> Result<String, Error> {
    let url = active_url(browser)?;
    let parsed = url::Url::parse(&url).map_err(|error| {
        jelly_error(
            ErrorKind::InvalidArguments,
            format!("active page URL cannot be used for DOM storage: {error}"),
            false,
        )
    })?;
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return Err(jelly_error(
            ErrorKind::Unsupported,
            format!(
                "active page has an opaque origin and no addressable DOM storage: {}",
                sanitize_url(&url)
            ),
            false,
        ));
    }
    Ok(origin)
}

fn storage_id(browser: &mut BrowserSession, area: &str) -> Result<(String, Value), Error> {
    let is_local = match area {
        "local" => true,
        "session" => false,
        _ => {
            return Err(jelly_error(
                ErrorKind::InvalidArguments,
                "storage area must be local or session",
                false,
            ));
        }
    };
    let origin = storage_origin(browser)?;
    Ok((
        origin.clone(),
        json!({"securityOrigin":origin,"isLocalStorage":is_local}),
    ))
}

fn validated_storage_entries(response: &Value) -> Result<Vec<(String, String)>, Error> {
    let entries = response["result"]["entries"].as_array().ok_or_else(|| {
        jelly_error(
            ErrorKind::Internal,
            "DOMStorage.getDOMStorageItems response is missing an entries array",
            false,
        )
    })?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let pair = entry.as_array().filter(|pair| pair.len() == 2);
            match pair {
                Some(pair) => match (pair[0].as_str(), pair[1].as_str()) {
                    (Some(key), Some(value)) => Ok((key.to_owned(), value.to_owned())),
                    _ => Err(index),
                },
                None => Err(index),
            }
            .map_err(|index| {
                jelly_error(
                    ErrorKind::Internal,
                    format!(
                        "DOMStorage.getDOMStorageItems response has invalid entry at index {index}"
                    ),
                    false,
                )
            })
        })
        .collect()
}

pub fn storage_list(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let area = crate::primitives::arg(args, 0, "storage-list <local|session>")?;
    let (origin, storage_id) = storage_id(browser, area)?;
    let response = browser.call(
        "DOMStorage.getDOMStorageItems",
        json!({"storageId":storage_id}),
    )?;
    let entries = validated_storage_entries(&response)?;
    Ok(crate::primitives::pretty(&json!({
        "area": area,
        "origin": origin,
        "entries": entries
    })))
}

pub fn storage_get(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let area = crate::primitives::arg(args, 0, "storage-get <local|session> <key>")?;
    let key = crate::primitives::arg(args, 1, "storage-get <local|session> <key>")?;
    let (origin, storage_id) = storage_id(browser, area)?;
    let response = browser.call(
        "DOMStorage.getDOMStorageItems",
        json!({"storageId":storage_id}),
    )?;
    let entries = validated_storage_entries(&response)?;
    let value = entries
        .iter()
        .find(|(entry_key, _)| entry_key == key)
        .map(|(_, value)| Value::String(value.clone()))
        .unwrap_or(Value::Null);
    Ok(crate::primitives::pretty(&json!({
        "area": area,
        "origin": origin,
        "key": key,
        "value": value,
        "found": !value.is_null()
    })))
}

pub fn storage_set(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let usage = "storage-set <local|session> <key> <value>";
    let area = crate::primitives::arg(args, 0, usage)?;
    let key = crate::primitives::arg(args, 1, usage)?;
    let value = crate::primitives::arg(args, 2, usage)?;
    let (origin, storage_id) = storage_id(browser, area)?;
    browser.call(
        "DOMStorage.setDOMStorageItem",
        json!({"storageId":storage_id,"key":key,"value":value}),
    )?;
    Ok(crate::primitives::pretty(&json!({
        "area": area,
        "origin": origin,
        "key": key,
        "set": true
    })))
}

pub fn storage_remove(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let usage = "storage-remove <local|session> <key>";
    let area = crate::primitives::arg(args, 0, usage)?;
    let key = crate::primitives::arg(args, 1, usage)?;
    let (origin, storage_id) = storage_id(browser, area)?;
    browser.call(
        "DOMStorage.removeDOMStorageItem",
        json!({"storageId":storage_id,"key":key}),
    )?;
    Ok(crate::primitives::pretty(&json!({
        "area": area,
        "origin": origin,
        "key": key,
        "removed": true
    })))
}

pub fn storage_clear(browser: &mut BrowserSession, args: &[String]) -> Result<String, Error> {
    let area = crate::primitives::arg(args, 0, "storage-clear <local|session>")?;
    let (origin, storage_id) = storage_id(browser, area)?;
    browser.call("DOMStorage.clear", json!({"storageId":storage_id}))?;
    Ok(crate::primitives::pretty(&json!({
        "area": area,
        "origin": origin,
        "cleared": true
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_entries_accept_valid_and_empty_responses() {
        assert_eq!(
            validated_storage_entries(&json!({"result":{"entries":[]}})).unwrap(),
            Vec::<(String, String)>::new()
        );
        assert_eq!(
            validated_storage_entries(&json!({"result":{"entries":[["a",""],["b","value"]]}}))
                .unwrap(),
            vec![("a".into(), "".into()), ("b".into(), "value".into())]
        );
    }

    #[test]
    fn storage_entries_reject_malformed_responses() {
        for response in [
            json!({}),
            json!({"result":{"entries":null}}),
            json!({"result":{"entries":{}}}),
            json!({"result":{"entries":[["key"]]}}),
            json!({"result":{"entries":[["key","value","extra"]]}}),
            json!({"result":{"entries":[[1,"value"]]}}),
            json!({"result":{"entries":[["key",null]]}}),
        ] {
            assert!(
                validated_storage_entries(&response).is_err(),
                "accepted {response}"
            );
        }
    }
}
