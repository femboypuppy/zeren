//! Read-only Antigravity CLI exports. The binary conversation DB stays owned by agy.
use super::*;
use crate::agent_projects::{file_uri_path, read_json};

struct Metadata {
    id: String,
    cwd: String,
    title: Option<String>,
    updated_at: DateTime<Utc>,
}

fn transcript_file(roots: &AgentStoreRoots, id: &str) -> PathBuf {
    roots
        .antigravity_cli
        .join("brain")
        .join(id)
        .join(".system_generated/logs/transcript.jsonl")
}

fn metadata(roots: &AgentStoreRoots) -> Vec<Metadata> {
    let Some(index) = read_json(
        &roots
            .antigravity_cli
            .join("cache/conversation_metadata.json"),
    ) else {
        return Vec::new();
    };
    let Some(conversations) = index["conversations"].as_object() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = conversations
        .iter()
        .filter_map(|(id, entry)| {
            if !valid_session_id(id)
                || entry["is_internal"] == true
                || entry["summary"]["Internal"] == true
            {
                return None;
            }
            let file = transcript_file(roots, id);
            if !file.is_file()
                || !roots
                    .antigravity_cli
                    .join("conversations")
                    .join(format!("{id}.db"))
                    .is_file()
            {
                return None;
            }
            let summary = &entry["summary"];
            let cwd = summary["WorkspaceURIs"]
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .find_map(file_uri_path)?;
            let updated_at = summary["UpdatedAt"]
                .as_str()
                .or_else(|| entry["last_modified_time"].as_str())
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .map(|date| date.with_timezone(&Utc))
                .or_else(|| {
                    file.metadata()
                        .ok()?
                        .modified()
                        .ok()
                        .map(DateTime::<Utc>::from)
                })?;
            Some(Metadata {
                id: id.clone(),
                cwd,
                title: summary["Title"].as_str().map(str::to_owned),
                updated_at,
            })
        })
        .collect();
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    entries
}

pub(crate) fn project_paths(roots: &AgentStoreRoots) -> Vec<(String, DateTime<Utc>)> {
    metadata(roots)
        .into_iter()
        .map(|entry| (entry.cwd, entry.updated_at))
        .collect()
}

pub(super) fn sessions(roots: &AgentStoreRoots, project: &str) -> Vec<AgentSession> {
    metadata(roots)
        .into_iter()
        .filter(|entry| same_folder(&entry.cwd, project))
        .take(MAX_SESSIONS)
        .filter_map(|entry| {
            let transcript = read_transcript(roots, &entry).ok()?;
            let prompt_count = transcript.prompt_count();
            if prompt_count == 0 {
                return None;
            }
            Some(AgentSession {
                id: entry.id,
                cwd: entry.cwd,
                updated_at: entry.updated_at,
                prompt_count,
                title: entry
                    .title
                    .filter(|title| !title.trim().is_empty())
                    .or_else(|| {
                        transcript
                            .first_prompt()
                            .map(|text| one_line(text, TITLE_MAX))
                    })
                    .unwrap_or_else(|| "Untitled conversation".into()),
            })
        })
        .collect()
}

pub(super) fn transcript(roots: &AgentStoreRoots, id: &str) -> Result<Transcript, EngineError> {
    let entry = metadata(roots)
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| not_found(id))?;
    read_transcript(roots, &entry)
}

fn read_transcript(
    roots: &AgentStoreRoots,
    metadata: &Metadata,
) -> Result<Transcript, EngineError> {
    let mut transcript = Transcript {
        cwd: Some(metadata.cwd.clone()),
        title: metadata.title.clone(),
        ..Default::default()
    };
    let mut pending_tool = None;
    for_each_line(&transcript_file(roots, &metadata.id), &[], |line| {
        let at = line["created_at"]
            .as_str()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|date| date.with_timezone(&Utc));
        match line["type"].as_str() {
            Some("USER_INPUT") => {
                if let Some(content) = line["content"].as_str() {
                    let content = content
                        .trim()
                        .strip_prefix("<USER_REQUEST>")
                        .unwrap_or(content)
                        .trim();
                    let content = content
                        .strip_suffix("</USER_REQUEST>")
                        .unwrap_or(content)
                        .trim();
                    if !content.is_empty() {
                        transcript.prompt(content.to_owned(), at);
                    }
                }
            }
            Some("PLANNER_RESPONSE" | "AGENT_RESPONSE") => {
                if let Some(content) = line["content"].as_str() {
                    transcript.text(content, at);
                }
                if let Some(thinking) = line["thinking"].as_str() {
                    transcript.reasoning(thinking, at);
                }
                for (index, tool) in line["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    let name = tool["name"].as_str().unwrap_or("tool");
                    let mut args = tool["args"].clone();
                    if let Some(fields) = args.as_object_mut() {
                        for value in fields.values_mut() {
                            if let Some(encoded) = value.as_str()
                                && let Ok(decoded) = serde_json::from_str::<Value>(encoded)
                            {
                                *value = decoded;
                            }
                        }
                    }
                    let call = if name == "run_command" {
                        args["CommandLine"].as_str().map(|command| ToolCall::Exec {
                            command: command.into(),
                        })
                    } else {
                        None
                    }
                    .unwrap_or_else(|| ToolCall::Unknown {
                        name: name.into(),
                        input: Some(args),
                    });
                    let id = format!(
                        "agy-{}-{index}",
                        line["step_index"].as_u64().unwrap_or_default()
                    );
                    transcript.tool(id.clone(), call, at);
                    pending_tool = Some(id);
                }
            }
            Some("GENERIC") => {
                if let Some(id) = pending_tool.take() {
                    transcript.tool_result(&id, line["status"] == "ERROR", at);
                }
            }
            _ => {}
        }
    })?;
    Ok(transcript)
}
