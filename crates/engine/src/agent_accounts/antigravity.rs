//! Native agy quota and sign-in. The CLI owns credentials and browser login.
use super::*;

impl AgentAccounts {
    pub(super) async fn refresh_antigravity_usage(&self) {
        let key = usage_key(HarnessId::Antigravity, "cli-oauth");
        {
            let usage = lock(&self.inner.usage);
            if usage
                .get(&key)
                .is_some_and(|entry| !entry.probe_due("native-agy", now_ms()))
            {
                return;
            }
        }
        if !lock(&self.inner.inflight_probes).insert(key.clone()) {
            return;
        }
        let _release = InflightGuard {
            set: &self.inner.inflight_probes,
            keys: vec![key.clone()],
        };
        let mut cli = zeren_harness::AgyCliHarness::new();
        if let Some(path) = lock(&self.inner.cli_overrides)
            .get(&HarnessId::Antigravity)
            .cloned()
        {
            cli = cli.with_executable(path);
        }
        let result = match cli.usage().await {
            Ok(frame) => parse_usage(&frame),
            Err(error) => {
                let message = error.to_string().to_ascii_lowercase();
                if message.contains("authentication") || message.contains("not logged") {
                    Err(ProbeError::Unauthorized { status: 401 })
                } else {
                    Err(ProbeError::Network {
                        timeout: message.contains("timed out"),
                    })
                }
            }
        };
        let mut usage = lock(&self.inner.usage);
        usage
            .entry(key)
            .or_default()
            .record(result, "native-agy".into(), now_ms());
        let persisted = serde_json::to_vec_pretty(&UsageCacheFile {
            entries: usage.clone(),
        })
        .map_err(|error| EngineError::Other(error.to_string()))
        .and_then(|bytes| {
            self.inner.config.private_root()?;
            write_file_atomic(&self.inner.config.usage_cache_file(), &bytes, true)
        });
        if let Err(error) = persisted {
            tracing::warn!(%error, "Antigravity usage cache write failed");
        }
    }

    #[cfg(windows)]
    pub(super) fn start_native_antigravity_login(
        &self,
        requester: Option<&str>,
    ) -> AgentLoginStart {
        self.reap_spawned_flows(HarnessId::Antigravity);
        let login_id = new_id();
        let state = Arc::new(Mutex::new(TaskLoginState {
            requester: requester.map(str::to_owned),
            ..Default::default()
        }));
        let task_state = state.clone();
        let executable = zeren_harness::AgyCliHarness::new().resolve_executable();
        let handle = tokio::spawn(async move {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::CREATE_NEW_CONSOLE;
            let outcome = async {
                if antigravity_cli_keychain_item() {
                    return Ok(());
                }
                let executable = executable.map_err(|error| error.to_string())?;
                let mut child = std::process::Command::new(executable)
                    .creation_flags(CREATE_NEW_CONSOLE)
                    .spawn()
                    .map_err(|error| format!("Could not open Antigravity sign-in: {error}"))?;
                let deadline = Instant::now() + Duration::from_secs(300);
                loop {
                    if antigravity_cli_keychain_item() {
                        return Ok(());
                    }
                    if Instant::now() >= deadline {
                        return Err("Antigravity sign-in timed out. Finish sign-in in agy, then Refresh.".into());
                    }
                    if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                        return Err(format!("Antigravity closed before saving a login ({status}). Try sign-in again."));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }.await;
            lock(&task_state).outcome = Some(outcome);
        });
        lock(&self.inner.flows).insert(
            login_id.clone(),
            LoginFlow::Task {
                harness: HarnessId::Antigravity,
                started_at: Instant::now(),
                state,
                handle,
                home: None,
                port: None,
            },
        );
        AgentLoginStart {
            login_id,
            url: String::new(),
            mode: AgentLoginMode::Browser,
            callback_port: None,
        }
    }
}

fn parse_usage(frame: &serde_json::Value) -> Result<UsageSnapshot, ProbeError> {
    if frame["command"]["name"] != "usage" || frame["status"] != "SUCCESS" {
        return Err(ProbeError::Schema);
    }
    let groups = frame["command"]["data"]["groups"]
        .as_array()
        .ok_or(ProbeError::Schema)?;
    let mut windows = Vec::new();
    for group in groups {
        let group_name = group["name"].as_str().ok_or(ProbeError::Schema)?;
        for bucket in group["buckets"].as_array().ok_or(ProbeError::Schema)? {
            if bucket["disabled"] == true {
                continue;
            }
            let remaining = bucket["remaining_fraction"]
                .as_f64()
                .ok_or(ProbeError::Schema)?;
            if !remaining.is_finite() || !(0.0..=1.0).contains(&remaining) {
                return Err(ProbeError::Schema);
            }
            let window = bucket["window"].as_str().ok_or(ProbeError::Schema)?;
            let label = match window {
                "weekly" => "Week",
                "5h" => "5 hours",
                other => other,
            };
            let resets_at = bucket["reset_time"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(chrono::DateTime::parse_from_rfc3339)
                .transpose()
                .map_err(|_| ProbeError::Schema)?
                .map(|value| value.with_timezone(&chrono::Utc));
            windows.push(AgentUsageWindow {
                label: format!("{group_name} · {label}"),
                used_fraction: (1.0 - remaining) as f32,
                resets_at,
            });
        }
    }
    if windows.is_empty() {
        return Err(ProbeError::Schema);
    }
    Ok(UsageSnapshot {
        windows,
        plan_label: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_usage_converts_remaining_to_used_and_skips_disabled_buckets() {
        let frame = serde_json::json!({"status":"SUCCESS","command":{"name":"usage","data":{"groups":[
            {"name":"Gemini Models","buckets":[{"window":"weekly","remaining_fraction":0.88,"reset_time":"2026-10-17T20:57:50Z"}]},
            {"name":"Claude and GPT models","buckets":[{"window":"weekly","remaining_fraction":0.0}, {"window":"5h","disabled":true,"remaining_fraction":1.0}]}
        ]}}});
        let usage = parse_usage(&frame).unwrap();
        assert_eq!(usage.windows.len(), 2);
        assert!((usage.windows[0].used_fraction - 0.12).abs() < 0.001);
        assert!(usage.windows[0].resets_at.is_some());
        assert_eq!(usage.windows[1].used_fraction, 1.0);
        assert_eq!(usage.windows[1].label, "Claude and GPT models · Week");
    }
    #[test]
    fn malformed_usage_never_claims_unused_quota() {
        assert!(parse_usage(&serde_json::json!({"status":"SUCCESS"})).is_err());
    }
}
