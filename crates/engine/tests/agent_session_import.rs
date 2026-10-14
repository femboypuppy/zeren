//! Importing another agent's conversation: the chat row resumes the native
//! session from the conversation's own folder, the transcript is seeded on
//! chat2, the project is created on demand, and re-importing reopens the chat.

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use zeren_doc::{MessagePart, MessageRole};
use zeren_engine::agent_projects::AgentStoreRoots;
use zeren_engine::agent_sessions::import_session_with;
use zeren_engine::{EngineCore, EngineProfile, HarnessId, default_registry};
use zeren_proto::ImportedAgentSession;

fn roots(stores: &Path) -> AgentStoreRoots {
    AgentStoreRoots {
        claude_config: stores.join("claude"),
        claude_json: stores.join("claude.json"),
        codex_home: stores.join("codex"),
        opencode_data: stores.join("opencode"),
        pi_agent: stores.join("pi"),
        cursor_user: stores.join("cursor"),
        antigravity_user: stores.join("antigravity"),
        antigravity_cli: stores.join("agy"),
        excluded: Vec::new(),
    }
}

fn write_claude_session(stores: &Path, cwd: &str, session_id: &str) {
    let dir = stores.join("claude").join("projects").join("encoded-app");
    std::fs::create_dir_all(&dir).unwrap();
    let lines = [
        json!({"type": "user", "cwd": cwd, "timestamp": "2026-09-01T10:00:00Z",
               "message": {"role": "user", "content": "Fix the failing test"}}),
        json!({"type": "assistant", "cwd": cwd, "timestamp": "2026-09-01T10:00:03Z",
               "message": {"role": "assistant", "content": [
                   {"type": "text", "text": "Running the tests."},
                   {"type": "tool_use", "id": "toolu_1", "name": "Bash",
                    "input": {"command": "cargo test"}}]}}),
        json!({"type": "user", "cwd": cwd, "timestamp": "2026-09-01T10:00:09Z",
               "message": {"role": "user", "content": [
                   {"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}]}}),
        json!({"type": "assistant", "cwd": cwd, "timestamp": "2026-09-01T10:00:10Z",
               "message": {"role": "assistant", "content": [
                   {"type": "text", "text": "All green."}]}}),
    ];
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(dir.join(format!("{session_id}.jsonl")), body).unwrap();
}

#[tokio::test]
async fn antigravity_import_lists_previews_and_resumes_native_cli_history() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("work app");
    std::fs::create_dir_all(&project).unwrap();
    let cwd = project.to_string_lossy().into_owned();
    let stores = dir.path().join("stores");
    let agy = stores.join("agy");
    std::fs::create_dir_all(agy.join("cache")).unwrap();
    std::fs::create_dir_all(agy.join("conversations")).unwrap();
    let logs = agy.join("brain/native-1/.system_generated/logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(agy.join("conversations/native-1.db"), b"owned by agy").unwrap();
    let uri = format!(
        "file://{}{}",
        if cwd.starts_with('/') { "" } else { "/" },
        cwd.replace('\\', "/").replace(' ', "%20")
    );
    std::fs::write(agy.join("cache/conversation_metadata.json"), json!({"conversations":{
        "native-1":{"summary":{"Title":"Native chat","WorkspaceURIs":[uri],"UpdatedAt":"2026-10-14T07:40:52Z","Internal":false}},
        "../escape":{"summary":{"WorkspaceURIs":["file:///invalid"]}},
        "internal":{"is_internal":true}
    }}).to_string()).unwrap();
    let lines = [
        json!({"step_index":0,"type":"USER_INPUT","content":"<USER_REQUEST>\nCheck the tests\n</USER_REQUEST>","created_at":"2026-10-14T07:40:34Z"}),
        json!({"step_index":1,"type":"PLANNER_RESPONSE","content":"Checking.","thinking":"Plan", "tool_calls":[{"name":"run_command","args":{"CommandLine":"\"echo ok\""}}],"created_at":"2026-10-14T07:40:35Z"}),
        json!({"step_index":2,"type":"GENERIC","status":"ERROR","content":"private tool output","created_at":"2026-10-14T07:40:41Z"}),
        json!({"step_index":3,"type":"PLANNER_RESPONSE","content":"Command was denied.","created_at":"2026-10-14T07:40:52Z"}),
    ];
    let transcript = lines
        .iter()
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let transcript_path = logs.join("transcript.jsonl");
    std::fs::write(&transcript_path, &transcript).unwrap();
    let source_roots = roots(&stores);
    assert!(zeren_engine::agent_sessions::supports_sessions(
        HarnessId::Antigravity
    ));
    let projects = zeren_engine::agent_projects::discover(&source_roots);
    assert!(
        projects
            .iter()
            .any(|source| source.harness == HarnessId::Antigravity && source.projects.len() == 1)
    );
    let sessions = zeren_engine::agent_sessions::list_sessions_with(
        &source_roots,
        HarnessId::Antigravity,
        &cwd,
    )
    .unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "native-1");
    assert_eq!(sessions[0].prompt_count, 1);
    assert_eq!(sessions[0].title, "Native chat");
    let core = EngineCore::assemble_with_profile(
        EngineProfile::local(&dir.path().join("data")).unwrap(),
        Arc::new(default_registry()),
        HarnessId::Mock,
        None,
    )
    .unwrap();
    let imported = import_session_with(
        &core.doc_host,
        &core.workspace,
        &source_roots,
        HarnessId::Antigravity,
        "native-1",
    )
    .unwrap();
    let chat = core.workspace.chat(&imported.chat_id).unwrap().unwrap();
    assert_eq!(chat.harness_session_id.as_deref(), Some("agy-cli:native-1"));
    assert_eq!(chat.config.unwrap().harness, HarnessId::Antigravity);
    let entries = core
        .doc_host
        .open(&imported.chat_id)
        .unwrap()
        .doc()
        .read_entries()
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert!(
        matches!(&entries[0].parts[0], MessagePart::Text {text,..} if text == "Check the tests")
    );
    assert!(
        !serde_json::to_string(&entries)
            .unwrap()
            .contains("private tool output")
    );
    let again = import_session_with(
        &core.doc_host,
        &core.workspace,
        &source_roots,
        HarnessId::Antigravity,
        "native-1",
    )
    .unwrap();
    assert!(again.existing);
    assert_eq!(again.chat_id, imported.chat_id);
    assert_eq!(
        std::fs::read_to_string(transcript_path).unwrap(),
        transcript
    );
    assert_eq!(
        std::fs::read(agy.join("conversations/native-1.db")).unwrap(),
        b"owned by agy"
    );
    assert!(
        import_session_with(
            &core.doc_host,
            &core.workspace,
            &source_roots,
            HarnessId::Antigravity,
            "../escape"
        )
        .is_err()
    );
    core.shutdown().await;
}

/// The RPC's shape: the blocking import runs off the async runtime.
async fn import(core: &EngineCore, stores: &Path, session_id: &str) -> ImportedAgentSession {
    let (doc_host, workspace, roots, session_id) = (
        core.doc_host.clone(),
        core.workspace.clone(),
        roots(stores),
        session_id.to_string(),
    );
    tokio::task::spawn_blocking(move || {
        import_session_with(
            &doc_host,
            &workspace,
            &roots,
            HarnessId::ClaudeCode,
            &session_id,
        )
    })
    .await
    .unwrap()
    .expect("import")
}

#[tokio::test]
async fn claude_conversation_imports_as_a_resumable_chat() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("work").join("app");
    std::fs::create_dir_all(&project).unwrap();
    let cwd = project.to_string_lossy().into_owned();
    let stores = dir.path().join("stores");
    write_claude_session(&stores, &cwd, "sess-1");

    let core = EngineCore::assemble_with_profile(
        EngineProfile::local(&dir.path().join("data")).unwrap(),
        Arc::new(default_registry()),
        HarnessId::Mock,
        None,
    )
    .unwrap();

    let imported = import(&core, &stores, "sess-1").await;
    assert!(!imported.existing);

    let chat = core
        .workspace
        .chat(&imported.chat_id)
        .unwrap()
        .expect("row");
    assert_eq!(chat.device_id, core.device_id);
    assert_eq!(chat.title.as_deref(), Some("Fix the failing test"));
    assert_eq!(chat.config.as_ref().unwrap().harness, HarnessId::ClaudeCode);
    assert_eq!(chat.room_gen, Some(2));
    assert_eq!(chat.space_id.as_deref(), Some(imported.space_id.as_str()));
    assert_eq!(chat.cwd.as_deref(), Some(cwd.as_str()));
    assert_eq!(
        core.workspace.chat_harness_session(&imported.chat_id),
        Some((
            "sess-1".to_string(),
            Some(cwd.clone()),
            Some(HarnessId::ClaudeCode)
        )),
        "the next run resumes the native session from the same cwd"
    );

    let spaces = core.workspace.read_spaces().unwrap();
    let space = spaces
        .iter()
        .find(|s| s.id == imported.space_id)
        .expect("project created");
    assert_eq!(space.device_id, core.device_id);
    assert_eq!(Path::new(&space.path), project.as_path());

    let entries = core
        .doc_host
        .open(&imported.chat_id)
        .unwrap()
        .doc()
        .read_entries()
        .unwrap();
    let roles: Vec<_> = entries.iter().map(|e| e.role).collect();
    assert_eq!(roles, vec![MessageRole::User, MessageRole::Assistant]);
    assert!(matches!(
        &entries[0].parts[..],
        [MessagePart::Text { text, .. }] if text == "Fix the failing test"
    ));
    assert_eq!(entries[1].parts.len(), 3, "text, tool, text");

    let again = import(&core, &stores, "sess-1").await;
    assert!(again.existing, "re-importing reopens the chat");
    assert_eq!(again.chat_id, imported.chat_id);

    // An existing project for the folder is reused rather than duplicated.
    write_claude_session(&stores, &cwd, "sess-2");
    let second = import(&core, &stores, "sess-2").await;
    assert_eq!(second.space_id, imported.space_id);
    assert_ne!(second.chat_id, imported.chat_id);

    core.shutdown().await;
}
