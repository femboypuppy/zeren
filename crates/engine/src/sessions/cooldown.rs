//! One-shot quota recovery, explicitly armed by the user and owned by the host.
use super::*;
use crate::agent_accounts::AgentAccounts;
use zeren_proto::{AgentAccount, CooldownResumeState};

pub(super) struct PendingResume {
    id: String,
    state: CooldownResumeState,
    cancel: CancellationToken,
}

impl Drop for PendingResume {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

struct BlockedTurn {
    entry_id: String,
    harness: HarnessId,
    request: RunRequest,
    config: serde_json::Value,
}

fn quota_error(message: &str) -> bool {
    let text = message.to_ascii_lowercase();
    if text.contains("image generation") || text.contains("billing") {
        return false;
    }
    [
        "usage limit",
        "rate limit",
        "rate_limit",
        "usage_limit",
        "limit reached",
        "hit your limit",
        "out of extra usage",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

fn reset_time(account: &AgentAccount, now: chrono::DateTime<Utc>) -> Option<chrono::DateTime<Utc>> {
    account
        .usage_windows
        .iter()
        .filter(|window| window.used_fraction >= 0.999)
        .filter_map(|window| window.resets_at)
        .filter(|at| *at > now)
        .max()
}

fn config_key(request: &RunRequest) -> serde_json::Value {
    serde_json::json!([
        request.harness,
        request.model,
        request.reasoning,
        request.model_options,
        request.cwd,
        request.sandbox
    ])
}

impl SessionsEngine {
    pub(super) fn cancel_cooldown_resume(&self, chat_id: &str) {
        lock(&self.inner.cooldown_resumes).remove(chat_id);
    }

    fn blocked_turn(&self, chat_id: &str) -> Option<BlockedTurn> {
        if self.turn_in_flight(chat_id) {
            return None;
        }
        let host = self.inner.doc_host()?;
        let config = host.request_from_chat_row(chat_id, "")?;
        let harness = host.harness_for_request(chat_id, &config);
        if !matches!(harness, HarnessId::ClaudeCode | HarnessId::Codex) {
            return None;
        }
        let handle = self.doc_handle(chat_id).ok()?;
        let entries = handle.doc().read_entries().ok()?;
        let entry = entries.last()?;
        if entry.role != MessageRole::Assistant
            || !entry.parts.iter().any(|part| match part {
                MessagePart::Error { message, .. } => quota_error(message),
                // Claude sometimes returns the quota notice as its assistant response.
                MessagePart::Text { text, .. } => {
                    let text = text.trim().to_ascii_lowercase();
                    (text.starts_with("you've hit your limit")
                        || text.starts_with("you have hit your limit")
                        || text.starts_with("usage limit reached"))
                        && quota_error(&text)
                }
                _ => false,
            })
        {
            return None;
        }
        let mut request = self.last_request(chat_id).unwrap_or_else(|| config.clone());
        request.prompt = "Continue where you stopped when the usage limit was reached. Finish the previous request.".into();
        request.attachments.clear();
        request.resume = None;
        Some(BlockedTurn {
            entry_id: entry.id.clone(),
            harness,
            request,
            config: config_key(&config),
        })
    }

    pub(crate) async fn cooldown_resume(
        &self,
        chat_id: &str,
        enabled: Option<bool>,
        accounts: AgentAccounts,
    ) -> Result<CooldownResumeState, EngineError> {
        if enabled == Some(false) {
            self.cancel_cooldown_resume(chat_id);
        }
        if let Some(pending) = lock(&self.inner.cooldown_resumes).get(chat_id) {
            return Ok(pending.state.clone());
        }
        let Some(blocked) = self.blocked_turn(chat_id) else {
            return Ok(CooldownResumeState::default());
        };
        let mut state = CooldownResumeState {
            available: true,
            ..Default::default()
        };
        if enabled != Some(true) {
            return Ok(state);
        }
        let snapshot = accounts.list(true).await?;
        let account = snapshot
            .accounts
            .iter()
            .find(|account| account.harness == blocked.harness && account.active)
            .ok_or_else(|| EngineError::Other("Sign in before enabling automatic resume".into()))?;
        // The account probe may have awaited a manual send or model change.
        if !self.same_blocked_turn(chat_id, &blocked) {
            return Ok(CooldownResumeState::default());
        }
        state.enabled = true;
        state.resumes_at = reset_time(account, Utc::now());
        let account_id = account.id.clone();
        let id = new_id();
        let cancel = CancellationToken::new();
        lock(&self.inner.cooldown_resumes).insert(
            chat_id.into(),
            PendingResume {
                id: id.clone(),
                state: state.clone(),
                cancel: cancel.clone(),
            },
        );
        let sessions = self.clone();
        let chat_id = chat_id.to_owned();
        tokio::spawn(async move {
            let result = sessions
                .wait_for_cooldown(&chat_id, &id, &blocked, &account_id, accounts, cancel)
                .await;
            let mut pending = lock(&sessions.inner.cooldown_resumes);
            if pending.get(&chat_id).is_some_and(|job| job.id == id) {
                pending.remove(&chat_id);
            }
            if let Err(error) = result {
                tracing::warn!(%chat_id, %error, "Cooldown resume stopped");
            }
        });
        Ok(state)
    }

    fn same_blocked_turn(&self, chat_id: &str, expected: &BlockedTurn) -> bool {
        self.blocked_turn(chat_id).is_some_and(|current| {
            current.entry_id == expected.entry_id
                && current.harness == expected.harness
                && current.config == expected.config
        })
    }

    async fn wait_for_cooldown(
        &self,
        chat_id: &str,
        id: &str,
        blocked: &BlockedTurn,
        account_id: &str,
        accounts: AgentAccounts,
        cancel: CancellationToken,
    ) -> Result<(), EngineError> {
        loop {
            let deadline = {
                let jobs = lock(&self.inner.cooldown_resumes);
                let Some(job) = jobs.get(chat_id).filter(|job| job.id == id) else {
                    return Ok(());
                };
                job.state.resumes_at
            };
            let delay = deadline
                .map(|at| {
                    (at + chrono::Duration::seconds(2) - Utc::now())
                        .to_std()
                        .unwrap_or_default()
                })
                .unwrap_or(std::time::Duration::from_secs(60));
            tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::sleep(delay.min(std::time::Duration::from_secs(60))) => {},
            }
            if !self.same_blocked_turn(chat_id, blocked) {
                return Ok(());
            }
            if deadline.is_some_and(|at| at + chrono::Duration::seconds(2) > Utc::now()) {
                continue;
            }
            let snapshot = tokio::select! {
                _ = cancel.cancelled() => return Ok(()),
                snapshot = accounts.list(true) => snapshot?,
            };
            let Some(account) = snapshot
                .accounts
                .iter()
                .find(|account| account.harness == blocked.harness && account.active)
            else {
                return Ok(());
            };
            if account.id != account_id {
                return Ok(());
            }
            let next_reset = reset_time(account, Utc::now());
            {
                let mut jobs = lock(&self.inner.cooldown_resumes);
                let Some(job) = jobs.get_mut(chat_id).filter(|job| job.id == id) else {
                    return Ok(());
                };
                job.state.resumes_at = next_reset;
            }
            if next_reset.is_some()
                || account.usage_error.is_some()
                || account.usage_windows.is_empty()
                || account
                    .usage_windows
                    .iter()
                    .any(|window| window.used_fraction >= 0.999 && window.resets_at.is_none())
            {
                continue;
            }
            if cancel.is_cancelled() || !self.same_blocked_turn(chat_id, blocked) {
                return Ok(());
            }
            // One continuation only. A second quota failure needs another click.
            let host = self
                .inner
                .doc_host()
                .ok_or_else(|| EngineError::Other("Chat host closed".into()))?;
            host.dispatch_with_source_context(
                self,
                chat_id,
                blocked.harness,
                blocked.request.clone(),
                None,
            )
            .await?;
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_plan_limits_but_not_billing_or_tool_limits() {
        assert!(quota_error(
            "Claude session limit reached — the turn was blocked"
        ));
        assert!(quota_error("You've hit your usage limit. Try again later"));
        assert!(!quota_error("Image generation usage limit exceeded"));
        assert!(!quota_error("Billing limit reached"));
        assert!(!quota_error("Network connection failed"));
    }
    #[test]
    fn removing_a_pending_resume_cancels_its_timer() {
        let cancel = CancellationToken::new();
        drop(PendingResume {
            id: "one".into(),
            state: Default::default(),
            cancel: cancel.clone(),
        });
        assert!(cancel.is_cancelled());
    }

    #[test]
    fn waits_for_all_exhausted_windows_and_ignores_unexhausted_ones() {
        let now = Utc::now();
        let short = now + chrono::Duration::hours(1);
        let long = now + chrono::Duration::days(2);
        let account: AgentAccount = serde_json::from_value(serde_json::json!({
            "id":"account", "harness":HarnessId::Codex, "active":true,
            "email":null, "planLabel":null, "usageFetchedAt":null, "usageError":null,
            "displayName":null, "organization":null, "authKind":null, "switchable":false,
            "savedAt":null, "provider":null,
            "usageWindows":[
                {"label":"Session","usedFraction":1.0,"resetsAt":short},
                {"label":"Week","usedFraction":1.0,"resetsAt":long},
                {"label":"Other","usedFraction":0.5,"resetsAt":long + chrono::Duration::days(1)}
            ]
        }))
        .unwrap();
        assert_eq!(reset_time(&account, now), Some(long));
        assert_eq!(
            reset_time(&account, long + chrono::Duration::seconds(1)),
            None
        );
    }
}
