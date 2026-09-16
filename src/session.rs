use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

const SESSION_LOAD_SNAPSHOT_META_KEY: &str = "querymt/sessionLoadSnapshot.v1";
const INSPECT_TEXT_LIMIT: usize = 4_000;
const FIND_PAGE_CAP: usize = 20;

pub fn resolve_cwd(cwd: Option<PathBuf>) -> Result<PathBuf> {
    let path = cwd.unwrap_or(std::env::current_dir().context("current directory")?);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("current directory")?
            .join(path)
    };
    if !path.is_absolute() {
        bail!("cwd must be an absolute path: {}", path.display());
    }
    Ok(path)
}

pub fn resolve_optional_cwd(cwd: Option<PathBuf>) -> Result<Option<PathBuf>> {
    cwd.map(|cwd| resolve_cwd(Some(cwd))).transpose()
}

pub fn profile_meta(profile_id: &str) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert("querymt".to_string(), json!({ "profile_id": profile_id }));
    meta
}

pub fn session_summary(session: &acp::SessionInfo) -> Value {
    json!({
        "sessionId": session.session_id.to_string(),
        "cwd": path_string(&session.cwd),
        "title": session.title,
        "updatedAt": session.updated_at,
        "meta": session.meta,
    })
}

pub fn config_status(
    session_id: &str,
    modes: Option<&acp::SessionModeState>,
    config_options: Option<&[acp::SessionConfigOption]>,
) -> Value {
    let options = config_options
        .map(config_values)
        .unwrap_or_else(|| json!({}));
    json!({
        "sessionId": session_id,
        "mode": modes.map(|state| state.current_mode_id.to_string()),
        "profile": options.get("profile").cloned(),
        "model": options.get("model").cloned(),
        "reasoningEffort": options.get("reasoning_effort").cloned(),
        "config": options,
    })
}

pub fn config_values(options: &[acp::SessionConfigOption]) -> Value {
    let mut values = Map::new();
    if let Ok(Value::Array(items)) = serde_json::to_value(options) {
        for option in items {
            let Some(id) = option.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Some(current) = current_config_value(&option) {
                values.insert(id.to_string(), current);
            }
        }
    }
    Value::Object(values)
}

fn current_config_value(option: &Value) -> Option<Value> {
    option
        .get("currentValue")
        .cloned()
        .or_else(|| option.get("value").cloned())
        .filter(|value| !value.is_null())
}

pub fn compact_inspect(
    session_id: &str,
    cwd: &Path,
    modes: Option<&acp::SessionModeState>,
    config_options: Option<&[acp::SessionConfigOption]>,
    load_value: &Value,
) -> Value {
    let snapshot = session_load_snapshot(load_value);
    json!({
        "sessionId": session_id,
        "cwd": path_string(cwd),
        "status": config_status(session_id, modes, config_options),
        "messages": compact_messages(snapshot),
        "tools": compact_tools(snapshot),
        "errors": compact_errors(snapshot),
    })
}

fn session_load_snapshot(response: &Value) -> Option<&Value> {
    response
        .get("_meta")
        .or_else(|| response.get("meta"))
        .and_then(|meta| meta.get(SESSION_LOAD_SNAPSHOT_META_KEY))
}

fn compact_messages(snapshot: Option<&Value>) -> Vec<Value> {
    let Some(events) = snapshot
        .and_then(|snapshot| snapshot.get("audit"))
        .and_then(|audit| audit.get("events"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut messages = Vec::new();
    for event in events {
        let Some(kind) = event.get("kind") else {
            continue;
        };
        let Some(kind_type) = kind.get("type").and_then(Value::as_str) else {
            continue;
        };
        let data = kind.get("data").unwrap_or(&Value::Null);
        match kind_type {
            "prompt_received" => messages.push(json!({
                "role": "user",
                "messageId": string_field(data, "message_id"),
                "text": truncate_text(&content_text(data.get("content").unwrap_or(&Value::Null))),
            })),
            "assistant_message_stored" => messages.push(json!({
                "role": "assistant",
                "messageId": string_field(data, "message_id"),
                "text": truncate_text(&string_field(data, "content").unwrap_or_default()),
                "thinking": string_field(data, "thinking").filter(|text| !text.is_empty()),
            })),
            _ => {}
        }
    }
    messages
}

fn compact_tools(snapshot: Option<&Value>) -> Vec<Value> {
    let Some(events) = snapshot
        .and_then(|snapshot| snapshot.get("audit"))
        .and_then(|audit| audit.get("events"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    events
        .iter()
        .filter_map(|event| {
            let kind = event.get("kind")?;
            let kind_type = kind.get("type").and_then(Value::as_str)?;
            let data = kind.get("data").unwrap_or(&Value::Null);
            match kind_type {
                "tool_call_start" => Some(json!({
                    "id": string_field(data, "tool_call_id"),
                    "name": string_field(data, "tool_name").unwrap_or_else(|| "tool".to_string()),
                    "status": "started",
                    "arguments": data.get("arguments").or_else(|| data.get("input")).cloned(),
                })),
                "tool_call_end" => Some(json!({
                    "id": string_field(data, "tool_call_id"),
                    "name": string_field(data, "tool_name").unwrap_or_else(|| "tool".to_string()),
                    "status": if data.get("is_error").and_then(Value::as_bool).unwrap_or(false) {
                        "error"
                    } else {
                        "ok"
                    },
                    "result": truncate_opt(string_field(data, "result")
                        .or_else(|| string_field(data, "output"))
                        .or_else(|| string_field(data, "content"))),
                })),
                _ => None,
            }
        })
        .collect()
}

fn compact_errors(snapshot: Option<&Value>) -> Vec<Value> {
    let Some(events) = snapshot
        .and_then(|snapshot| snapshot.get("audit"))
        .and_then(|audit| audit.get("events"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    events
        .iter()
        .filter_map(|event| {
            let kind = event.get("kind")?;
            let kind_type = kind.get("type").and_then(Value::as_str)?;
            if kind_type != "error" && kind_type != "cancelled" {
                return None;
            }
            Some(json!({
                "type": kind_type,
                "data": kind.get("data").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect()
}

pub fn matches_query(session: &Value, query: &str) -> bool {
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    for key in ["sessionId", "title", "cwd"] {
        if session
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|value| value.to_ascii_lowercase().contains(&query))
        {
            return true;
        }
    }
    false
}

pub fn find_page_cap() -> usize {
    FIND_PAGE_CAP
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn string_field(data: &Value, key: &str) -> Option<String> {
    match data.get(key)? {
        Value::String(text) => Some(text.clone()),
        Value::Null => None,
        other => serde_json::to_string(other).ok(),
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| item.as_str().map(str::to_string))
            })
            .collect::<Vec<_>>()
            .join(""),
        Value::Object(_) => content
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn truncate_text(text: &str) -> String {
    if text.chars().count() <= INSPECT_TEXT_LIMIT {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(INSPECT_TEXT_LIMIT).collect();
        format!("{truncated}…")
    }
}

fn truncate_opt(text: Option<String>) -> Option<String> {
    text.map(|text| truncate_text(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_inspect_extracts_user_and_tool_events() {
        let load = json!({
            "_meta": {
                "querymt/sessionLoadSnapshot.v1": {
                    "audit": {
                        "events": [
                            {"kind": {"type": "prompt_received", "data": {
                                "message_id": "m1",
                                "content": [{"type": "text", "text": "hello"}]
                            }}},
                            {"kind": {"type": "tool_call_end", "data": {
                                "tool_call_id": "t1",
                                "tool_name": "read",
                                "result": "ok"
                            }}},
                            {"kind": {"type": "assistant_message_stored", "data": {
                                "message_id": "m2",
                                "content": "done"
                            }}}
                        ]
                    }
                }
            }
        });
        let compact = compact_inspect("s1", Path::new("/repo"), None, None, &load);
        assert_eq!(compact["messages"][0]["role"], "user");
        assert_eq!(compact["messages"][0]["text"], "hello");
        assert_eq!(compact["tools"][0]["name"], "read");
        assert_eq!(compact["messages"][1]["text"], "done");
    }

    #[test]
    fn find_matches_id_title_or_cwd() {
        let session = json!({
            "sessionId": "abc-123",
            "title": "Fix build",
            "cwd": "/tmp/project"
        });
        assert!(matches_query(&session, "abc"));
        assert!(matches_query(&session, "build"));
        assert!(matches_query(&session, "project"));
        assert!(!matches_query(&session, "missing"));
    }
}
