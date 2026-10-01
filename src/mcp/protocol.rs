use serde_json::{Value, json};

pub(super) const SERVER_NAME: &str = "jelly";
pub(super) const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_INSTRUCTIONS: &str = include_str!("../../.agent/instructions/mcp.md");

#[derive(Debug, Clone)]
pub(super) struct McpRequest {
    pub(super) id: Option<Value>,
    pub(super) method: String,
    pub(super) params: Value,
}

impl McpRequest {
    pub(super) fn parse(value: &Value) -> Self {
        Self {
            id: value.get("id").cloned(),
            method: value
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            params: value.get("params").cloned().unwrap_or_else(|| json!({})),
        }
    }
}

pub(super) fn initialize(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION);
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {"listChanged": false}
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": SERVER_INSTRUCTIONS
    })
}

pub(super) fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

pub(super) fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc":"2.0",
        "id":id,
        "error":{"code":code,"message":message.into()}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parsing_preserves_notifications_and_defaults_params() {
        let request = McpRequest::parse(&json!({"jsonrpc":"2.0","method":"ping"}));
        assert!(request.id.is_none());
        assert_eq!(request.method, "ping");
        assert_eq!(request.params, json!({}));
    }

    #[test]
    fn response_helpers_preserve_json_rpc_envelope() {
        assert_eq!(
            success_response(json!(7), json!({"ok":true})),
            json!({"jsonrpc":"2.0","id":7,"result":{"ok":true}})
        );
        assert_eq!(
            error_response(json!(7), -32601, "missing"),
            json!({"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"missing"}})
        );
    }
}
