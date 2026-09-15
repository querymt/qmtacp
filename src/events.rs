use serde_json::{Value, json};

pub fn compact_session_update(session_id: &Value, update: &Value) -> Option<Value> {
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("session_update"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let session_id = session_id.clone();
    match kind {
        "agent_message_chunk" | "user_message_chunk" => Some(json!({
            "type": "text",
            "sessionId": session_id,
            "role": if kind.starts_with("user") { "user" } else { "assistant" },
            "text": content_text(update.get("content").unwrap_or(&Value::Null)),
        })),
        "agent_thought_chunk" => None,
        "tool_call" | "tool_call_update" => Some(json!({
            "type": "tool",
            "sessionId": session_id,
            "id": first_string(update, &["toolCallId", "tool_call_id", "id"]),
            "name": first_string(update, &["title", "kind", "name"]),
            "status": first_string(update, &["status"]).unwrap_or_else(|| {
                if kind == "tool_call" {
                    "started".to_string()
                } else {
                    "updated".to_string()
                }
            }),
        })),
        "current_mode_update" => Some(json!({
            "type": "mode",
            "sessionId": session_id,
            "mode": first_string(update, &["currentModeId", "current_mode_id", "mode"]),
        })),
        "plan" => Some(json!({
            "type": "plan",
            "sessionId": session_id,
            "entries": update.get("entries").cloned().unwrap_or(Value::Null),
        })),
        "available_commands_update" => None,
        _ => Some(json!({
            "type": "update",
            "sessionId": session_id,
            "kind": kind,
        })),
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(content_text).collect::<Vec<_>>().join(""),
        Value::Object(_) => content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key)? {
        Value::String(text) => Some(text.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_agent_text_chunk() {
        let event = compact_session_update(
            &json!("s1"),
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "hello" }
            }),
        )
        .unwrap();
        assert_eq!(event["type"], "text");
        assert_eq!(event["role"], "assistant");
        assert_eq!(event["text"], "hello");
    }

    #[test]
    fn skips_thought_chunks() {
        assert!(
            compact_session_update(
                &json!("s1"),
                &json!({
                    "sessionUpdate": "agent_thought_chunk",
                    "content": { "type": "text", "text": "secret" }
                }),
            )
            .is_none()
        );
    }

    #[test]
    fn compact_tool_and_mode() {
        let tool = compact_session_update(
            &json!("s1"),
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "t1",
                "title": "bash",
                "status": "pending"
            }),
        )
        .unwrap();
        assert_eq!(tool["type"], "tool");
        assert_eq!(tool["id"], "t1");
        assert_eq!(tool["name"], "bash");

        let mode = compact_session_update(
            &json!("s1"),
            &json!({
                "sessionUpdate": "current_mode_update",
                "currentModeId": "plan"
            }),
        )
        .unwrap();
        assert_eq!(mode["mode"], "plan");
    }
}
