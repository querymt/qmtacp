use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{Value, json};

use crate::error::{CliError, ExitCode};

pub fn write_json(value: &impl Serialize, pretty: bool) -> Result<()> {
    let text = if pretty {
        serde_json::to_string_pretty(value).context("serialize JSON")?
    } else {
        serde_json::to_string(value).context("serialize JSON")?
    };
    println!("{text}");
    Ok(())
}

pub fn write_event(value: &impl Serialize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(value).context("serialize event")?
    );
    Ok(())
}

pub fn write_error(error: &CliError, pretty: bool) -> Result<()> {
    write_json(
        &json!({
            "ok": false,
            "error": error.to_string(),
            "code": error.exit_code().as_i32(),
        }),
        pretty,
    )
}

pub fn ok(mut value: Value) -> Value {
    if let Some(object) = value.as_object_mut() {
        object.insert("ok".to_string(), Value::Bool(true));
    }
    value
}

pub fn exit_for_stop_reason(stop_reason: &str) -> ExitCode {
    match stop_reason {
        "cancelled" => ExitCode::Timeout,
        _ => ExitCode::Ok,
    }
}
