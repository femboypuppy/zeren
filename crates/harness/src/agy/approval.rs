//! An agy PreToolUse hook bridges command approval to the engine's input UI.
use crate::{CancellationToken, HarnessError, process::Command};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream as AsyncTcpStream},
    sync::oneshot,
};
use zeren_proto::{UserInputAnswer, UserInputQuestion};

const ADDRESS: &str = "ZEREN_AGY_APPROVAL_ADDRESS";
const TOKEN: &str = "ZEREN_AGY_APPROVAL_TOKEN";
const LIMIT: u64 = 1_048_576;
type InputFn =
    dyn Fn(Vec<UserInputQuestion>) -> oneshot::Receiver<Vec<UserInputAnswer>> + Send + Sync;
static INSTALL: Mutex<()> = Mutex::new(());

pub(super) struct Bridge {
    listener: TcpListener,
    token: String,
}

pub(super) struct BridgeTask(tokio::task::JoinHandle<()>);
impl Drop for BridgeTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Bridge {
    pub(super) async fn prepare(command: &mut Command) -> Result<Self, HarnessError> {
        let executable = std::env::current_exe()?;
        let hooks = crate::executable::home_or_current_dir().join(".gemini/config/hooks.json");
        install_hook(&hooks, &executable)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let token = uuid::Uuid::new_v4().to_string();
        command
            .env(ADDRESS, listener.local_addr()?.to_string())
            .env(TOKEN, &token);
        Ok(Self { listener, token })
    }

    pub(super) fn serve(self, input: Box<InputFn>, cancel: CancellationToken) -> BridgeTask {
        let input: Arc<InputFn> = Arc::from(input);
        BridgeTask(tokio::spawn(async move {
            let mut tasks = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tasks.join_next(), if !tasks.is_empty() => {},
                    connection = self.listener.accept() => {
                        let Ok((socket, _)) = connection else { break };
                        let token = self.token.clone();
                        let input = input.clone();
                        tasks.spawn(async move {
                            let _ = tokio::time::timeout(Duration::from_secs(600), answer(socket, &token, input)).await;
                        });
                    }
                }
            }
        }))
    }
}

fn install_hook(path: &Path, executable: &Path) -> Result<(), HarnessError> {
    let _lock = INSTALL.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut hooks: Value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
            HarnessError::Protocol(
                "Antigravity hooks.json is invalid; repair it before using command approvals"
                    .into(),
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error.into()),
    };
    let object = hooks
        .as_object_mut()
        .ok_or_else(|| HarnessError::Protocol("Antigravity hooks.json must be an object".into()))?;
    let command = hook_command(executable)?;
    let hook = json!({"PreToolUse":[{"matcher":"^run_command$","hooks":[{
        "type":"command", "command":command, "timeout":610
    }]}]});
    if object.get("zeren-command-approval") == Some(&hook) {
        return Ok(());
    }
    object.insert("zeren-command-approval".into(), hook);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<(), HarnessError> {
        let mut file = options.open(&temporary)?;
        file.write_all(
            &serde_json::to_vec_pretty(&hooks)
                .map_err(|e| HarnessError::Protocol(e.to_string()))?,
        )?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn hook_command(executable: &Path) -> Result<String, HarnessError> {
    let exe = executable.to_string_lossy();
    let command = if cfg!(windows) {
        if exe.contains(['"', '%', '\r', '\n']) {
            return Err(HarnessError::Protocol(
                "Cannot quote the Zeren hook executable path".into(),
            ));
        }
        // agy runs hook strings through cmd.exe. Starting with a quoted
        // executable makes its shell wrapper escape the quotes literally.
        // Encode the script so the wrapper cannot turn its quotes into literals.
        let path = exe.replace('\\', "/").replace('\'', "''");
        let script = format!("& '{path}' --agy-permission-hook");
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode(
            script
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
        format!("powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand {encoded}")
    } else {
        format!("'{}' --agy-permission-hook", exe.replace('\'', "'\\''"))
    };
    Ok(command)
}

fn question(payload: &Value) -> Option<UserInputQuestion> {
    if payload["toolCall"]["name"] != "run_command" {
        return None;
    }
    let args = &payload["toolCall"]["args"];
    let command = args["CommandLine"]
        .as_str()
        .filter(|command| !command.trim().is_empty())?;
    let cwd = args["Cwd"]
        .as_str()
        .or_else(|| payload["workspacePaths"][0].as_str())
        .unwrap_or("Current workspace");
    Some(UserInputQuestion {
        id: uuid::Uuid::new_v4().to_string(),
        header: "Command approval".into(),
        question: format!("Allow Antigravity to run this command?\n\n{command}\n\nFolder: {cwd}"),
        options: vec!["Reject".into(), "Accept".into()],
        multi_select: false,
        prefill: None,
        multiline: false,
    })
}

fn decision(accepted: bool) -> Value {
    json!({"decision":if accepted {"auto_approve"} else {"deny"},
        "reason":if accepted {"User accepted this command in Zeren"} else {"Command rejected or approval cancelled in Zeren"}})
}

async fn answer(
    socket: AsyncTcpStream,
    token: &str,
    input: Arc<InputFn>,
) -> Result<(), std::io::Error> {
    let (read, mut write) = socket.into_split();
    let mut line = String::new();
    let mut read = BufReader::new(tokio::io::AsyncReadExt::take(read, LIMIT));
    tokio::time::timeout(Duration::from_secs(5), read.read_line(&mut line)).await??;
    let envelope: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
    let mut accepted = false;
    if envelope["token"].as_str() == Some(token)
        && let Some(question) = question(&envelope["payload"])
    {
        let id = question.id.clone();
        let answers = input(vec![question]).await.unwrap_or_default();
        accepted = answers
            .iter()
            .any(|answer| answer.question_id == id && answer.labels == ["Accept"]);
    }
    write
        .write_all(format!("{}\n", decision(accepted)).as_bytes())
        .await
}

/// Fast CLI entry point, before UI initialization or logging. Hook failures deny.
pub fn run_hook() -> Result<(), std::io::Error> {
    let output = match std::env::var(ADDRESS) {
        Err(_) => json!({"decision":"allow"}), // Other agy launches retain their default policy.
        Ok(address) => hook_request(&address).unwrap_or_else(|_| decision(false)),
    };
    writeln!(std::io::stdout(), "{output}")
}

fn hook_request(address: &str) -> Result<Value, std::io::Error> {
    let address: SocketAddr = address
        .parse()
        .map_err(|_| std::io::Error::other("Invalid approval address"))?;
    if !address.ip().is_loopback() {
        return Err(std::io::Error::other("Approval must be local"));
    }
    let token =
        std::env::var(TOKEN).map_err(|_| std::io::Error::other("Missing approval token"))?;
    let mut bytes = Vec::new();
    std::io::stdin().take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(std::io::Error::other("Approval payload too large"));
    }
    let payload: Value = serde_json::from_slice(&bytes)?;
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_secs(5))?;
    socket.set_read_timeout(Some(Duration::from_secs(600)))?;
    socket.set_write_timeout(Some(Duration::from_secs(5)))?;
    writeln!(socket, "{}", json!({"token":token, "payload":payload}))?;
    let mut line = String::new();
    std::io::BufReader::new(socket.take(4096)).read_line(&mut line)?;
    let response: Value = serde_json::from_str(&line)?;
    match response["decision"].as_str() {
        Some("auto_approve" | "deny") => Ok(response),
        _ => Err(std::io::Error::other("Invalid approval decision")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> Value {
        json!({"toolCall":{"name":"run_command","args":{"CommandLine":"echo approved","Cwd":"/workspace"}}})
    }

    #[tokio::test]
    async fn command_waits_for_explicit_accept_before_approval() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (question_tx, mut question_rx) = tokio::sync::mpsc::unbounded_channel();
        let input: Arc<InputFn> = Arc::new(move |questions| {
            let (answer_tx, answer_rx) = oneshot::channel();
            question_tx.send((questions[0].clone(), answer_tx)).unwrap();
            answer_rx
        });
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            answer(socket, "secret", input).await.unwrap();
        });
        let socket = AsyncTcpStream::connect(address).await.unwrap();
        let (read, mut write) = socket.into_split();
        write
            .write_all(format!("{}\n", json!({"token":"secret","payload":payload()})).as_bytes())
            .await
            .unwrap();
        let (question, reply) = question_rx.recv().await.unwrap();
        assert!(question.question.contains("echo approved"));
        assert!(question.question.contains("/workspace"));
        assert_eq!(question.options, vec!["Reject", "Accept"]);
        let mut lines = BufReader::new(read).lines();
        assert!(
            tokio::time::timeout(Duration::from_millis(25), lines.next_line())
                .await
                .is_err()
        );
        reply
            .send(vec![UserInputAnswer {
                question_id: question.id,
                labels: vec!["Accept".into()],
            }])
            .unwrap();
        let response: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(response["decision"], "auto_approve");
        task.await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_rejected_and_unauthenticated_requests_never_execute() {
        for (token, label) in [
            ("secret", Some("Reject")),
            ("secret", None),
            ("wrong", Some("Accept")),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let input: Arc<InputFn> = Arc::new(move |questions| {
                let (tx, rx) = oneshot::channel();
                if let Some(label) = label {
                    tx.send(vec![UserInputAnswer {
                        question_id: questions[0].id.clone(),
                        labels: vec![label.into()],
                    }])
                    .unwrap();
                }
                rx
            });
            let task = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                answer(socket, "secret", input).await.unwrap();
            });
            let mut socket = AsyncTcpStream::connect(address).await.unwrap();
            socket
                .write_all(format!("{}\n", json!({"token":token,"payload":payload()})).as_bytes())
                .await
                .unwrap();
            let mut line = String::new();
            BufReader::new(socket).read_line(&mut line).await.unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&line).unwrap()["decision"],
                "deny"
            );
            task.await.unwrap();
        }
    }

    #[test]
    fn hook_install_preserves_user_hooks_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("hooks.json");
        std::fs::write(&path, r#"{"existing":{"enabled":false}}"#).unwrap();
        let exe = root.path().join("Zeren App/zeren.exe");
        install_hook(&path, &exe).unwrap();
        let first = std::fs::read(&path).unwrap();
        let hooks: Value = serde_json::from_slice(&first).unwrap();
        assert_eq!(hooks["existing"]["enabled"], false);
        assert_eq!(
            hooks["zeren-command-approval"]["PreToolUse"][0]["matcher"],
            "^run_command$"
        );
        install_hook(&path, &exe).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), first);
        std::fs::write(&path, "invalid").unwrap();
        assert!(install_hook(&path, &exe).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid");
    }
}
