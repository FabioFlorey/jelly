use crate::{
    BrowserSession, CdpEventCursor, CdpEventFilter, CdpEventPoll, DEFAULT_CDP_EVENT_POLL_LIMIT,
    Error, ErrorKind, MAX_CDP_EVENT_POLL_LIMIT, jelly_error,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

const MAX_FILTER_VALUES: usize = 64;
const MAX_FILTER_STRING_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
enum BrowserEventsRequest {
    Subscribe {
        filter: CdpEventFilter,
    },
    Poll {
        subscription_id: String,
        limit: usize,
    },
    Unsubscribe {
        subscription_id: String,
    },
}

pub fn browser_events_input_schema() -> Value {
    json!({
        "oneOf":[
            {
                "type":"object",
                "properties":{
                    "action":{"const":"subscribe"},
                    "target":{
                        "type":"string",
                        "minLength":1,
                        "maxLength":MAX_FILTER_STRING_LEN,
                        "description":"Optional logical browser target such as main or tab-2."
                    },
                    "methods":{
                        "type":"array",
                        "maxItems":MAX_FILTER_VALUES,
                        "uniqueItems":true,
                        "items":{"type":"string","minLength":1,"maxLength":MAX_FILTER_STRING_LEN},
                        "description":"Optional exact CDP notification methods such as Runtime.executionContextCreated."
                    },
                    "method_prefixes":{
                        "type":"array",
                        "maxItems":MAX_FILTER_VALUES,
                        "uniqueItems":true,
                        "items":{"type":"string","minLength":1,"maxLength":MAX_FILTER_STRING_LEN},
                        "description":"Optional CDP notification method prefixes such as Network. or Network.webSocket."
                    }
                },
                "required":["action"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "action":{"const":"poll"},
                    "subscription_id":{"type":"string","minLength":1},
                    "limit":{
                        "type":"integer",
                        "minimum":1,
                        "maximum":MAX_CDP_EVENT_POLL_LIMIT,
                        "default":DEFAULT_CDP_EVENT_POLL_LIMIT
                    }
                },
                "required":["action","subscription_id"],
                "additionalProperties":false
            },
            {
                "type":"object",
                "properties":{
                    "action":{"const":"unsubscribe"},
                    "subscription_id":{"type":"string","minLength":1}
                },
                "required":["action","subscription_id"],
                "additionalProperties":false
            }
        ]
    })
}

pub fn validate_browser_events(arguments: &Value) -> Result<(), Error> {
    prepare_browser_events(arguments).map(|_| ())
}

pub fn execute_browser_events(
    browser: &mut BrowserSession,
    arguments: &Value,
) -> Result<Value, Error> {
    match prepare_browser_events(arguments)? {
        BrowserEventsRequest::Subscribe { filter } => {
            let target = filter.target().map(str::to_owned);
            let methods = filter.methods().map(str::to_owned).collect::<Vec<_>>();
            let method_prefixes = filter
                .method_prefixes()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let (subscription_id, cursor) = browser.subscribe_cdp_events(filter)?;
            let stats = browser.cdp_event_stats();

            Ok(json!({
                "action":"subscribe",
                "subscription_id":subscription_id,
                "cursor":cursor_value(cursor),
                "filters":{
                    "target":target,
                    "methods":methods,
                    "method_prefixes":method_prefixes
                },
                "stream":stats_value(stats)
            }))
        }
        BrowserEventsRequest::Poll {
            subscription_id,
            limit,
        } => {
            let poll = browser.poll_cdp_events(&subscription_id, limit)?;
            let stats = browser.cdp_event_stats();
            Ok(poll_value(&subscription_id, &poll, stats))
        }
        BrowserEventsRequest::Unsubscribe { subscription_id } => {
            browser.unsubscribe_cdp_events(&subscription_id)?;
            Ok(json!({
                "action":"unsubscribe",
                "subscription_id":subscription_id,
                "unsubscribed":true
            }))
        }
    }
}

fn prepare_browser_events(arguments: &Value) -> Result<BrowserEventsRequest, Error> {
    let object = arguments
        .as_object()
        .ok_or_else(|| invalid_arguments("browser-events arguments must be a JSON object"))?;
    let action = required_nonempty_string(object, "action", "browser-events")?;

    match action {
        "subscribe" => prepare_subscribe(object),
        "poll" => prepare_poll(object),
        "unsubscribe" => prepare_unsubscribe(object),
        other => Err(invalid_arguments(format!(
            "browser-events action must be subscribe, poll, or unsubscribe; got {other}"
        ))),
    }
}

fn prepare_subscribe(object: &Map<String, Value>) -> Result<BrowserEventsRequest, Error> {
    reject_unknown_fields(
        object,
        &["action", "target", "methods", "method_prefixes"],
        "browser-events subscribe",
    )?;

    let target =
        optional_nonempty_string(object, "target", "browser-events subscribe")?.map(str::to_owned);
    let methods = string_array(object, "methods", "browser-events subscribe")?;
    let method_prefixes = string_array(object, "method_prefixes", "browser-events subscribe")?;

    for method in &methods {
        if !valid_cdp_method(method) {
            return Err(invalid_arguments(format!(
                "browser-events subscribe method must be a CDP Domain.event identifier: {method}"
            )));
        }
    }
    for prefix in &method_prefixes {
        if !valid_cdp_method_prefix(prefix) {
            return Err(invalid_arguments(format!(
                "browser-events subscribe method_prefix must be a CDP method prefix: {prefix}"
            )));
        }
    }

    Ok(BrowserEventsRequest::Subscribe {
        filter: CdpEventFilter::new(target, methods, method_prefixes),
    })
}

fn prepare_poll(object: &Map<String, Value>) -> Result<BrowserEventsRequest, Error> {
    reject_unknown_fields(
        object,
        &["action", "subscription_id", "limit"],
        "browser-events poll",
    )?;
    let subscription_id =
        required_nonempty_string(object, "subscription_id", "browser-events poll")?.to_owned();
    let limit = match object.get("limit") {
        None => DEFAULT_CDP_EVENT_POLL_LIMIT,
        Some(Value::Number(value)) => {
            let value = value.as_u64().ok_or_else(|| {
                invalid_arguments("browser-events poll.limit must be a positive integer")
            })?;
            usize::try_from(value).map_err(|_| {
                invalid_arguments("browser-events poll.limit is too large for this platform")
            })?
        }
        Some(_) => {
            return Err(invalid_arguments(
                "browser-events poll.limit must be an integer",
            ));
        }
    };
    if !(1..=MAX_CDP_EVENT_POLL_LIMIT).contains(&limit) {
        return Err(invalid_arguments(format!(
            "browser-events poll.limit must be between 1 and {MAX_CDP_EVENT_POLL_LIMIT}"
        )));
    }

    Ok(BrowserEventsRequest::Poll {
        subscription_id,
        limit,
    })
}

fn prepare_unsubscribe(object: &Map<String, Value>) -> Result<BrowserEventsRequest, Error> {
    reject_unknown_fields(
        object,
        &["action", "subscription_id"],
        "browser-events unsubscribe",
    )?;
    Ok(BrowserEventsRequest::Unsubscribe {
        subscription_id: required_nonempty_string(
            object,
            "subscription_id",
            "browser-events unsubscribe",
        )?
        .to_owned(),
    })
}

fn string_array(
    object: &Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Vec<String>, Error> {
    let Some(value) = object.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| invalid_arguments(format!("{context}.{key} must be an array")))?;
    if values.len() > MAX_FILTER_VALUES {
        return Err(invalid_arguments(format!(
            "{context}.{key} supports at most {MAX_FILTER_VALUES} values"
        )));
    }

    let mut unique = BTreeSet::new();
    for value in values {
        let value = value
            .as_str()
            .ok_or_else(|| invalid_arguments(format!("{context}.{key} must contain strings")))?;
        let value = value.trim();
        if value.is_empty() {
            return Err(invalid_arguments(format!(
                "{context}.{key} must not contain empty strings"
            )));
        }
        if value.len() > MAX_FILTER_STRING_LEN {
            return Err(invalid_arguments(format!(
                "{context}.{key} values must be at most {MAX_FILTER_STRING_LEN} bytes"
            )));
        }
        if !unique.insert(value.to_owned()) {
            return Err(invalid_arguments(format!(
                "{context}.{key} contains duplicate value: {value}"
            )));
        }
    }
    Ok(unique.into_iter().collect())
}

fn valid_cdp_method(method: &str) -> bool {
    let Some((domain, event)) = method.split_once('.') else {
        return false;
    };
    !event.contains('.') && valid_identifier(domain) && valid_identifier(event)
}

fn valid_cdp_method_prefix(prefix: &str) -> bool {
    let Some((domain, suffix)) = prefix.split_once('.') else {
        return false;
    };
    if suffix.contains('.') || !valid_identifier(domain) {
        return false;
    }
    suffix
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn cursor_value(cursor: CdpEventCursor) -> Value {
    json!({
        "stream":cursor.stream,
        "sequence":cursor.sequence
    })
}

fn stats_value(stats: crate::CdpEventRingStats) -> Value {
    json!({
        "retained_count":stats.retained_count,
        "retained_bytes":stats.retained_bytes,
        "dropped_total":stats.dropped,
        "latest_dropped_sequence":stats.latest_dropped_sequence,
        "stream_resets_total":stats.stream_resets,
        "oldest_sequence":stats.oldest_sequence,
        "newest_sequence":stats.newest_sequence,
        "next_sequence":stats.next_sequence,
        "max_count":stats.max_count,
        "max_bytes":stats.max_bytes
    })
}

fn poll_value(
    subscription_id: &str,
    poll: &CdpEventPoll,
    stats: crate::CdpEventRingStats,
) -> Value {
    let events = poll
        .events
        .iter()
        .map(|event| {
            json!({
                "sequence":event.sequence(),
                "method":event.method(),
                "target":event.target(),
                "params":event.params()
            })
        })
        .collect::<Vec<_>>();

    json!({
        "action":"poll",
        "subscription_id":subscription_id,
        "events":events,
        "cursor":{
            "before":cursor_value(poll.cursor_before),
            "after":cursor_value(poll.cursor_after)
        },
        "loss":{
            "cursor_lost":poll.cursor_lost,
            "dropped":poll.dropped,
            "stream_resets":poll.stream_resets
        },
        "has_more":poll.has_more,
        "stream":stats_value(stats)
    })
}

fn required_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, Error> {
    let value = object
        .get(key)
        .ok_or_else(|| invalid_arguments(format!("{context} requires {key}")))?
        .as_str()
        .ok_or_else(|| invalid_arguments(format!("{context}.{key} must be a string")))?;
    if value.trim().is_empty() {
        return Err(invalid_arguments(format!(
            "{context}.{key} must not be empty"
        )));
    }
    Ok(value)
}

fn optional_nonempty_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<Option<&'a str>, Error> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .ok_or_else(|| invalid_arguments(format!("{context}.{key} must be a string")))?;
    if value.trim().is_empty() {
        return Err(invalid_arguments(format!(
            "{context}.{key} must not be empty"
        )));
    }
    if value.len() > MAX_FILTER_STRING_LEN {
        return Err(invalid_arguments(format!(
            "{context}.{key} must be at most {MAX_FILTER_STRING_LEN} bytes"
        )));
    }
    Ok(Some(value))
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    context: &str,
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
    Err(invalid_arguments(format!(
        "{context} contains unknown field{}: {}",
        if unknown.len() == 1 { "" } else { "s" },
        unknown.join(", ")
    )))
}

fn invalid_arguments(message: impl Into<String>) -> Error {
    jelly_error(ErrorKind::InvalidArguments, message, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_kind(error: Error) -> ErrorKind {
        crate::classify_error(error.as_ref()).0
    }

    #[test]
    fn input_schema_is_a_strict_three_action_union() {
        let schema = browser_events_input_schema();
        let branches = schema["oneOf"].as_array().unwrap();
        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0]["properties"]["action"]["const"], "subscribe");
        assert_eq!(branches[1]["properties"]["action"]["const"], "poll");
        assert_eq!(branches[2]["properties"]["action"]["const"], "unsubscribe");
        assert_eq!(
            branches[1]["properties"]["limit"]["maximum"],
            MAX_CDP_EVENT_POLL_LIMIT
        );
    }

    #[test]
    fn subscribe_filters_are_normalized_and_validated() {
        let request = prepare_browser_events(&json!({
            "action":"subscribe",
            "target":"main",
            "methods":["Runtime.executionContextCreated","Page.loadEventFired"],
            "method_prefixes":["Network.","Page.frame"]
        }))
        .unwrap();

        let BrowserEventsRequest::Subscribe { filter } = request else {
            panic!("expected subscribe request");
        };
        assert_eq!(filter.target(), Some("main"));
        assert_eq!(
            filter.methods().collect::<Vec<_>>(),
            vec!["Page.loadEventFired", "Runtime.executionContextCreated"]
        );
        assert_eq!(
            filter.method_prefixes().collect::<Vec<_>>(),
            vec!["Network.", "Page.frame"]
        );

        for invalid in [
            json!({"action":"subscribe","methods":["Runtime"]}),
            json!({"action":"subscribe","methods":["Runtime.bad.name"]}),
            json!({"action":"subscribe","method_prefixes":["Network"]}),
            json!({"action":"subscribe","method_prefixes":["Network.bad.name"]}),
            json!({"action":"subscribe","methods":["Runtime.enabled","Runtime.enabled"]}),
        ] {
            assert_eq!(
                error_kind(prepare_browser_events(&invalid).unwrap_err()),
                ErrorKind::InvalidArguments
            );
        }
    }

    #[test]
    fn poll_bounds_and_action_shapes_are_strict() {
        for invalid in [
            json!({"action":"poll","subscription_id":"s","limit":0}),
            json!({"action":"poll","subscription_id":"s","limit":501}),
            json!({"action":"poll","subscription_id":""}),
            json!({"action":"unsubscribe","subscription_id":"s","extra":true}),
            json!({"action":"unknown"}),
        ] {
            assert_eq!(
                error_kind(prepare_browser_events(&invalid).unwrap_err()),
                ErrorKind::InvalidArguments
            );
        }

        let BrowserEventsRequest::Poll { limit, .. } =
            prepare_browser_events(&json!({"action":"poll","subscription_id":"s"})).unwrap()
        else {
            panic!("expected poll");
        };
        assert_eq!(limit, DEFAULT_CDP_EVENT_POLL_LIMIT);
    }

    #[test]
    fn cdp_method_validation_accepts_domain_events_and_prefixes() {
        assert!(valid_cdp_method("Runtime.executionContextCreated"));
        assert!(valid_cdp_method("Network.webSocketFrameReceived"));
        assert!(!valid_cdp_method("Runtime."));
        assert!(!valid_cdp_method("Runtime.event.extra"));

        assert!(valid_cdp_method_prefix("Network."));
        assert!(valid_cdp_method_prefix("Network.webSocket"));
        assert!(!valid_cdp_method_prefix("Network"));
        assert!(!valid_cdp_method_prefix("Network.web.socket"));
    }
}
