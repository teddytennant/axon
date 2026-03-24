//! Minimal MCP server for integration testing.
//!
//! Implements the MCP JSON-RPC 2.0 protocol over stdio.
//! Provides two tools:
//! - `echo`: Returns the input text back
//! - `add`: Adds two numbers
//!
//! This binary is used by axon-core integration tests to verify
//! the MCP client/bridge/registry pipeline end-to-end.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = request.get("id").cloned();
        let params = request.get("params").cloned().unwrap_or(json!({}));

        // Notifications (no id) don't get responses
        if id.is_none() {
            continue;
        }

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "axon-test-mcp-server",
                        "version": "0.1.0"
                    }
                }
            }),

            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "description": "Returns the input text back unchanged",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "text": {
                                        "type": "string",
                                        "description": "Text to echo back"
                                    }
                                },
                                "required": ["text"]
                            }
                        },
                        {
                            "name": "add",
                            "description": "Adds two numbers together",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "a": {
                                        "type": "number",
                                        "description": "First number"
                                    },
                                    "b": {
                                        "type": "number",
                                        "description": "Second number"
                                    }
                                },
                                "required": ["a", "b"]
                            }
                        }
                    ]
                }
            }),

            "tools/call" => {
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                match tool_name {
                    "echo" => {
                        let text = arguments.get("text").and_then(|v| v.as_str()).unwrap_or("");
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": text
                                    }
                                ]
                            }
                        })
                    }
                    "add" => {
                        let a = arguments.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        let b = arguments.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                        json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("{}", a + b)
                                    }
                                ]
                            }
                        })
                    }
                    other => json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("Unknown tool: {}", other)
                        }
                    }),
                }
            }

            other => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {}", other)
                }
            }),
        };

        let json_str = serde_json::to_string(&response).unwrap();
        let _ = writeln!(stdout, "{}", json_str);
        let _ = stdout.flush();
    }
}
