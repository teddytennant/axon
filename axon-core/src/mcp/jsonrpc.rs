use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// Create a new JSON-RPC 2.0 request (expects a response).
    pub fn new(id: u64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.into(),
            params,
        }
    }

    /// Create a JSON-RPC 2.0 notification (no response expected).
    pub fn notification(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 response message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Check if this is an error response.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Extract the result, returning an error if the response is an error.
    pub fn into_result(self) -> Result<serde_json::Value, JsonRpcError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok(self.result.unwrap_or(serde_json::Value::Null))
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// Check if a JSON value looks like a JSON-RPC response (has "jsonrpc" and "id" fields).
pub fn is_response(value: &serde_json::Value) -> bool {
    value.get("jsonrpc").is_some() && value.get("id").is_some()
}

/// Check if a JSON value looks like a JSON-RPC notification (has "method" but no "id").
pub fn is_notification(value: &serde_json::Value) -> bool {
    value.get("method").is_some() && value.get("id").is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_serialization() {
        let req = JsonRpcRequest::new(1, "tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"method\":\"tools/list\""));
        // params should be omitted when None
        assert!(!json.contains("params"));
    }

    #[test]
    fn request_with_params() {
        let params = json!({"name": "read_file", "arguments": {"path": "/tmp/test"}});
        let req = JsonRpcRequest::new(42, "tools/call", Some(params.clone()));
        let json_str = serde_json::to_string(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.id, Some(42));
        assert_eq!(parsed.params.unwrap(), params);
    }

    #[test]
    fn notification_has_no_id() {
        let notif = JsonRpcRequest::notification("notifications/initialized", None);
        let json = serde_json::to_string(&notif).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(json.contains("\"method\":\"notifications/initialized\""));
    }

    #[test]
    fn response_success() {
        let json_str = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(!resp.is_error());
        let result = resp.into_result().unwrap();
        assert_eq!(result, json!({"tools": []}));
    }

    #[test]
    fn response_error() {
        let json_str =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.is_error());
        let err = resp.into_result().unwrap_err();
        assert_eq!(err.code, -32600);
        assert_eq!(err.message, "Invalid Request");
    }

    #[test]
    fn response_error_with_data() {
        let json_str = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found","data":"tools/unknown"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        let err = resp.into_result().unwrap_err();
        assert_eq!(err.data, Some(json!("tools/unknown")));
    }

    #[test]
    fn is_response_check() {
        let resp = json!({"jsonrpc": "2.0", "id": 1, "result": {}});
        assert!(is_response(&resp));
        assert!(!is_notification(&resp));
    }

    #[test]
    fn is_notification_check() {
        let notif = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(is_notification(&notif));
        assert!(!is_response(&notif));
    }

    #[test]
    fn request_roundtrip() {
        let req = JsonRpcRequest::new(
            99,
            "initialize",
            Some(json!({"protocolVersion": "2024-11-05"})),
        );
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: JsonRpcRequest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(req, parsed);
    }

    #[test]
    fn error_display() {
        let err = JsonRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: None,
        };
        assert_eq!(
            format!("{}", err),
            "JSON-RPC error -32601: Method not found"
        );
    }

    #[test]
    fn response_null_result() {
        let json_str = r#"{"jsonrpc":"2.0","id":5}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert!(!resp.is_error());
        let result = resp.into_result().unwrap();
        assert_eq!(result, serde_json::Value::Null);
    }
}
