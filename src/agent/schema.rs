use crate::{PrimitiveSpec, named_primitive_input_schema};
use serde_json::{Value, json};

pub(crate) fn tool_output_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "ok":{"type":"boolean"},
            "data":{},
            "error":{
                "type":["object","null"],
                "properties":{
                    "kind":{"type":"string"},
                    "message":{"type":"string"},
                    "retryable":{"type":"boolean"},
                    "details":{"type":"object"}
                },
                "required":["kind","message","retryable"],
                "additionalProperties":false
            },
            "meta":{
                "type":"object",
                "properties":{"tool":{"type":"string"}},
                "required":["tool"],
                "additionalProperties":true
            }
        },
        "required":["ok","data","error","meta"],
        "additionalProperties":false
    })
}

pub(crate) fn primitive_input_schema(spec: &PrimitiveSpec) -> Result<Value, String> {
    named_primitive_input_schema(spec)
}

pub(crate) fn system_input_schema(name: &str) -> Option<Value> {
    Some(match name {
        "open-browser" => json!({
            "type":"object",
            "properties":{
                "url":{"type":"string","description":"Optional URL to open after Chromium starts."}
            },
            "additionalProperties":false
        }),
        "close-browser" | "downloads" => json!({
            "type":"object","properties":{},"additionalProperties":false
        }),
        "download" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["list","status","wait","cancel"]},
                "id":{"type":"string","description":"Chromium download GUID returned by download list/status."},
                "seconds":{"type":"integer","minimum":1,"description":"Maximum wait time for action=wait; defaults to 30."},
                "destination":{"type":"string","description":"Optional destination directory for a completed download. Jelly keeps the managed original and copies the suggested filename here."},
                "collision":{"type":"string","enum":["fail","overwrite","uniquify"],"description":"Destination collision policy; defaults to fail."}
            },
            "required":["action"],
            "additionalProperties":false
        }),
        "verify-artifact" => json!({
            "type":"object",
            "properties":{
                "artifact":{"type":"string","description":"Artifact ID or file path."},
                "semantic_checks":{"type":"array","items":{"type":"string"},"description":"Optional evidence labels already established before capture."}
            },
            "required":["artifact"],
            "additionalProperties":false
        }),
        "wait-download" => json!({
            "type":"object",
            "properties":{
                "after_ms":{"type":"integer","minimum":0,"description":"Unix timestamp in milliseconds captured before triggering the download."},
                "seconds":{"type":"integer","minimum":1,"description":"Maximum wait time; defaults to 30."},
                "name_contains":{"type":"string","description":"Optional filename substring."}
            },
            "required":["after_ms"],
            "additionalProperties":false
        }),
        "browser-task" => json!({
            "type":"object",
            "properties":{
                "url":{"type":"string"},
                "tool":{"type":"string"},
                "args":{"type":"array","items":{"type":"string"}},
                "persist":{"type":"boolean"}
            },
            "required":["url","tool"],
            "additionalProperties":false
        }),
        "profile-import" => json!({
            "type":"object",
            "properties":{
                "source":{"type":"string","description":"Closed Chromium user-data directory to copy into Jelly runtime."},
                "force":{"type":"boolean","description":"Replace an existing Jelly profile."}
            },
            "required":["source"],
            "additionalProperties":false
        }),
        "screenshot" => json!({
            "type":"object",
            "properties":{
                "target":{"type":"string","description":"Optional browser target such as body, main, css:..., or @eN. Omit for the active page viewport."},
                "output":{"type":"string","description":"Optional output path."}
            },
            "additionalProperties":false
        }),
        "record-browser" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["start","stop"]},
                "mode":{"type":"string","enum":["continuous","steps"],"description":"Recording mode for start. continuous streams frames; steps captures browser state after relevant actions."},
                "interval_ms":{"type":"integer","minimum":100,"description":"Frame interval for continuous mode; defaults to 500 ms."},
                "hold_ms":{"type":"integer","minimum":100,"description":"How long each captured action frame is shown in steps mode; defaults to 1000 ms."}
            },
            "required":["action"],
            "additionalProperties":false
        }),
        "inspect-network" => json!({
            "type":"object",
            "properties":{
                "action":{"type":"string","enum":["start","stop","show"]},
                "filters":{"type":"array","items":{"type":"string"}}
            },
            "required":["action"],
            "additionalProperties":false
        }),
        "call-routine" => json!({
            "type":"object",
            "properties":{
                "name":{"type":"string","description":"Routine name when starting a routine."},
                "resume_id":{"type":"string","description":"Continuation ID when resuming a suspended routine."},
                "vars":{"type":"object","additionalProperties":{"type":"string"}}
            },
            "oneOf":[{"required":["name"]},{"required":["resume_id"]}],
            "additionalProperties":false
        }),
        "hitl" => json!({
            "type":"object",
            "properties":{
                "message":{"type":"string"},
                "video":{"type":"string","description":"Optional local MP4 path to attach as a Telegram video instead of a screenshot."},
                "screenshot_target":{"type":"string","description":"Optional browser element to attach instead of the viewport."},
                "no_screenshot":{"type":"boolean"}
            },
            "required":["message"],
            "additionalProperties":false
        }),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive_specs;

    #[test]
    fn primitive_schema_uses_named_arguments() {
        let click = primitive_specs
            .iter()
            .find(|spec| spec.name == "click")
            .unwrap();
        let schema = primitive_input_schema(click).unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["target"]["type"], "string");
        assert_eq!(schema["required"][0], "target");
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn every_current_system_tool_has_an_input_schema() {
        for tool in crate::tool_specs {
            let schema = system_input_schema(tool.name)
                .unwrap_or_else(|| panic!("missing agent input schema for {}", tool.name));
            assert_eq!(schema["type"], "object", "{} input schema", tool.name);
        }
    }

    #[test]
    fn output_schema_is_a_closed_object_contract() {
        let schema = tool_output_schema();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"].as_array().unwrap().len(), 4);
        assert_eq!(
            schema["properties"]["error"]["properties"]["details"]["type"],
            "object"
        );
        assert_eq!(
            schema["properties"]["error"]["required"],
            json!(["kind", "message", "retryable"])
        );
    }
}
