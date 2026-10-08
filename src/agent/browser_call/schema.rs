use super::{MAX_BATCH_CALLS, RawCdpAccess};
use serde_json::{Value, json};

pub fn browser_call_input_schema(raw_cdp: RawCdpAccess) -> Value {
    let semantic_item = json!({
        "type":"object",
        "properties":{
            "target":{"type":"string","minLength":1,"description":"Optional logical page target such as main or tab-2. Omit to use the active target."},
            "call":{
                "type":"object",
                "properties":{
                    "jelly":{"type":"string","minLength":1,"description":"Semantic operation name from browser-schema, not a raw CDP method."},
                    "params":{"type":"object","default":{},"description":"Named JSON arguments matching browser-schema schema for this operation; not CLI positional args."}
                },
                "required":["jelly"],
                "additionalProperties":false
            }
        },
        "required":["call"],
        "additionalProperties":false
    });

    let item_schema = if raw_cdp.enabled() {
        json!({
            "oneOf":[
                semantic_item,
                {
                    "type":"object",
                    "description":"Privileged raw CDP call on one attached page target. This bypasses Jelly semantic-operation policy and may access page/runtime/DOM/network/storage/input capabilities exposed by Chromium. Use only when the operator has explicitly enabled raw CDP.",
                    "properties":{
                        "scope":{"const":"target","description":"Send directly to the selected page target's privileged CDP session."},
                        "target":{"type":"string","minLength":1,"description":"Optional logical page target such as main or tab-2. Omit to use the active target."},
                        "call":{
                            "type":"object",
                            "properties":{
                                "method":{
                                    "type":"string",
                                    "pattern":"^[A-Za-z][A-Za-z0-9_]*\\.[A-Za-z][A-Za-z0-9_]*$"
                                },
                                "params":{"type":"object","default":{}}
                            },
                            "required":["method"],
                            "additionalProperties":false
                        }
                    },
                    "required":["scope","call"],
                    "additionalProperties":false
                },
                {
                    "type":"object",
                    "description":"Privileged raw CDP call on the browser-level connection. This is broader than page JavaScript or Jelly semantic operations and may affect targets, browser contexts, permissions, cookies/storage, downloads, networking, or other Chromium-wide state. Use only when the operator has explicitly enabled raw CDP.",
                    "properties":{
                        "scope":{"const":"browser","description":"Send directly to Chromium's browser-level CDP connection."},
                        "call":{
                            "type":"object",
                            "properties":{
                                "method":{
                                    "type":"string",
                                    "pattern":"^[A-Za-z][A-Za-z0-9_]*\\.[A-Za-z][A-Za-z0-9_]*$"
                                },
                                "params":{"type":"object","default":{}}
                            },
                            "required":["method"],
                            "additionalProperties":false
                        }
                    },
                    "required":["scope","call"],
                    "additionalProperties":false
                }
            ]
        })
    } else {
        semantic_item
    };

    json!({
        "type":"object",
        "properties":{
            "calls":{
                "type":"array",
                "minItems":1,
                "maxItems":MAX_BATCH_CALLS,
                "items":item_schema
            },
            "on_error":{
                "type":"string",
                "enum":["stop","continue"],
                "default":"stop"
            }
        },
        "required":["calls"],
        "additionalProperties":false
    })
}

pub fn cdp_call_input_schema() -> Value {
    let mut schema = browser_call_input_schema(RawCdpAccess::Enabled);
    let branches = schema["properties"]["calls"]["items"]["oneOf"]
        .as_array_mut()
        .expect("raw browser-call schema must expose oneOf branches");
    branches.remove(0);
    schema
}
