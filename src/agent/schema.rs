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
                "action":{"type":"string","enum":["list","status","wait","transfer","cancel"]},
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
                "name":{"type":"string","description":"Saved routine name, UUID or name@vN pinned revision to execute or describe."},
                "resume_id":{"type":"string","description":"Continuation ID when resuming a suspended routine."},
                "owner":{"type":"string","minLength":1,"maxLength":64,"description":"Human-readable lease claimant label; not an authenticated agent identity."},
                "ttl_seconds":{"type":"integer","minimum":60,"maximum":3600,"description":"Optional claim or renewal lifetime (default 900 seconds)."},
                "takeover_reason":{"type":"string","minLength":12,"maxLength":300,"description":"Mandatory for explicitly claiming an expired lease after inspecting possible side effects."},
                "lease_token":{"type":"string","pattern":"^[0-9a-f]{32}$","description":"Secret returned by claim; required to resume a claimed routine, renew or release it."},
                "handoff_to":{"type":"string","minLength":1,"description":"Optional operator-configured agent profile for this run; omitted or caller routes to the invoking agent. Fixed across resume."},
                "vars":{"type":"object","additionalProperties":{"type":"string"}},
                "action":{"type":"string","enum":["save","list","search","describe","status","claim","renew","release","versions","revise","promote","crystallize","capture"],"description":"Optional management operation. Omit to execute a saved graph."},
                "query":{"type":"string","minLength":1,"maxLength":200,"description":"Case-insensitive substring of routine name or description for action=search."},
                "operation":{"type":"string","enum":["start","status","stop"],"description":"Capture management operation, when action=capture."},
                "scope":{"type":"string","enum":["isolated"],"description":"Opt-in: start an isolated capture, selected explicitly by capture_id per MCP browser call or JELLY_CAPTURE_ID for CLI. No authenticated agent identity is implied."},
                "parameters":{"type":"object","maxProperties":12,"additionalProperties":{"type":"string"},"description":"Optional crystallization bindings: required-input-name -> exact previously recorded argument. No inference or defaults."},
                "capture_id":{"type":"string","pattern":"^capture-[0-9a-f]{32}$","description":"Capture identifier from start, required for stop and optional for historical status."},
                "trace_id":{"type":"string","minLength":1,"maxLength":128,"description":"Shared trace ID from Jelly actions.jsonl; used by action=crystallize. No arbitrary file path accepted."},
                "revision":{"type":"integer","minimum":1,"maximum":1000000,"description":"Revision to activate; use versions to discover."},
                "expected_revision":{"type":"integer","minimum":1,"maximum":1000000,"description":"Active revision expected by the caller (compare-and-swap guard)."},
                "graph":{"type":"object","description":"JSON graph with name, description, entry, nodes; required for save/revise. Neither executes."}
            },
            "oneOf":[
                {"required":["name"],"not":{"anyOf":[{"required":["action"]},{"required":["resume_id"]},{"required":["lease_token"]}]}},
                {"required":["resume_id"],"not":{"anyOf":[{"required":["action"]},{"required":["handoff_to"]}]}},
                {"required":["action","graph"],"properties":{"action":{"const":"save"}}},
                {"required":["action"],"properties":{"action":{"const":"list"}}},
                {"required":["action","query"],"properties":{"action":{"const":"search"}}},
                {"required":["action","name"],"properties":{"action":{"const":"versions"}}},
                {"required":["action","name","expected_revision","graph"],"properties":{"action":{"const":"revise"}}},
                {"required":["action","name","revision","expected_revision"],"properties":{"action":{"const":"promote"}}},
                {"required":["action","name","expected_revision","trace_id"],"properties":{"action":{"const":"crystallize"}}},
                {"required":["action","operation"],"properties":{"action":{"const":"capture"},"operation":{"enum":["start","status"]}}},
                {"required":["action","operation","capture_id"],"properties":{"action":{"const":"capture"},"operation":{"const":"stop"}}},
                {"required":["action","resume_id"],"properties":{"action":{"const":"status"}}},
                {"required":["action","resume_id","owner"],"properties":{"action":{"const":"claim"}}},
                {"required":["action","resume_id","lease_token"],"properties":{"action":{"const":"renew"}}},
                {"required":["action","resume_id","lease_token"],"properties":{"action":{"const":"release"}}},
                {"required":["action","name"],"properties":{"action":{"const":"describe"}}}
            ],
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
