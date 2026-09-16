use std::io::{self, Read};
use std::path::PathBuf;
use std::time::Duration;

use agent_client_protocol::schema::v1 as acp;
use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde_json::{Value, json};

use crate::client::AcpClient;
use crate::error::{CliError, ExitCode};
use crate::output;
use crate::session;

#[derive(Subcommand)]
pub enum Command {
    /// Protocol version, agent info, ACP session features, and QueryMT capabilities.
    Caps,
    /// List QueryMT profiles advertised by the server.
    Profiles,
    /// List models advertised by the server.
    Models {
        #[arg(long)]
        refresh: bool,
    },
    /// Create a session.
    New {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
    },
    /// List sessions with ACP pagination.
    Sessions {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        cursor: Option<String>,
        /// Follow nextCursor until exhausted or the page cap is reached.
        #[arg(long)]
        all: bool,
    },
    /// Client-side search over session/list pages.
    Find {
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Compact session history from session/load.
    Inspect {
        session_id: String,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        full: bool,
    },
    /// Current mode/model/profile/effort for a session.
    Status {
        session_id: String,
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Set session mode (build|plan|review).
    SetMode { session_id: String, mode: String },
    /// Set session model id.
    SetModel { session_id: String, model: String },
    /// Set reasoning effort (auto|low|medium|high|max).
    SetEffort { session_id: String, effort: String },
    /// Send a prompt. `prompt --new TEXT` or `prompt SESSION_ID TEXT`.
    Prompt {
        /// Create a session, then prompt. Remaining args are the prompt text.
        #[arg(long)]
        new: bool,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
        /// `SESSION_ID TEXT` or, with --new, just `TEXT`. Use `-` to read stdin.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Cancel the current turn for a session.
    Cancel { session_id: String },
    /// Close a materialized session.
    Close { session_id: String },
}

pub struct CommandOutcome {
    pub exit: ExitCode,
}

pub async fn run(
    client: &AcpClient,
    command: Command,
    pretty: bool,
) -> Result<CommandOutcome, CliError> {
    match command {
        Command::Caps => write_ok(pretty, caps(client).await?),
        Command::Profiles => write_ok(pretty, profiles(client).await?),
        Command::Models { refresh } => write_ok(pretty, models(client, refresh).await?),
        Command::New {
            cwd,
            profile,
            mode,
            model,
            effort,
        } => write_ok(
            pretty,
            new_session(client, cwd, profile, mode, model, effort).await?,
        ),
        Command::Sessions { cwd, cursor, all } => {
            write_ok(pretty, sessions(client, cwd, cursor, all).await?)
        }
        Command::Find { cwd, query, limit } => {
            write_ok(pretty, find(client, cwd, query, limit).await?)
        }
        Command::Inspect {
            session_id,
            cwd,
            full,
        } => write_ok(pretty, inspect(client, session_id, cwd, full).await?),
        Command::Status { session_id, cwd } => {
            write_ok(pretty, status(client, session_id, cwd).await?)
        }
        Command::SetMode { session_id, mode } => {
            write_ok(pretty, set_mode(client, session_id, mode).await?)
        }
        Command::SetModel { session_id, model } => {
            write_ok(pretty, set_model(client, session_id, model).await?)
        }
        Command::SetEffort { session_id, effort } => {
            write_ok(pretty, set_effort(client, session_id, effort).await?)
        }
        Command::Prompt {
            new,
            cwd,
            profile,
            mode,
            model,
            effort,
            timeout,
            args,
        } => {
            prompt(
                client,
                PromptArgs {
                    new,
                    cwd,
                    profile,
                    mode,
                    model,
                    effort,
                    timeout,
                    args,
                    pretty,
                },
            )
            .await
        }
        Command::Cancel { session_id } => write_ok(pretty, cancel(client, session_id).await?),
        Command::Close { session_id } => write_ok(pretty, close(client, session_id).await?),
    }
}

fn write_ok(pretty: bool, value: Value) -> Result<CommandOutcome, CliError> {
    output::write_json(&output::ok(value), pretty).map_err(CliError::rpc)?;
    Ok(CommandOutcome { exit: ExitCode::Ok })
}

async fn caps(client: &AcpClient) -> Result<Value, CliError> {
    let initialized = client.initialize().await.map_err(CliError::from_request)?;
    let querymt = match client.extension("querymt/capabilities", json!({})).await {
        Ok(value) => extension_payload(&value),
        Err(_) => Value::Null,
    };
    let agent = initialized.agent_info.as_ref();
    let session = &initialized.agent_capabilities.session_capabilities;
    Ok(json!({
        "protocolVersion": initialized.protocol_version,
        "agent": {
            "name": agent.map(|info| info.name.clone()),
            "title": agent.and_then(|info| info.title.clone()),
            "version": agent.map(|info| info.version.clone()),
        },
        "authMethods": initialized.auth_methods.iter().map(|method| method.id().to_string()).collect::<Vec<_>>(),
        "acp": {
            "loadSession": initialized.agent_capabilities.load_session,
            "list": session.list.is_some(),
            "resume": session.resume.is_some(),
            "close": session.close.is_some(),
            "delete": session.delete.is_some(),
        },
        "querymt": querymt,
    }))
}

async fn profiles(client: &AcpClient) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let response = client
        .extension("querymt/profiles", json!({}))
        .await
        .map_err(CliError::from_request)?;
    Ok(extension_payload(&response))
}

async fn models(client: &AcpClient, refresh: bool) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let (method, params) = if refresh {
        (
            "querymt/refreshModels",
            json!({ "wait_for_completion": true }),
        )
    } else {
        ("querymt/models", json!({}))
    };
    let response = client
        .extension(method, params)
        .await
        .map_err(CliError::from_request)?;
    Ok(extension_payload(&response))
}

async fn new_session(
    client: &AcpClient,
    cwd: Option<PathBuf>,
    profile: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let cwd = session::resolve_cwd(cwd).map_err(CliError::rpc)?;
    let created = client
        .new_session(cwd.clone(), profile.as_deref())
        .await
        .map_err(CliError::from_request)?;
    let session_id = created.session_id.to_string();
    let configured = mode.is_some() || model.is_some() || effort.is_some();
    apply_config(client, &session_id, mode, model, effort).await?;
    let status = if configured {
        open_session(client, session_id.clone(), cwd.clone())
            .await?
            .status
    } else {
        status_from_new(&created)
    };
    Ok(json!({
        "sessionId": session_id,
        "cwd": cwd,
        "status": status,
    }))
}

async fn sessions(
    client: &AcpClient,
    cwd: Option<PathBuf>,
    cursor: Option<String>,
    all: bool,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let cwd = session::resolve_optional_cwd(cwd).map_err(CliError::rpc)?;
    if !all {
        let page = client
            .list_sessions(cwd.clone(), cursor)
            .await
            .map_err(CliError::from_request)?;
        return Ok(list_page(cwd, &page));
    }

    let mut sessions = Vec::new();
    let mut next = cursor;
    let mut pages = 0usize;
    loop {
        pages += 1;
        if pages > session::find_page_cap() {
            bail_rpc("session list exceeded page cap")?;
        }
        let page = client
            .list_sessions(cwd.clone(), next.clone())
            .await
            .map_err(CliError::from_request)?;
        sessions.extend(page.sessions.iter().map(session::session_summary));
        match page.next_cursor.clone() {
            Some(cursor) => next = Some(cursor),
            None => {
                return Ok(json!({
                    "cwd": cwd,
                    "sessions": sessions,
                    "nextCursor": Value::Null,
                }));
            }
        }
    }
}

async fn find(
    client: &AcpClient,
    cwd: Option<PathBuf>,
    query: Option<String>,
    limit: usize,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let cwd = session::resolve_optional_cwd(cwd).map_err(CliError::rpc)?;
    let query = query.unwrap_or_default();
    let limit = limit.max(1);
    let mut matches = Vec::new();
    let mut next = None;
    let mut pages = 0usize;
    loop {
        pages += 1;
        if pages > session::find_page_cap() {
            break;
        }
        let page = client
            .list_sessions(cwd.clone(), next.clone())
            .await
            .map_err(CliError::from_request)?;
        for session in page.sessions {
            let summary = session::session_summary(&session);
            if session::matches_query(&summary, &query) {
                matches.push(summary);
                if matches.len() >= limit {
                    return Ok(json!({
                        "query": query,
                        "cwd": cwd,
                        "sessions": matches,
                    }));
                }
            }
        }
        match page.next_cursor {
            Some(cursor) => next = Some(cursor),
            None => break,
        }
    }
    Ok(json!({
        "query": query,
        "cwd": cwd,
        "sessions": matches,
    }))
}

async fn inspect(
    client: &AcpClient,
    session_id: String,
    cwd: Option<PathBuf>,
    full: bool,
) -> Result<Value, CliError> {
    let initialized = client.initialize().await.map_err(CliError::from_request)?;
    if !initialized.agent_capabilities.load_session {
        return Err(CliError::rpc("agent does not support session/load"));
    }
    let cwd = session::resolve_cwd(cwd).map_err(CliError::rpc)?;
    let loaded = client
        .load_session(session_id.clone(), cwd.clone())
        .await
        .map_err(CliError::from_request)?;
    let load_value = serde_json::to_value(&loaded).map_err(CliError::rpc)?;
    if full {
        return Ok(json!({
            "sessionId": session_id,
            "cwd": cwd,
            "status": session::config_status(
                &session_id,
                loaded.modes.as_ref(),
                loaded.config_options.as_deref(),
            ),
            "raw": load_value,
        }));
    }
    Ok(session::compact_inspect(
        &session_id,
        &cwd,
        loaded.modes.as_ref(),
        loaded.config_options.as_deref(),
        &load_value,
    ))
}

async fn status(
    client: &AcpClient,
    session_id: String,
    cwd: Option<PathBuf>,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let cwd = session::resolve_cwd(cwd).map_err(CliError::rpc)?;
    let opened = open_session(client, session_id.clone(), cwd).await?;
    Ok(opened.status)
}

async fn set_mode(client: &AcpClient, session_id: String, mode: String) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    client
        .set_mode(session_id.clone(), mode.clone())
        .await
        .map_err(CliError::from_request)?;
    Ok(json!({
        "sessionId": session_id,
        "mode": mode,
    }))
}

async fn set_model(
    client: &AcpClient,
    session_id: String,
    model: String,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let response = client
        .set_config(session_id.clone(), "model", &model)
        .await
        .map_err(CliError::from_request)?;
    Ok(json!({
        "sessionId": session_id,
        "model": model,
        "config": session::config_values(&response.config_options),
    }))
}

async fn set_effort(
    client: &AcpClient,
    session_id: String,
    effort: String,
) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    let response = client
        .set_config(session_id.clone(), "reasoning_effort", &effort)
        .await
        .map_err(CliError::from_request)?;
    Ok(json!({
        "sessionId": session_id,
        "reasoningEffort": effort,
        "config": session::config_values(&response.config_options),
    }))
}

struct PromptArgs {
    new: bool,
    cwd: Option<PathBuf>,
    profile: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    timeout: Option<u64>,
    args: Vec<String>,
    pretty: bool,
}

async fn prompt(client: &AcpClient, args: PromptArgs) -> Result<CommandOutcome, CliError> {
    let PromptArgs {
        new,
        cwd,
        profile,
        mode,
        model,
        effort,
        timeout,
        args,
        pretty,
    } = args;
    let initialized = client.initialize().await.map_err(CliError::from_request)?;
    let cwd = session::resolve_cwd(cwd).map_err(CliError::rpc)?;
    let (session_id, text) = parse_prompt_args(new, args)?;
    let text = read_prompt_text(text).map_err(CliError::rpc)?;
    let session_id = if new {
        let created = client
            .new_session(cwd.clone(), profile.as_deref())
            .await
            .map_err(CliError::from_request)?;
        created.session_id.to_string()
    } else {
        let Some(session_id) = session_id else {
            return Err(CliError::Usage(
                "prompt requires SESSION_ID or --new".into(),
            ));
        };
        let _ = open_session(client, session_id.clone(), cwd.clone()).await?;
        session_id
    };
    apply_config(client, &session_id, mode, model, effort).await?;
    output::write_event(&json!({
        "type": "session",
        "sessionId": session_id,
        "cwd": cwd,
    }))
    .map_err(CliError::rpc)?;

    let timeout = timeout.map(Duration::from_secs);
    let response = client
        .prompt(session_id.clone(), text, timeout)
        .await
        .map_err(CliError::from_request)?;
    let stop_reason = stop_reason_name(&response.stop_reason);
    let done = json!({
        "type": "done",
        "ok": true,
        "sessionId": session_id,
        "stopReason": stop_reason,
        "loadSession": initialized.agent_capabilities.load_session,
    });
    if pretty {
        output::write_json(&done, true).map_err(CliError::rpc)?;
    } else {
        output::write_event(&done).map_err(CliError::rpc)?;
    }
    Ok(CommandOutcome {
        exit: output::exit_for_stop_reason(&stop_reason),
    })
}

async fn cancel(client: &AcpClient, session_id: String) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    client.cancel(session_id.clone()).map_err(CliError::rpc)?;
    Ok(json!({
        "sessionId": session_id,
        "cancelled": true,
    }))
}

fn parse_prompt_args(
    new: bool,
    args: Vec<String>,
) -> Result<(Option<String>, Option<String>), CliError> {
    if new {
        return Ok((None, join_prompt_args(args)));
    }
    let mut args = args.into_iter();
    let Some(session_id) = args.next() else {
        return Err(CliError::Usage(
            "prompt requires SESSION_ID TEXT, or --new TEXT".into(),
        ));
    };
    Ok((Some(session_id), join_prompt_args(args.collect())))
}

fn join_prompt_args(args: Vec<String>) -> Option<String> {
    if args.is_empty() {
        None
    } else {
        Some(args.join(" "))
    }
}

async fn close(client: &AcpClient, session_id: String) -> Result<Value, CliError> {
    client.initialize().await.map_err(CliError::from_request)?;
    client
        .close_session(session_id.clone())
        .await
        .map_err(CliError::from_request)?;
    Ok(json!({
        "sessionId": session_id,
        "closed": true,
    }))
}

async fn apply_config(
    client: &AcpClient,
    session_id: &str,
    mode: Option<String>,
    model: Option<String>,
    effort: Option<String>,
) -> Result<(), CliError> {
    if let Some(mode) = mode {
        client
            .set_mode(session_id.to_string(), mode)
            .await
            .map_err(CliError::from_request)?;
    }
    if let Some(model) = model {
        client
            .set_config(session_id.to_string(), "model", &model)
            .await
            .map_err(CliError::from_request)?;
    }
    if let Some(effort) = effort {
        client
            .set_config(session_id.to_string(), "reasoning_effort", &effort)
            .await
            .map_err(CliError::from_request)?;
    }
    Ok(())
}

struct OpenedSession {
    status: Value,
}

async fn open_session(
    client: &AcpClient,
    session_id: String,
    cwd: PathBuf,
) -> Result<OpenedSession, CliError> {
    match client.resume_session(session_id.clone(), cwd.clone()).await {
        Ok(response) => Ok(OpenedSession {
            status: session::config_status(
                &session_id,
                response.modes.as_ref(),
                response.config_options.as_deref(),
            ),
        }),
        Err(_) => {
            let loaded = client
                .load_session(session_id.clone(), cwd)
                .await
                .map_err(CliError::from_request)?;
            Ok(OpenedSession {
                status: session::config_status(
                    &session_id,
                    loaded.modes.as_ref(),
                    loaded.config_options.as_deref(),
                ),
            })
        }
    }
}

fn status_from_new(response: &acp::NewSessionResponse) -> Value {
    session::config_status(
        &response.session_id.to_string(),
        response.modes.as_ref(),
        response.config_options.as_deref(),
    )
}

fn list_page(cwd: Option<PathBuf>, page: &acp::ListSessionsResponse) -> Value {
    json!({
        "cwd": cwd,
        "sessions": page.sessions.iter().map(session::session_summary).collect::<Vec<_>>(),
        "nextCursor": page.next_cursor,
    })
}

fn extension_payload(response: &Value) -> Value {
    response
        .get("data")
        .cloned()
        .unwrap_or_else(|| response.clone())
}

fn read_prompt_text(text: Option<String>) -> Result<String> {
    match text {
        Some(text) if text == "-" => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).context("read stdin")?;
            Ok(buf)
        }
        Some(text) => Ok(text),
        None => {
            if atty_stdin() {
                bail!("prompt text is required (pass TEXT or - for stdin)");
            }
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).context("read stdin")?;
            Ok(buf)
        }
    }
}

fn atty_stdin() -> bool {
    std::io::IsTerminal::is_terminal(&io::stdin())
}

fn stop_reason_name(reason: &acp::StopReason) -> String {
    match reason {
        acp::StopReason::EndTurn => "endTurn".to_string(),
        acp::StopReason::MaxTokens => "maxTokens".to_string(),
        acp::StopReason::MaxTurnRequests => "maxTurnRequests".to_string(),
        acp::StopReason::Refusal => "refusal".to_string(),
        acp::StopReason::Cancelled => "cancelled".to_string(),
        other => format!("{other:?}"),
    }
}

fn bail_rpc(message: &str) -> Result<Value, CliError> {
    Err(CliError::rpc(message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_new_uses_remaining_args_as_text() {
        let (session_id, text) =
            parse_prompt_args(true, vec!["fix".into(), "the".into(), "build".into()]).unwrap();
        assert!(session_id.is_none());
        assert_eq!(text.as_deref(), Some("fix the build"));
    }

    #[test]
    fn prompt_existing_takes_session_then_text() {
        let (session_id, text) =
            parse_prompt_args(false, vec!["sess-1".into(), "continue".into()]).unwrap();
        assert_eq!(session_id.as_deref(), Some("sess-1"));
        assert_eq!(text.as_deref(), Some("continue"));
    }

    #[test]
    fn prompt_without_session_is_usage_error() {
        assert!(matches!(
            parse_prompt_args(false, Vec::new()),
            Err(CliError::Usage(_))
        ));
    }
}
