//! 外部程序通过现有用户凭据登记工作区待办，不启动执行。
use super::{
    client_profile_runner_config, client_profile_user_token_file, post_json_authed,
    read_optional_user_api_token, ApiCall,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;
use webcodex_admin::ServerHttpOptions;

pub(crate) const USAGE: &str = "Usage: webcodex task <add|list|show|update|complete> [OPTIONS]\n\nTarget: --project <runtime-id> OR --workspace <directory> (default: current directory)\nConnection: --server-url <url> --token-file <path> --config <runner.toml> --profile <name>\nWorkspace lookup also accepts --client-id <runner-id>. Missing connection values use the selected Runner profile.\n\nadd: --title <text> [--description <text>|--description-file <path|->] --idempotency-key <key>\nlist: [--status open|in_progress|done|cancelled] [--query <title>] [--offset <n>] [--limit <1..50>]\nshow: --task-id <id>\nupdate: --task-id <id> --expected-revision <n> [--title <text>] [--description <text>|--description-file <path|->] [--status <state>]\n\nUse --json for machine-readable output. Complete: --task-id <id> --expected-revision <n> --summary <text> (or --summary-file <path|->). update also accepts these summary options. Registration never executes work. Reuse the same add key after an uncertain response; update conflicts require a fresh read.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskCommand {
    action: String,
    values: BTreeMap<String, String>,
    json: bool,
}

pub(crate) fn parse(args: &[String]) -> Result<TaskCommand, String> {
    let action = args
        .first()
        .filter(|a| matches!(a.as_str(), "add" | "list" | "show" | "update" | "complete"))
        .ok_or("Expected task add, list, show, update or complete")?
        .clone();
    let mut values = BTreeMap::new();
    let mut json = false;
    let mut args = args[1..].iter();
    while let Some(flag) = args.next() {
        if flag == "--json" {
            json = true;
            continue;
        }
        let common = matches!(
            flag.as_str(),
            "--project"
                | "--workspace"
                | "--server-url"
                | "--token-file"
                | "--config"
                | "--profile"
                | "--client-id"
        );
        let specific = match action.as_str() {
            "add" => matches!(
                flag.as_str(),
                "--title" | "--description" | "--description-file" | "--idempotency-key"
            ),
            "list" => matches!(
                flag.as_str(),
                "--status" | "--query" | "--offset" | "--limit"
            ),
            "show" => flag == "--task-id",
            "update" => matches!(
                flag.as_str(),
                "--task-id"
                    | "--expected-revision"
                    | "--title"
                    | "--description"
                    | "--description-file"
                    | "--status"
                    | "--summary"
                    | "--summary-file"
            ),
            "complete" => matches!(
                flag.as_str(),
                "--task-id" | "--expected-revision" | "--summary" | "--summary-file"
            ),
            _ => false,
        };
        if !common && !specific {
            return Err(format!("Unknown option for task {action}: {flag}"));
        }
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} requires a value"))?;
        if values.insert(flag.clone(), value.clone()).is_some() {
            return Err(format!("Duplicate option: {flag}"));
        }
    }
    for required in match action.as_str() {
        "add" => &["--title", "--idempotency-key"][..],
        "show" => &["--task-id"][..],
        "update" | "complete" => &["--task-id", "--expected-revision"][..],
        _ => &[],
    } {
        if values.get(*required).is_none_or(|v| v.trim().is_empty()) {
            return Err(format!("{required} is required"));
        }
    }
    if action == "complete"
        && !["--summary", "--summary-file"]
            .iter()
            .any(|key| values.contains_key(*key))
    {
        return Err("complete requires --summary or --summary-file".into());
    }
    if values.contains_key("--summary") && values.contains_key("--summary-file") {
        return Err("Use --summary or --summary-file, not both".into());
    }
    if values
        .get("--description-file")
        .is_some_and(|value| value == "-")
        && values
            .get("--summary-file")
            .is_some_and(|value| value == "-")
    {
        return Err("Only one field may read stdin".into());
    }
    if values.contains_key("--project")
        && (values.contains_key("--workspace") || values.contains_key("--client-id"))
    {
        return Err("Use --project or --workspace/--client-id, not both".into());
    }
    if values.contains_key("--description") && values.contains_key("--description-file") {
        return Err("Use --description or --description-file, not both".into());
    }
    for field in ["--offset", "--limit", "--expected-revision"] {
        if let Some(value) = values.get(field) {
            let number = value
                .parse::<u64>()
                .map_err(|_| format!("{field} requires a non-negative integer"))?;
            if field != "--offset" && number == 0 {
                return Err(format!("{field} must be positive"));
            }
        }
    }
    if let Some(status) = values.get("--status") {
        if !matches!(
            status.as_str(),
            "open" | "in_progress" | "done" | "cancelled"
        ) {
            return Err("Unknown task status".into());
        }
    }
    if action == "update"
        && ![
            "--title",
            "--description",
            "--description-file",
            "--status",
            "--summary",
            "--summary-file",
        ]
        .iter()
        .any(|k| values.contains_key(*k))
    {
        return Err("update requires a content or status change".into());
    }
    Ok(TaskCommand {
        action,
        values,
        json,
    })
}

#[derive(Deserialize)]
struct RunnerConfig {
    server_url: String,
    client_id: String,
}

fn output(value: Value) -> Result<Value, String> {
    if value["success"] != true {
        let code = value
            .pointer("/output/error_kind")
            .and_then(Value::as_str)
            .unwrap_or("workspace_task_request_failed");
        return Err(code.to_string());
    }
    value
        .get("output")
        .cloned()
        .ok_or_else(|| "Invalid task response".into())
}

pub(crate) async fn run(command: TaskCommand) -> Result<String, String> {
    let value = |flag: &str| command.values.get(flag).map(String::as_str);
    let profile = value("--profile").unwrap_or("default");
    super::validate_client_profile(profile)?;
    let config = if value("--config").is_some()
        || value("--server-url").is_none()
        || value("--project").is_none() && value("--client-id").is_none()
    {
        let path = value("--config")
            .map(PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| client_profile_runner_config(profile))?;
        let text = std::fs::read_to_string(path).map_err(|_| {
            "Runner config unavailable; provide --config or explicit --server-url and target"
        })?;
        Some(toml::from_str::<RunnerConfig>(&text).map_err(|_| "Runner config is invalid")?)
    } else {
        None
    };
    let server = value("--server-url")
        .or_else(|| config.as_ref().map(|c| c.server_url.as_str()))
        .ok_or("--server-url is required")?;
    let server_identity = url::Url::parse(server).map_err(|_| "Invalid Server URL")?;
    if !matches!(server_identity.scheme(), "http" | "https")
        || server_identity.host_str().is_none()
        || !server_identity.username().is_empty()
        || server_identity.password().is_some()
        || server_identity.query().is_some()
        || server_identity.fragment().is_some()
    {
        return Err(
            "Server URL must be HTTP(S) without embedded credentials, query or fragment".into(),
        );
    }
    let token_path = value("--token-file")
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| client_profile_user_token_file(profile))?;
    let token = read_optional_user_api_token(&Some(token_path), "user token")?
        .ok_or("User credential unavailable; provide --token-file")?;
    let http = ServerHttpOptions::default();
    let project = match value("--project") {
        Some(id) => id.to_string(),
        None => {
            let runner = value("--client-id")
                .or_else(|| config.as_ref().map(|c| c.client_id.as_str()))
                .ok_or("--client-id is required for workspace lookup")?;
            let path = value("--workspace")
                .map(PathBuf::from)
                .map(Ok)
                .unwrap_or_else(std::env::current_dir)
                .map_err(|_| "Cannot resolve workspace directory")?
                .canonicalize()
                .map_err(|_| "Workspace directory does not exist")?;
            let resolved = output(post_json_authed(ApiCall { server_url:server, server_http:&http, token:&token, path:"/api/tools/call", body:json!({"tool":"resolve_workspace","params":{"client_id":runner,"path":path.to_string_lossy()}}) }).await?)?;
            if resolved["resolution"] != "resolved" {
                return Err(
                    "Workspace is missing, ambiguous or incomplete; select an exact --project"
                        .into(),
                );
            }
            resolved
                .pointer("/workspace/project")
                .and_then(Value::as_str)
                .ok_or("Workspace response has no exact Project")?
                .to_string()
        }
    };
    let request = |operation: Value| json!({"project":project,"operation":operation});
    let initial = if command.action == "show"
        || command.action == "update"
        || command.action == "complete"
    {
        json!({"action":"read","task_id":value("--task-id")})
    } else if command.action == "list" {
        json!({"action":"list","query":{"status":value("--status"),"query":value("--query"),"offset":value("--offset").unwrap_or("0").parse::<u64>().map_err(|_| "Invalid offset")?,"limit":value("--limit").unwrap_or("20").parse::<u64>().map_err(|_| "Invalid limit")?}})
    } else {
        json!({"action":"list","query":{"limit":1}})
    };
    let observed = output(
        post_json_authed(ApiCall {
            server_url: server,
            server_http: &http,
            token: &token,
            path: "/api/runtime-console/workspace-tasks",
            body: request(initial),
        })
        .await?,
    )?;
    let description = read_text(value("--description"), value("--description-file"), 32000)?;
    let summary = read_text(value("--summary"), value("--summary-file"), 8000)?;
    let operation = match command.action.as_str() {
        "show" => None,
        "list" => None,
        "add" => Some(
            json!({"action":"create","title":value("--title"),"description":description.unwrap_or_default(),"idempotency_key":value("--idempotency-key"),"expected_root_fingerprint":observed["root_fingerprint"]}),
        ),
        "update" => {
            let revision = value("--expected-revision")
                .ok_or("Missing revision")?
                .parse::<u64>()
                .map_err(|_| "Invalid revision")?;
            if observed.pointer("/task/revision").and_then(Value::as_u64) != Some(revision) {
                return Err("workspace_task_revision_conflict".into());
            }
            Some(
                json!({"action":"update","task_id":value("--task-id"),"expected_revision":revision,"expected_root_fingerprint":observed["root_fingerprint"],"title":value("--title").map(Value::from).unwrap_or_else(|| observed["task"]["title"].clone()),"description":description.map(Value::from).unwrap_or_else(|| observed["task"]["description"].clone()),"completion_summary":summary.map(Value::from).unwrap_or_else(|| observed["task"]["completion_summary"].clone()),"status":value("--status").map(Value::from).unwrap_or_else(|| observed["task"]["status"].clone())}),
            )
        }
        "complete" => Some(
            json!({"action":"complete","task_id":value("--task-id"),"expected_revision":value("--expected-revision").ok_or("Missing revision")?.parse::<u64>().map_err(|_| "Invalid revision")?,"expected_root_fingerprint":observed["root_fingerprint"],"completion_summary":summary.unwrap_or_default()}),
        ),
        _ => return Err("Unknown task command".into()),
    };
    let result = if let Some(operation) = operation {
        output(
            post_json_authed(ApiCall {
                server_url: server,
                server_http: &http,
                token: &token,
                path: "/api/runtime-console/workspace-tasks",
                body: request(operation),
            })
            .await?,
        )?
    } else {
        observed
    };
    if command.json {
        serde_json::to_string_pretty(&result).map_err(|_| "Cannot encode task output".into())
    } else if let Some(tasks) = result["tasks"].as_array() {
        Ok(format!(
            "{}\nnext_offset: {}",
            tasks
                .iter()
                .map(|t| format!(
                    "{}\t{}\trev {}\t{}",
                    t["task_id"].as_str().unwrap_or_default(),
                    t["status"].as_str().unwrap_or_default(),
                    t["revision"],
                    t["title"].as_str().unwrap_or_default()
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            result["next_offset"]
        ))
    } else {
        Ok(format!(
            "{}\n{}\nstatus: {} · revision: {}\n{}\n\nCompletion summary:\n{}",
            result["task"]["task_id"].as_str().unwrap_or_default(),
            result["task"]["title"].as_str().unwrap_or_default(),
            result["task"]["status"],
            result["task"]["revision"],
            result["task"]["description"].as_str().unwrap_or_default(),
            result["task"]["completion_summary"]
                .as_str()
                .unwrap_or_default()
        ))
    }
}

fn read_text(
    text: Option<&str>,
    file: Option<&str>,
    max_bytes: u64,
) -> Result<Option<String>, String> {
    if let Some(path) = file {
        let mut bytes = Vec::new();
        if path == "-" {
            std::io::stdin()
                .take(max_bytes + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read task text")?;
        } else {
            std::fs::File::open(path)
                .map_err(|_| "Cannot open task text file")?
                .take(max_bytes + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "Cannot read task text")?;
        }
        if bytes.len() as u64 > max_bytes {
            return Err(format!("Task text exceeds {max_bytes} bytes"));
        }
        Ok(Some(
            String::from_utf8(bytes).map_err(|_| "Task text must be UTF-8")?,
        ))
    } else {
        Ok(text.map(str::to_string))
    }
}

#[cfg(test)]
#[path = "tests/tasks.rs"]
mod tests;
