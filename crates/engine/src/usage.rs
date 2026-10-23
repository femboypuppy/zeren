//! Private, device-local usage ledger. Provider snapshots replace a turn's
//! previous snapshot; reconnects and repeated usage notifications never add it twice.
use chrono::{Days, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Mutex, PoisonError},
};
use zeren_proto::HarnessId;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: Option<f64>,
    pub active_ms: u64,
    pub turns: u64,
    pub reported_turns: u64,
}
impl UsageTotals {
    pub fn tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
    fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
        self.cache_write_tokens = self
            .cache_write_tokens
            .saturating_add(other.cache_write_tokens);
        self.active_ms = self.active_ms.saturating_add(other.active_ms);
        self.turns += other.turns;
        self.reported_turns += other.reported_turns;
        if let Some(cost) = other.cost_usd {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + cost);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub chat_id: String,
    pub harness: HarnessId,
    pub model: String,
    pub started_at: i64,
    pub totals: UsageTotals,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroup {
    pub label: String,
    pub totals: UsageTotals,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub totals: UsageTotals,
    pub sessions: usize,
    pub by_provider: Vec<UsageGroup>,
    pub by_model: Vec<UsageGroup>,
    pub days: Vec<UsageGroup>,
    pub recent: Vec<UsageRecord>,
    pub peak_day_tokens: u64,
    pub longest_task_ms: u64,
    pub longest_streak: usize,
    pub current_streak: usize,
    pub first_recorded_at: Option<i64>,
}

pub struct UsageStore {
    connection: Mutex<Connection>,
}

pub(crate) struct TurnUsage {
    chat_id: String,
    harness: HarnessId,
    model: String,
    pub(crate) record: Option<UsageRecord>,
}
impl TurnUsage {
    pub(crate) fn set_model(&mut self, model: &str) {
        if !model.is_empty() {
            self.model = model.into();
            if let Some(record) = &mut self.record {
                record.model = model.into();
            }
        }
    }
    pub(crate) fn new(chat_id: String, harness: HarnessId, model: Option<String>) -> Self {
        Self {
            chat_id,
            harness,
            model: model.unwrap_or_else(|| "Provider default".into()),
            record: None,
        }
    }
    pub(crate) fn observe(
        &mut self,
        id: &str,
        started_at: i64,
        now: i64,
        event: &zeren_proto::AgentEvent,
    ) -> &mut UsageRecord {
        if self.record.as_ref().is_none_or(|record| record.id != id) {
            self.record = Some(UsageRecord {
                id: id.into(),
                chat_id: self.chat_id.clone(),
                harness: self.harness,
                model: self.model.clone(),
                started_at,
                totals: UsageTotals {
                    turns: 1,
                    ..Default::default()
                },
            });
        }
        let record = self.record.as_mut().expect("initialized");
        record.totals.active_ms = now.saturating_sub(record.started_at).max(0) as u64;
        match event {
            zeren_proto::AgentEvent::Usage {
                input_tokens,
                output_tokens,
            } => {
                record.totals.input_tokens = *input_tokens;
                record.totals.output_tokens = *output_tokens;
                record.totals.reported_turns = 1;
            }
            zeren_proto::AgentEvent::UsageDetails {
                cached_tokens,
                cache_write_tokens,
                cost_usd,
                cost_session_id,
            } => {
                record.totals.cached_tokens = *cached_tokens;
                record.totals.cache_write_tokens = *cache_write_tokens;
                if cost_session_id.is_none() {
                    record.totals.cost_usd =
                        cost_usd.filter(|cost| cost.is_finite() && *cost >= 0.0);
                }
            }
            _ => {}
        }
        record
    }
}
impl UsageStore {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(3))?;
        connection.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS usage_turns (id TEXT PRIMARY KEY, started_at INTEGER NOT NULL, record TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS usage_started ON usage_turns(started_at);
            CREATE TABLE IF NOT EXISTS usage_cost_baselines (session_id TEXT PRIMARY KEY, latest REAL NOT NULL, base REAL NOT NULL, turn_id TEXT NOT NULL, known INTEGER NOT NULL);")?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn record(&self, record: &UsageRecord) -> anyhow::Result<()> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        connection.execute(
            "INSERT INTO usage_turns(id, started_at, record) VALUES (?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET record=excluded.record",
            params![record.id, record.started_at, serde_json::to_string(record)?],
        )?;
        Ok(())
    }
    /// Atomically convert a native running total into a per-turn estimate.
    /// An unobserved resumed session establishes a baseline without attributing
    /// its old spend to today's activity. Repeated snapshots retain one base.
    pub(crate) fn record_session_cost(
        &self,
        record: &mut UsageRecord,
        session_id: &str,
        total: f64,
        restored: bool,
    ) -> anyhow::Result<()> {
        if !total.is_finite() || total < 0.0 {
            return self.record(record);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction()?;
        let previous: Option<(f64, f64, String, bool)> = tx
            .query_row(
                "SELECT latest,base,turn_id,known FROM usage_cost_baselines WHERE session_id=?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (base, known) = match previous {
            Some((latest, _, _, _)) if total < latest => {
                // Crash results may zero the whole snapshot; retain the last
                // valid baseline instead of charging the session twice later.
                return Ok(());
            }
            Some((_, base, turn, known)) if turn == record.id => (base, known),
            Some((latest, _, _, _)) => (latest, true),
            None => (if restored { total } else { 0.0 }, !restored),
        };
        record.totals.cost_usd = known.then_some((total - base).max(0.0));
        tx.execute("INSERT INTO usage_cost_baselines(session_id,latest,base,turn_id,known) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(session_id) DO UPDATE SET latest=excluded.latest,base=excluded.base,turn_id=excluded.turn_id,known=excluded.known",
            params![session_id,total,base,record.id,known])?;
        tx.execute(
            "INSERT INTO usage_turns(id,started_at,record) VALUES(?1,?2,?3)
            ON CONFLICT(id) DO UPDATE SET record=excluded.record",
            params![record.id, record.started_at, serde_json::to_string(record)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn report(
        &self,
        days: Option<u32>,
        harness: Option<HarnessId>,
        chat: Option<&str>,
    ) -> anyhow::Result<UsageReport> {
        let today = Utc::now().date_naive();
        let since = days
            .filter(|days| *days > 0)
            .and_then(|days| today.checked_sub_days(Days::new(u64::from(days - 1))))
            .and_then(|day| day.and_hms_opt(0, 0, 0))
            .map(|day| day.and_utc().timestamp_millis())
            .unwrap_or(0);
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT record FROM usage_turns WHERE started_at >= ?1 ORDER BY started_at DESC",
        )?;
        let records = statement.query_map([since], |row| row.get::<_, String>(0))?;
        let mut report = UsageReport::default();
        let mut sessions = BTreeSet::new();
        let mut providers = BTreeMap::<String, UsageTotals>::new();
        let mut models = BTreeMap::<String, UsageTotals>::new();
        let mut activity = BTreeMap::<String, UsageTotals>::new();
        for record in records {
            let record: UsageRecord = serde_json::from_str(&record?)?;
            if harness.is_some_and(|h| h != record.harness)
                || chat.is_some_and(|id| id != record.chat_id)
            {
                continue;
            }
            report.totals.add(&record.totals);
            report.first_recorded_at = Some(
                report
                    .first_recorded_at
                    .map_or(record.started_at, |at| at.min(record.started_at)),
            );
            report.longest_task_ms = report.longest_task_ms.max(record.totals.active_ms);
            sessions.insert(record.chat_id.clone());
            providers
                .entry(provider_label(record.harness).into())
                .or_default()
                .add(&record.totals);
            models
                .entry(format!(
                    "{} · {}",
                    provider_label(record.harness),
                    record.model
                ))
                .or_default()
                .add(&record.totals);
            if let Some(date) = chrono::DateTime::from_timestamp_millis(record.started_at) {
                activity
                    .entry(date.format("%Y-%m-%d").to_string())
                    .or_default()
                    .add(&record.totals);
            }
            if report.recent.len() < 50 {
                report.recent.push(record);
            }
        }
        report.sessions = sessions.len();
        report.peak_day_tokens = activity
            .values()
            .map(UsageTotals::tokens)
            .max()
            .unwrap_or(0);
        let dates: BTreeSet<NaiveDate> = activity
            .keys()
            .filter_map(|date| date.parse().ok())
            .collect();
        (report.longest_streak, report.current_streak) = streaks(&dates, today);
        let groups = |map: BTreeMap<String, UsageTotals>| {
            map.into_iter()
                .map(|(label, totals)| UsageGroup { label, totals })
                .collect::<Vec<_>>()
        };
        report.by_provider = groups(providers);
        report.by_model = groups(models);
        report
            .by_model
            .sort_by_key(|group| std::cmp::Reverse(group.totals.tokens()));
        report.days = groups(activity);
        Ok(report)
    }
}

pub fn provider_label(harness: HarnessId) -> &'static str {
    match harness {
        HarnessId::Codex => "ChatGPT / Codex",
        HarnessId::ClaudeCode => "Claude Code",
        HarnessId::Antigravity => "Antigravity",
        HarnessId::Opencode => "OpenCode",
        HarnessId::Cursor => "Cursor",
        HarnessId::Grok => "Grok",
        HarnessId::Devin => "Devin",
        HarnessId::Pi => "Pi",
        HarnessId::Hermes => "Hermes",
        HarnessId::Mock => "Mock",
    }
}
fn streaks(dates: &BTreeSet<NaiveDate>, today: NaiveDate) -> (usize, usize) {
    let mut longest = 0;
    let mut run = 0;
    let mut previous: Option<NaiveDate> = None;
    for &date in dates {
        run = if previous.and_then(|date| date.succ_opt()) == Some(date) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        previous = Some(date);
    }
    let mut current = 0;
    let mut date = if dates.contains(&today) {
        Some(today)
    } else {
        today.pred_opt()
    };
    while let Some(day) = date.filter(|day| dates.contains(day)) {
        current += 1;
        date = day.pred_opt();
    }
    (longest, current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn running_cost_totals_are_differenced_and_survive_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.sqlite");
        let store = UsageStore::open(&path).unwrap();
        let mut turn = TurnUsage::new("chat".into(), HarnessId::ClaudeCode, None);
        let usage = zeren_proto::AgentEvent::Usage {
            input_tokens: 100,
            output_tokens: 20,
        };
        let first = turn.observe("first", 1000, 2000, &usage);
        store
            .record_session_cost(first, "native", 1.0, false)
            .unwrap();
        let second = turn.observe("second", 2000, 3000, &usage);
        store
            .record_session_cost(second, "native", 1.5, true)
            .unwrap();
        store
            .record_session_cost(second, "native", 1.7, true)
            .unwrap();
        assert!(
            (store
                .report(None, None, None)
                .unwrap()
                .totals
                .cost_usd
                .unwrap()
                - 1.7)
                .abs()
                < 1e-12
        );
        drop(store);
        let store = UsageStore::open(&path).unwrap();
        let third = turn.observe("third", 3000, 4000, &usage);
        store
            .record_session_cost(third, "native", 2.0, true)
            .unwrap();
        assert!((third.totals.cost_usd.unwrap() - 0.3).abs() < 1e-12);
        assert!(
            (store
                .report(None, None, None)
                .unwrap()
                .totals
                .cost_usd
                .unwrap()
                - 2.0)
                .abs()
                < 1e-12
        );
        let unknown = turn.observe("old-session-first", 5000, 6000, &usage);
        store
            .record_session_cost(unknown, "unobserved", 9.0, true)
            .unwrap();
        assert_eq!(unknown.totals.cost_usd, None);
        let known = turn.observe("old-session-next", 6000, 7000, &usage);
        store
            .record_session_cost(known, "unobserved", 9.2, true)
            .unwrap();
        assert!((known.totals.cost_usd.unwrap() - 0.2).abs() < 1e-12);
    }
    #[test]
    fn snapshots_replace_and_reports_filter_without_double_counting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.sqlite");
        let store = UsageStore::open(&path).unwrap();
        let mut record = UsageRecord {
            id: "turn".into(),
            chat_id: "chat".into(),
            harness: HarnessId::Codex,
            model: "model".into(),
            started_at: Utc::now().timestamp_millis(),
            totals: UsageTotals {
                input_tokens: 100,
                output_tokens: 20,
                turns: 1,
                reported_turns: 1,
                ..Default::default()
            },
        };
        store.record(&record).unwrap();
        record.totals.output_tokens = 40;
        store.record(&record).unwrap();
        drop(store);
        let store = UsageStore::open(&path).unwrap();
        let report = store.report(Some(7), None, Some("chat")).unwrap();
        assert_eq!(report.totals.tokens(), 140);
        assert_eq!(report.totals.turns, 1);
        assert_eq!(report.sessions, 1);
        assert_eq!(report.days.len(), 1);
        assert_eq!(report.current_streak, 1);
        assert_eq!(
            store
                .report(None, Some(HarnessId::ClaudeCode), None)
                .unwrap()
                .totals
                .tokens(),
            0
        );
        assert_eq!(store.report(None, None, Some("other")).unwrap().sessions, 0);
    }
    #[test]
    fn streak_keeps_yesterdays_run_until_today_is_over() {
        let today: NaiveDate = "2026-10-23".parse().unwrap();
        let dates = ["2026-10-19", "2026-10-21", "2026-10-22"]
            .map(|s| s.parse().unwrap())
            .into_iter()
            .collect();
        assert_eq!(streaks(&dates, today), (2, 2));
    }
}
