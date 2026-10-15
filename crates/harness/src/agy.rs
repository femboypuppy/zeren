use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};
mod approval;
pub use approval::hook_command as permission_hook_command;
pub use approval::run_hook as run_permission_hook;

use async_trait::async_trait;
use futures::{StreamExt, stream::BoxStream};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};
use zeren_proto::{
    AgentEvent, DoneStatus, HarnessId, Model, ModelOption, ModelOptionChoice, ReasoningLevel,
    RunRequest, SandboxLevel, SteeringMode, ToolCall,
};

use crate::{
    Harness, HarnessError, RunControls, StderrTail,
    process::{Child, Command, Stdio},
};

pub struct AgyCliHarness {
    executable: Option<PathBuf>,
    models_cache: crate::catalog::Catalog,
}

const CLI_SESSION_PREFIX: &str = "agy-cli:";

impl AgyCliHarness {
    pub fn new() -> Self {
        Self {
            executable: None,
            models_cache: crate::catalog::Catalog::default(),
        }
    }

    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = Some(executable.into());
        self
    }

    pub fn resolve_executable(&self) -> Result<PathBuf, HarnessError> {
        if let Some(path) = self
            .executable
            .clone()
            .or_else(|| std::env::var_os("AGY_EXECUTABLE").map(PathBuf::from))
        {
            return crate::executable::validate_native_override(&path);
        }
        let mut extra = vec![
            PathBuf::from("/opt/homebrew/bin/agy"),
            PathBuf::from("/usr/local/bin/agy"),
        ];
        if let Some(home) = crate::executable::home_dir() {
            extra.push(home.join(".local/bin/agy"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            extra.push(PathBuf::from(local).join("agy/bin/agy.exe"));
        }
        crate::executable::find_on_paths("agy", extra).ok_or_else(|| {
            HarnessError::NotInstalled("Install Antigravity CLI (agy) or set AGY_EXECUTABLE".into())
        })
    }

    fn command(&self, cwd: &Path) -> Result<Command, HarnessError> {
        let executable = self.resolve_executable()?;
        let mut command = Command::new(&executable);
        crate::process::owned::configure(&mut command);
        crate::compose_child_path(&mut command, &executable);
        command.current_dir(cwd).kill_on_drop(true);
        Ok(command)
    }

    async fn available_models(&self) -> Result<Vec<Model>, HarnessError> {
        let mut command = self.command(&std::env::current_dir()?)?;
        command.arg("models").stdin(Stdio::null());
        let output = tokio::time::timeout(Duration::from_secs(30), command.output())
            .await
            .map_err(|_| HarnessError::Protocol("agy models timed out".into()))??;
        if !output.status.success() {
            return Err(HarnessError::Protocol(format!(
                "agy models failed: {}",
                crate::redact_secrets(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        let models = parse_models(&String::from_utf8_lossy(&output.stdout));
        if models.is_empty() {
            return Err(HarnessError::Protocol(
                "agy models returned no usable models".into(),
            ));
        }
        Ok(models)
    }

    async fn cached_models(&self, force: bool) -> Result<crate::ModelCatalog, HarnessError> {
        self.models_cache
            .get_with_timeout(
                force,
                Duration::from_secs(30),
                || self.model_context().map(|context| context.unwrap().key()),
                || self.available_models(),
            )
            .await
    }

    /// Fetch native account quotas without invoking a model turn.
    pub async fn usage(&self) -> Result<Value, HarnessError> {
        let mut command = self.command(&std::env::current_dir()?)?;
        command.args(["-p", "/usage", "--output-format", "json"]);
        command.stdin(Stdio::null());
        let output = tokio::time::timeout(Duration::from_secs(30), command.output())
            .await
            .map_err(|_| HarnessError::Protocol("Antigravity usage timed out".into()))??;
        let frame: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
            HarnessError::Protocol("Antigravity returned invalid usage data".into())
        })?;
        if !output.status.success() || frame["status"] != "SUCCESS" {
            return Err(HarnessError::Protocol(crate::redact_secrets(
                frame["error"]
                    .as_str()
                    .unwrap_or("Antigravity usage unavailable"),
            )));
        }
        Ok(frame)
    }
}

impl Default for AgyCliHarness {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_models(output: &str) -> Vec<Model> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, char::is_whitespace);
            let id = parts.next()?.trim();
            let label = parts.next()?.trim();
            if id.is_empty() || label.is_empty() || id.chars().any(char::is_control) {
                return None;
            }
            Some(Model {
                id: id.into(),
                label: label.into(),
                description: None,
                reasoning_levels: Vec::new(),
                options: vec![permission_option()],
            })
        })
        .collect()
}

fn effort(level: ReasoningLevel) -> &'static str {
    match level {
        ReasoningLevel::Minimal | ReasoningLevel::Low => "low",
        ReasoningLevel::Medium => "medium",
        ReasoningLevel::High | ReasoningLevel::Ultrathink => "high",
        ReasoningLevel::XHigh | ReasoningLevel::Ultracode => "xhigh",
        ReasoningLevel::Max | ReasoningLevel::Ultra => "max",
    }
}

fn permission_option() -> ModelOption {
    ModelOption {
        id: "skipPermissions".into(),
        label: "Skip permission checks".into(),
        choices: vec![
            ModelOptionChoice {
                id: "off".into(),
                label: "Off".into(),
            },
            ModelOptionChoice {
                id: "on".into(),
                label: "On".into(),
            },
        ],
        default_choice: "off".into(),
    }
}

fn skip_permissions(request: &RunRequest) -> bool {
    request
        .model_options
        .get("skipPermissions")
        .and_then(Value::as_str)
        == Some("on")
        || (request.auto_approve && request.sandbox == SandboxLevel::DangerFullAccess)
}

fn model_base(requested: &str) -> &str {
    requested
        .rsplit_once('-')
        .filter(|(_, suffix)| matches!(*suffix, "low" | "medium" | "high" | "xhigh" | "max"))
        .map_or(requested, |(base, _)| base)
}

fn selected_model(
    requested: Option<&str>,
    reasoning: Option<ReasoningLevel>,
    available: &[Model],
) -> Option<String> {
    let requested = requested?;
    if let Some(variant) = crate::acp::grouped_effort_variant(available, requested, reasoning) {
        return Some(variant);
    }
    let level = reasoning.map(effort);
    let base = model_base(requested);
    if base != requested && available.iter().any(|model| model.id == requested) {
        return Some(requested.into());
    }
    if let Some(level) = level {
        let variant = format!("{base}-{level}");
        if available.iter().any(|model| model.id == variant) {
            return Some(variant);
        }
    }
    if available.iter().any(|model| model.id == requested) {
        return Some(requested.into());
    }
    let prefix = format!("{base}-");
    let strongest = format!("{base}-high");
    available
        .iter()
        .find(|model| model.id == strongest)
        .or_else(|| available.iter().find(|model| model.id.starts_with(&prefix)))
        .map(|model| model.id.clone())
        .or_else(|| Some(requested.into()))
}

#[async_trait]
impl Harness for AgyCliHarness {
    fn id(&self) -> HarnessId {
        HarnessId::Antigravity
    }

    fn display_name(&self) -> &str {
        "Antigravity"
    }

    fn supports_steering(&self) -> bool {
        false
    }

    fn steering_mode(&self) -> SteeringMode {
        SteeringMode::TurnBoundary
    }

    fn reasoning_levels(&self) -> &[ReasoningLevel] {
        &[]
    }

    fn installed(&self) -> bool {
        self.resolve_executable().is_ok()
    }

    fn executable_path(&self) -> Option<PathBuf> {
        self.resolve_executable().ok()
    }

    fn authoritative_prompt_end(&self) -> bool {
        true
    }

    async fn commands(&self) -> Result<Vec<zeren_proto::SlashCommand>, HarnessError> {
        Ok(vec![zeren_proto::SlashCommand {
            name: "usage".into(),
            description: "Show Antigravity model quotas and reset times".into(),
            input_hint: None,
        }])
    }

    fn model_context(&self) -> Result<Option<crate::ModelContext>, HarnessError> {
        let home = crate::executable::home_or_current_dir();
        crate::model_context::context(
            HarnessId::Antigravity,
            &self.resolve_executable()?,
            &[home.join(".gemini/antigravity-cli/settings.json")],
        )
        .map(Some)
    }

    async fn models(&self) -> Result<Vec<Model>, HarnessError> {
        self.model_catalog(false)
            .await
            .map(|catalog| catalog.models)
    }

    async fn model_catalog(&self, force: bool) -> Result<crate::ModelCatalog, HarnessError> {
        let mut catalog = self.cached_models(force).await?;
        catalog.models = crate::acp::group_effort_variants(catalog.models);
        Ok(catalog)
    }

    async fn run(
        &self,
        request: RunRequest,
        controls: RunControls,
    ) -> Result<BoxStream<'static, Result<AgentEvent, HarnessError>>, HarnessError> {
        if matches!(request.prompt.trim(), "/usage" | "/quota") {
            let frame = self.usage().await?;
            return Ok(futures::stream::iter(vec![
                Ok(AgentEvent::TextDelta {
                    text: frame["response"].as_str().unwrap_or_default().to_owned(),
                }),
                Ok(AgentEvent::Done {
                    status: DoneStatus::Completed,
                    result: None,
                    error: None,
                    session_id: None,
                }),
            ])
            .boxed());
        }
        if request
            .resume
            .as_deref()
            .is_some_and(|id| !id.starts_with(CLI_SESSION_PREFIX))
        {
            return crate::AcpHarness::antigravity()
                .run(request, controls)
                .await;
        }
        let available = if request.model.as_deref().is_some_and(|requested| {
            model_base(requested) == requested || request.reasoning.is_some()
        }) {
            self.cached_models(false)
                .await
                .map(|catalog| catalog.models)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let model = selected_model(request.model.as_deref(), request.reasoning, &available);
        let mut command = self.command(Path::new(&request.cwd))?;
        command.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--print-timeout",
            "0",
        ]);
        if let Some(model) = &model {
            command.arg("--model").arg(model);
        } else if let Some(reasoning) = request.reasoning {
            command.args(["--effort", effort(reasoning)]);
        }
        if let Some(resume) = request
            .resume
            .as_deref()
            .and_then(|id| id.strip_prefix(CLI_SESSION_PREFIX))
        {
            command.arg("--conversation").arg(resume);
        }
        match request.sandbox {
            SandboxLevel::ReadOnly => {
                command.args(["--mode", "plan"]);
            }
            SandboxLevel::WorkspaceWrite | SandboxLevel::DangerFullAccess => {
                command.args(["--mode", "accept-edits"]);
            }
        }
        if skip_permissions(&request) {
            command.arg("--dangerously-skip-permissions");
        }
        if request.sandbox != SandboxLevel::DangerFullAccess {
            command.arg("--sandbox");
        }
        let approval = if skip_permissions(&request) {
            None
        } else {
            Some(approval::Bridge::prepare(&mut command).await?)
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn()?;
        let (sender, receiver) = mpsc::channel(256);
        tokio::spawn(run_child(child, request, controls, sender, approval));
        Ok(
            futures::stream::unfold(receiver, |mut receiver| async move {
                receiver.recv().await.map(|event| (event, receiver))
            })
            .boxed(),
        )
    }
}

async fn run_child(
    mut child: Child,
    request: RunRequest,
    controls: RunControls,
    sender: mpsc::Sender<Result<AgentEvent, HarnessError>>,
    approval: Option<approval::Bridge>,
) {
    let RunControls {
        execution_lease: _lease,
        interrupt,
        steering: _steering,
        request_input,
        realtime: _realtime,
    } = controls;
    let _approval_task = approval.map(|bridge| bridge.serve(request_input, interrupt.clone()));
    let mut session = request
        .resume
        .as_deref()
        .and_then(|id| id.strip_prefix(CLI_SESSION_PREFIX))
        .unwrap_or_default()
        .to_owned();
    let mut started = false;
    let mut finished = false;
    let mut streamed = String::new();
    let mut tools = HashSet::new();
    let mut completed_tools = HashSet::new();
    let stderr_tail = StderrTail::default();
    let stderr = child.stderr.take().expect("piped stderr");
    let tail = stderr_tail.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tail.push(&line);
        }
    });
    let prompt = json!({"event":"user","message":{"content":&request.prompt}}).to_string();
    let mut stdin = child.stdin.take().expect("piped stdin");
    if stdin.write_all(prompt.as_bytes()).await.is_err() || stdin.write_all(b"\n").await.is_err() {
        let _ = sender
            .send(Ok(AgentEvent::Done {
                status: DoneStatus::Errored,
                result: None,
                error: Some("could not send the prompt to agy".into()),
                session_id: None,
            }))
            .await;
        let _ = child.start_kill();
        let _ = child.wait().await;
        stderr_task.abort();
        return;
    }
    drop(stdin);
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    loop {
        tokio::select! {
            _ = interrupt.cancelled() => {
                let _ = child.start_kill();
                let _ = sender.send(Ok(AgentEvent::Done {
                    status: DoneStatus::Interrupted,
                    result: None,
                    error: None,
                    session_id: session_id(&session),
                })).await;
                finished = true;
                break;
            }
            _ = sender.closed() => {
                let _ = child.start_kill();
                break;
            }
            line = lines.next_line() => {
                let Ok(Some(line)) = line else { break };
                let Ok(frame) = serde_json::from_str::<Value>(&line) else { continue };
                if let Some(id) = frame["conversation_id"].as_str() {
                    session = id.to_owned();
                }
                match frame["event"].as_str() {
                    Some("init") => {
                        if !started {
                            let _ = sender.send(Ok(AgentEvent::SessionStarted {
                                harness: HarnessId::Antigravity,
                                model: model_name(&request, &frame),
                                tools: frame["init"]["tools"].as_array().map(|tools| {
                                    tools.iter().filter_map(Value::as_str).map(str::to_owned).collect()
                                }).unwrap_or_default(),
                                cwd: request.cwd.clone(),
                                session_id: session_id(&session).unwrap_or_default(),
                                assistant_message_id: uuid::Uuid::new_v4().to_string(),
                            })).await;
                            started = true;
                        }
                    }
                    Some("step_update") => {
                        let step = &frame["step_update"];
                        if step["step_type"] == "agent_response" {
                            if let Some(delta) = step["text_delta"].as_str().filter(|delta| !delta.is_empty()) {
                                streamed.push_str(delta);
                                let _ = sender.send(Ok(AgentEvent::TextDelta { text: delta.into() })).await;
                            }
                        } else if step["step_type"] == "tool" {
                            emit_tool(step, &mut tools, &mut completed_tools, &sender).await;
                        }
                    }
                    Some("result") => {
                        let result = &frame["result"];
                        if let Some(id) = result["conversation_id"].as_str() {
                            session = id.to_owned();
                        }
                        let response = result["response"].as_str().unwrap_or_default();
                        if let Some(remainder) = response.strip_prefix(&streamed).filter(|text| !text.is_empty()) {
                            let _ = sender.send(Ok(AgentEvent::TextDelta { text: remainder.into() })).await;
                        }
                        let (status, error) = terminal_outcome(result, &streamed, !tools.is_empty());
                        let _ = sender.send(Ok(AgentEvent::Done {
                            status,
                            result: None,
                            error,
                            session_id: session_id(&session),
                        })).await;
                        finished = true;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    drop(lines);
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => Some(status),
        _ => {
            let _ = child.start_kill();
            child.wait().await.ok()
        }
    };
    let mut stderr_task = stderr_task;
    if tokio::time::timeout(Duration::from_secs(1), &mut stderr_task)
        .await
        .is_err()
    {
        stderr_task.abort();
    }
    if !finished && !sender.is_closed() {
        let _ = sender
            .send(Ok(AgentEvent::Done {
                status: DoneStatus::Errored,
                result: None,
                error: Some(crate::crash_message(
                    "Antigravity CLI",
                    status,
                    &stderr_tail,
                )),
                session_id: session_id(&session),
            }))
            .await;
    }
}

fn session_id(id: &str) -> Option<String> {
    (!id.is_empty()).then(|| format!("{CLI_SESSION_PREFIX}{id}"))
}

fn terminal_outcome(
    result: &Value,
    streamed: &str,
    has_tools: bool,
) -> (DoneStatus, Option<String>) {
    let response = result["response"].as_str().unwrap_or_default();
    let status = match result["status"].as_str() {
        Some("SUCCESS")
            if response.trim().is_empty() && streamed.trim().is_empty() && !has_tools =>
        {
            DoneStatus::Errored
        }
        Some("SUCCESS") => DoneStatus::Completed,
        Some("CANCELED" | "INTERRUPTED") => DoneStatus::Interrupted,
        _ => DoneStatus::Errored,
    };
    let error = (status == DoneStatus::Errored).then(|| crate::redact_secrets(
        result["error"].as_str().filter(|error| !error.trim().is_empty()).unwrap_or(
            "Antigravity ended the turn without a reply. Retry or check /usage for this model's quota."
        ),
    ));
    (status, error)
}

fn model_name(request: &RunRequest, frame: &Value) -> String {
    frame["init"]["model"]
        .as_str()
        .or(request.model.as_deref())
        .unwrap_or("default")
        .into()
}

async fn emit_tool(
    step: &Value,
    seen: &mut HashSet<String>,
    completed: &mut HashSet<String>,
    sender: &mpsc::Sender<Result<AgentEvent, HarnessError>>,
) {
    let Some(index) = step["step_index"].as_u64() else {
        return;
    };
    let id = format!("agy-step-{index}");
    let info = &step["tool_info"];
    let name = info["name"]
        .as_str()
        .or_else(|| step["tool_name"].as_str())
        .unwrap_or("tool");
    if seen.insert(id.clone()) {
        let call = cli_tool_call(name, info.get("parameters"));
        let _ = sender
            .send(Ok(AgentEvent::ToolCall {
                id: id.clone(),
                call,
            }))
            .await;
    }
    let state = step["state"].as_str();
    if matches!(state, Some("DONE" | "ERROR" | "FAILED" | "CANCELED"))
        && completed.insert(id.clone())
    {
        let error = &info["error"];
        let is_error = state != Some("DONE") || !error.is_null();
        let output = info["output"]
            .as_str()
            .filter(|output| !output.trim().is_empty())
            .or_else(|| error["message"].as_str())
            .or_else(|| error.as_str())
            .map(|output| {
                let output = if is_error {
                    crate::redact_secrets(output)
                } else {
                    output.to_owned()
                };
                let output = if output.contains("permission check failed") {
                    format!(
                        "Antigravity CLI denied this command in headless mode. Configure a scoped command allow rule in agy's /permissions, then retry.\n\n{output}"
                    )
                } else {
                    output
                };
                output.chars().take(16_000).collect()
            })
            .or_else(|| is_error.then(|| format!("Antigravity tool {name} failed.")));
        let _ = sender
            .send(Ok(AgentEvent::ToolResult {
                id,
                is_error,
                output,
                diff: None,
            }))
            .await;
    }
}

fn cli_tool_call(name: &str, parameters: Option<&Value>) -> ToolCall {
    if name == "run_command"
        && let Some(command) = parameters
            .and_then(|parameters| parameters["CommandLine"].as_str())
            .filter(|command| !command.is_empty())
    {
        return ToolCall::Exec {
            command: command.into(),
        };
    }
    ToolCall::Unknown {
        name: name.into(),
        input: parameters.cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_choice_survives_effort_grouping_and_request_round_trip() {
        let models = crate::acp::group_effort_variants(parse_models(
            "gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\ngemini-3.8-flash-high\tGemini 3.8 Flash (High)\n",
        ));
        assert_eq!(models.len(), 1);
        let option = &models[0].options[0];
        assert_eq!(option.id, "skipPermissions");
        assert_eq!(option.default_choice, "off");
        let mut request: RunRequest = serde_json::from_value(json!({
            "prompt":"hello", "model":null, "reasoning":null,
            "cwd":".", "sandbox":"workspace-write", "resume":null
        }))
        .unwrap();
        assert!(!skip_permissions(&request));
        request.model_options.insert(option.id.clone(), json!("on"));
        let round_trip: RunRequest =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        assert!(skip_permissions(&round_trip));
        assert_eq!(round_trip.sandbox, SandboxLevel::WorkspaceWrite);
        request
            .model_options
            .insert(option.id.clone(), json!("off"));
        assert!(!skip_permissions(&request));
    }

    #[test]
    fn empty_turns_have_visible_errors_and_quota_errors_are_preserved() {
        let (status, error) = terminal_outcome(
            &json!({"status":"SUCCESS","response":"","error":""}),
            "",
            false,
        );
        assert_eq!(status, DoneStatus::Errored);
        assert!(error.unwrap().contains("without a reply"));
        let (status, error) = terminal_outcome(
            &json!({"status":"ERROR","error":"Individual quota reached"}),
            "",
            false,
        );
        assert_eq!(status, DoneStatus::Errored);
        assert_eq!(error.as_deref(), Some("Individual quota reached"));
        assert_eq!(
            terminal_outcome(&json!({"status":"SUCCESS","response":""}), "OK", false).0,
            DoneStatus::Completed
        );
        assert_eq!(
            terminal_outcome(&json!({"status":"SUCCESS","response":""}), "", true).0,
            DoneStatus::Completed
        );
        assert_eq!(
            terminal_outcome(&json!({"status":"CANCELED"}), "", false).0,
            DoneStatus::Interrupted
        );
    }

    #[test]
    fn parses_installed_cli_models() {
        let output = "gemini-3.8-flash-high\tGemini 3.8 Flash (High)\nclaude-sonnet-5-5-low\tClaude Sonnet 5.5 (Low)\nteam/model_1\tCustom model\n";
        let models = parse_models(output);
        assert_eq!(models.len(), 3);
        assert_eq!(models[1].id, "claude-sonnet-5-5-low");
        assert_eq!(models[1].label, "Claude Sonnet 5.5 (Low)");
        assert_eq!(models[2].id, "team/model_1");
    }

    #[test]
    fn groups_cli_effort_variants_for_the_picker() {
        let models = parse_models(
            "claude-opus-5-5-low\tClaude Opus 5.5 (Low)\nclaude-opus-5-5-medium\tClaude Opus 5.5 (Medium)\nclaude-opus-5-5-high\tClaude Opus 5.5 (High)\ngemini-3.1-pro-low\tGemini 3.1 Pro (Low)\ngemini-3.1-pro-high\tGemini 3.1 Pro (High)\ngpt-oss-120b-medium\tGPT OSS 120B (Medium)\n",
        );
        let grouped = crate::acp::group_effort_variants(models.clone());
        assert_eq!(grouped.len(), 3);
        assert_eq!(grouped[0].id, "claude-opus-5-5");
        assert_eq!(grouped[0].label, "Claude Opus 5.5");
        assert_eq!(
            grouped[0].reasoning_levels,
            vec![
                ReasoningLevel::Low,
                ReasoningLevel::Medium,
                ReasoningLevel::High,
            ]
        );
        assert_eq!(
            grouped[1].reasoning_levels,
            vec![ReasoningLevel::Low, ReasoningLevel::High]
        );
        assert_eq!(grouped[2].reasoning_levels, vec![ReasoningLevel::Medium]);
        assert_eq!(
            selected_model(Some(&grouped[0].id), Some(ReasoningLevel::Medium), &models),
            Some("claude-opus-5-5-medium".into())
        );
        assert_eq!(
            selected_model(
                Some("claude-opus-5-5-low"),
                Some(ReasoningLevel::High),
                &models
            ),
            Some("claude-opus-5-5-high".into())
        );
        let aliased = parse_models(
            "gemini-3.1-pro-low\tGemini 3.1 Pro (Low)\ngemini-pro-agent\tGemini 3.1 Pro (High)\n",
        );
        assert_eq!(
            selected_model(Some("gemini-3.1-pro"), Some(ReasoningLevel::High), &aliased),
            Some("gemini-pro-agent".into())
        );
    }

    #[test]
    fn maps_existing_acp_model_to_a_cli_variant() {
        let models = parse_models(
            "gemini-3.7-flash-high\tGemini 3.7 Flash (High)\ngemini-3.7-flash-low\tGemini 3.7 Flash (Low)\n",
        );
        assert_eq!(
            selected_model(Some("gemini-3.7-flash"), Some(ReasoningLevel::Low), &models),
            Some("gemini-3.7-flash-low".into())
        );
        assert_eq!(
            selected_model(
                Some("gemini-3.7-flash-low"),
                Some(ReasoningLevel::High),
                &models
            ),
            Some("gemini-3.7-flash-high".into())
        );
        assert_eq!(
            session_id("conversation-1"),
            Some("agy-cli:conversation-1".into())
        );
    }

    #[tokio::test]
    async fn tool_step_emits_one_call_and_one_result() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();
        let active = json!({
            "step_index": 4,
            "state": "ACTIVE",
            "tool_info": {"name": "run_command", "parameters": {"CommandLine": "echo hello"}}
        });
        let done = json!({
            "step_index": 4,
            "state": "DONE",
            "tool_info": {"name": "run_command", "output": "hello"}
        });
        emit_tool(&active, &mut seen, &mut completed, &sender).await;
        emit_tool(&done, &mut seen, &mut completed, &sender).await;
        emit_tool(&done, &mut seen, &mut completed, &sender).await;
        assert!(matches!(
            receiver.try_recv().unwrap().unwrap(),
            AgentEvent::ToolCall { id, call: ToolCall::Exec { command } }
                if id == "agy-step-4" && command == "echo hello"
        ));
        assert!(matches!(
            receiver.try_recv().unwrap().unwrap(),
            AgentEvent::ToolResult { id, output: Some(output), .. }
                if id == "agy-step-4" && output == "hello"
        ));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn denied_command_finishes_with_a_visible_permission_error() {
        let (sender, mut receiver) = mpsc::channel(4);
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();
        let active = json!({
            "step_index": 7,
            "state": "ACTIVE",
            "tool_info": {"name": "run_command", "parameters": {"CommandLine": "git status"}}
        });
        let denied = json!({
            "step_index": 7,
            "state": "ERROR",
            "tool_name": "run_command",
            "tool_info": {
                "name": "run_command",
                "error": {"type": "TOOL_ERROR", "message": "permission check failed: user denied permission to run command"}
            }
        });
        emit_tool(&active, &mut seen, &mut completed, &sender).await;
        emit_tool(&denied, &mut seen, &mut completed, &sender).await;
        emit_tool(&denied, &mut seen, &mut completed, &sender).await;
        assert!(matches!(
            receiver.try_recv().unwrap().unwrap(),
            AgentEvent::ToolCall { id, call: ToolCall::Exec { command } }
                if id == "agy-step-7" && command == "git status"
        ));
        assert!(matches!(
            receiver.try_recv().unwrap().unwrap(),
            AgentEvent::ToolResult { id, is_error: true, output: Some(output), .. }
                if id == "agy-step-7"
                    && output.contains("headless mode")
                    && output.contains("/permissions")
        ));
        assert!(receiver.try_recv().is_err());
    }
}
