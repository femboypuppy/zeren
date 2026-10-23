//! The composer footer's ring cluster. The plan-usage ring shows how much of
//! the active account's rate limit the session's harness has used, beside
//! the context ring. Clicking it opens the harness's accounts — each with its
//! usage meters — and clicking one switches to it, the same `ActivateAgentAccount` Settings →
//! Accounts runs. Both views share [`AccountsSnapshotCache`], so a switch in
//! either shows up in the other.
use std::time::{Duration, Instant};

use gpui::{
    Context, Entity, IntoElement, Render, SharedString, Subscription, Task, Window, div,
    prelude::*, px,
};
use zeren_proto::{AgentAccount, AgentAccountsSnapshot, CooldownResumeState, HarnessId};
use zeren_rpc::methods;

use crate::popover;
use crate::settings::accounts::{
    self, AccountsSnapshotCache, UsageLevel, render_account_usage, reports_usage, signs_in,
    usage_color, usage_level,
};
use crate::state::AppState;
use crate::theme::Theme;

/// Forced probes hit the provider; the engine throttles them too, but the
/// ring re-probes on every hover, so don't even ask more often than this.
const FORCE_MIN_INTERVAL: Duration = Duration::from_secs(30);
/// Background re-probe while a composer is alive: usage moves as turns run.
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// The binding limit: the most-used window of the account. Pure.
pub fn used_fraction(account: &AgentAccount) -> Option<f32> {
    account
        .usage_windows
        .iter()
        .map(|window| window.used_fraction.clamp(0.0, 1.0))
        .reduce(f32::max)
}

fn used_fraction_for_model(account: &AgentAccount, model: Option<&str>) -> Option<f32> {
    let group = match (account.harness, model) {
        (HarnessId::Antigravity, Some(model)) if model.starts_with("gemini-") => "Gemini Models",
        (HarnessId::Antigravity, Some(model))
            if model.starts_with("claude-") || model.starts_with("gpt-") =>
        {
            "Claude and GPT models"
        }
        _ => return used_fraction(account),
    };
    account
        .usage_windows
        .iter()
        .filter(|window| window.label.starts_with(group))
        .map(|window| window.used_fraction.clamp(0.0, 1.0))
        .reduce(f32::max)
}

/// The harness's live account, when it reports usage. Pure.
pub fn active_account(
    snapshot: &AgentAccountsSnapshot,
    harness: HarnessId,
) -> Option<&AgentAccount> {
    snapshot
        .accounts
        .iter()
        .find(|account| account.harness == harness && account.active)
}

/// Loads (and switches) the accounts of the composer's target device.
pub struct AccountUsage {
    session_chat: Option<String>,
    session_report: Option<zeren_engine::usage::UsageReport>,
    session_error: Option<String>,
    session_busy: bool,
    session_task: Option<Task<()>>,
    cooldown_chat: Option<String>,
    cooldown: CooldownResumeState,
    cooldown_busy: bool,
    cooldown_task: Option<Task<()>>,
    _cooldown_poll: Task<()>,
    state: Entity<AppState>,
    /// `None` = this device, as in [`AccountsSnapshotCache`].
    target: Option<String>,
    harness: Option<HarnessId>,
    model: Option<String>,
    /// Once any list has been asked for the current target.
    loaded: bool,
    last_forced: Option<Instant>,
    error: Option<SharedString>,
    load_task: Option<Task<()>>,
    action_task: Option<Task<()>>,
    popup: popover::Popup<FooterCard>,
    _poll: Task<()>,
    _cache: Subscription,
    _state: Subscription,
}

impl AccountUsage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let cooldown_poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                if this
                    .update(cx, |usage, cx| usage.refresh_cooldown(None, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this
                    .update(cx, |usage, cx| {
                        if usage.harness.is_some() {
                            usage.load(true, cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            session_chat: None,
            session_report: None,
            session_error: None,
            session_busy: false,
            session_task: None,
            cooldown_chat: None,
            cooldown: Default::default(),
            cooldown_busy: false,
            cooldown_task: None,
            _cooldown_poll: cooldown_poll,
            _state: cx.observe(&state, |_, _, cx| cx.notify()),
            state,
            target: None,
            harness: None,
            loaded: false,
            last_forced: None,
            error: None,
            model: None,
            load_task: None,
            action_task: None,
            popup: popover::Popup::default(),
            _poll: poll,
            // Settings → Accounts writes the same cache.
            _cache: cx.observe_global::<AccountsSnapshotCache>(|_, cx| cx.notify()),
        }
    }

    /// Point at the session's harness and device; loads on first sight of a
    /// device. Cheap enough to call every render.
    pub fn track(
        &mut self,
        harness: Option<HarnessId>,
        model: Option<String>,
        target: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let session_chat = self.state.read(cx).selected_chat.clone();
        if self.session_chat != session_chat || self.target != target {
            self.session_chat = session_chat;
            self.session_report = None;
            self.session_error = None;
            self.session_task = None;
            self.session_busy = false;
            if self.popup.get() == Some(&FooterCard::Accounts) {
                self.dismiss(cx);
            }
        }
        let harness = harness.filter(|h| signs_in(*h) && reports_usage(*h));
        self.harness = harness;
        self.model = model;
        let chat = matches!(harness, Some(HarnessId::ClaudeCode | HarnessId::Codex))
            .then(|| self.state.read(cx).selected_chat.clone())
            .flatten();
        if self.cooldown_chat != chat || self.target != target {
            self.cooldown_chat = chat;
            self.cooldown = Default::default();
            self.cooldown_task = None;
            self.cooldown_busy = false;
        }
        if self.target != target {
            self.target = target;
            self.loaded = false;
            self.last_forced = None;
            self.error = None;
        }
        if harness.is_some() && !self.loaded {
            self.loaded = true;
            self.load(true, cx);
        }
        if self.cooldown_chat.is_some() && self.cooldown_task.is_none() {
            self.refresh_cooldown(None, cx);
        }
    }

    fn refresh_cooldown(&mut self, enabled: Option<bool>, cx: &mut Context<Self>) {
        if enabled.is_none()
            && self.popup.as_open() == Some(&FooterCard::Accounts)
            && !self.session_busy
        {
            self.load_session(cx);
        }
        if self.cooldown_busy {
            return;
        }
        let Some(chat_id) = self.cooldown_chat.clone() else {
            return;
        };
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        let target = self.target.clone();
        let params = self.params(serde_json::json!({"chatId":chat_id,"enabled":enabled}));
        self.cooldown_busy = true;
        self.cooldown_task = Some(cx.spawn(async move |this, cx| {
            let result = engine.client().call(methods::COOLDOWN_RESUME, params).await;
            this.update(cx, |usage, cx| {
                if usage.cooldown_chat.as_ref() != Some(&chat_id) || usage.target != target {
                    return;
                }
                usage.cooldown_busy = false;
                match result {
                    Ok(value) => {
                        if let Ok(state) = serde_json::from_value(value) {
                            usage.cooldown = state;
                        }
                    }
                    Err(error) if enabled.is_some() => {
                        usage.error = Some(error.to_string().into());
                    }
                    Err(_) => {}
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn snapshot<'a>(&self, cx: &'a gpui::App) -> Option<&'a AgentAccountsSnapshot> {
        cx.try_global::<AccountsSnapshotCache>()?
            .0
            .get(&self.target)
    }

    fn params(&self, mut value: serde_json::Value) -> serde_json::Value {
        if let (Some(target), Some(object)) = (&self.target, value.as_object_mut()) {
            object.insert("targetDeviceId".into(), serde_json::json!(target));
        }
        value
    }

    /// Plain list first when nothing is cached (the engine's persisted usage
    /// paints at once), then the forced probe replaces it.
    fn load(&mut self, force_usage: bool, cx: &mut Context<Self>) {
        if force_usage {
            if self
                .last_forced
                .is_some_and(|at| at.elapsed() < FORCE_MIN_INTERVAL)
            {
                return;
            }
            self.last_forced = Some(Instant::now());
        }
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        let key = self.target.clone();
        let paint_first = force_usage && self.snapshot(cx).is_none();
        let plain = self.params(serde_json::json!({ "forceUsage": false }));
        let params = self.params(serde_json::json!({ "forceUsage": force_usage }));
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let fetch = |params: serde_json::Value| {
                let engine = engine.clone();
                async move {
                    engine
                        .client()
                        .call(methods::LIST_AGENT_ACCOUNTS, params)
                        .await
                        .ok()
                        .and_then(|value| {
                            serde_json::from_value::<AgentAccountsSnapshot>(value).ok()
                        })
                }
            };
            let store = |snapshot: AgentAccountsSnapshot, cx: &mut gpui::AsyncApp| {
                let key = key.clone();
                this.update(cx, |_, cx| {
                    cx.default_global::<AccountsSnapshotCache>()
                        .0
                        .insert(key, snapshot);
                })
                .ok();
            };
            if paint_first && let Some(snapshot) = fetch(plain).await {
                store(snapshot, cx);
            }
            if let Some(snapshot) = fetch(params).await {
                store(snapshot, cx);
            }
        }));
    }

    /// Switch optimistically, like Settings → Accounts: the rows flip at once
    /// and the engine's reply replaces them; a refusal restores the list.
    fn switch(&mut self, account: &AgentAccount, cx: &mut Context<Self>) {
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        let key = self.target.clone();
        let cache = &mut cx.default_global::<AccountsSnapshotCache>().0;
        let previous = cache.get(&key).cloned();
        if let Some(snapshot) = cache.get_mut(&key) {
            // Per-provider agents keep one live login per provider: only the
            // switched-to row's group changes.
            accounts::mark_switched(snapshot, account);
        }
        self.error = None;
        let params = self.params(serde_json::json!({
            "id": account.id,
            "accountId": account.id,
            "harness": account.harness,
        }));
        self.action_task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::ACTIVATE_AGENT_ACCOUNT, params)
                .await;
            this.update(cx, |usage, cx| {
                match result.map(serde_json::from_value::<AgentAccountsSnapshot>) {
                    Ok(Ok(snapshot)) => {
                        cx.default_global::<AccountsSnapshotCache>()
                            .0
                            .insert(key, snapshot);
                    }
                    Ok(Err(_)) => usage.load(false, cx),
                    Err(err) => {
                        if let Some(previous) = previous {
                            cx.default_global::<AccountsSnapshotCache>()
                                .0
                                .insert(key, previous);
                        }
                        usage.error = Some(err.to_string().into());
                    }
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// The ring's reading: the live account's most-used window, when the
    /// harness has a live account with usage to show.
    fn fraction(&self, cx: &gpui::App) -> Option<f32> {
        used_fraction_for_model(
            active_account(self.snapshot(cx)?, self.harness?)?,
            self.model.as_deref(),
        )
    }

    /// A trigger click: open this ring's popover, or close it when the
    /// press found it open (the card's mouse-down-out already began that
    /// close — see `Popup::note_trigger_press`).
    fn toggle(&mut self, card: FooterCard, cx: &mut Context<Self>) {
        if self.popup.take_press_was_open() || self.popup.as_open() == Some(&card) {
            self.dismiss(cx);
            return;
        }
        if card == FooterCard::Accounts {
            // Opening the card is the moment someone cares: re-probe.
            self.load(true, cx);
            self.load_session(cx);
        }
        self.popup.open(card);
        cx.notify();
    }

    fn load_session(&mut self, cx: &mut Context<Self>) {
        let Some(chat_id) = self.session_chat.clone() else {
            return;
        };
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        let target = self.target.clone();
        let params = self.params(serde_json::json!({"chatId":chat_id}));
        self.session_busy = true;
        self.session_task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(methods::GET_USAGE_REPORT, params)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| {
                    serde_json::from_value::<zeren_engine::usage::UsageReport>(value)
                        .map_err(|error| error.to_string())
                });
            this.update(cx, |usage, cx| {
                if usage.session_chat.as_ref() != Some(&chat_id) || usage.target != target {
                    return;
                }
                usage.session_busy = false;
                match result {
                    Ok(report) => {
                        usage.session_report = Some(report);
                        usage.session_error = None;
                    }
                    Err(error) => usage.session_error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
    }

    fn session_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = Theme::of(cx).for_popup();
        let mut card = div()
            .debug_selector(|| "usage-session-details".into())
            .flex()
            .flex_col()
            .gap(px(10.0))
            .mt(px(12.0))
            .pt(px(12.0))
            .px(px(8.0))
            .border_t_1()
            .border_color(theme.border)
            .child(popover::menu_heading(&theme, "This session"));
        if let Some(report) = &self.session_report {
            card = card.child(crate::settings::usage::session_summary(report, &theme));
            for group in &report.by_provider {
                card = card.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.text_muted)
                        .child(format!(
                            "{} · {} tokens",
                            group.label,
                            crate::settings::usage::number(group.totals.tokens())
                        )),
                );
            }
        } else if self.session_error.is_none() {
            card = card.child("Loading session usage…");
        }
        if let Some(error) = &self.session_error {
            card = card.child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(SharedString::from(error.clone())),
            );
        }
        card.child(
            crate::settings::widgets::text_action(
                &theme,
                crate::settings::widgets::ActionTone::Quiet,
                "Refresh",
            )
            .id("session-usage-refresh")
            .on_click(cx.listener(|usage, _, _, cx| usage.load_session(cx))),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme.text_muted)
                .child("Full history: Settings → Usage"),
        )
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.popup.begin_close() {
            popover::reap_popup(cx, |usage: &mut Self| &mut usage.popup);
        }
        cx.notify();
    }

    /// A footer ring that opens `card` above itself, right-aligned like the
    /// model picker.
    fn trigger(
        &self,
        chip: gpui::Stateful<gpui::Div>,
        card: FooterCard,
        content: impl FnOnce(&Self, &mut Context<Self>) -> gpui::Div,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let chip = chip
            .relative()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |usage, _, _, _| {
                    usage
                        .popup
                        .note_trigger_press_matching(|open| *open == card);
                }),
            )
            .on_click(cx.listener(move |usage, _, _, cx| usage.toggle(card, cx)));
        if self.popup.get() != Some(&card) {
            return chip.into_any_element();
        }
        let content = content(self, cx)
            .on_mouse_down_out(cx.listener(|usage, _, _, cx| usage.dismiss(cx)))
            .into_any_element();
        chip.child(popover::anchored_menu_above_end(
            card.id(),
            content,
            self.popup.closing_since(),
        ))
        .into_any_element()
    }

    fn accounts_card(&self, cx: &mut Context<Self>) -> gpui::Div {
        let theme = &Theme::of(cx).for_popup();
        let harness = self.harness;
        let rows: Vec<AgentAccount> = match (harness, self.snapshot(cx)) {
            (Some(harness), Some(snapshot)) => accounts::provider_accounts(snapshot, harness)
                .into_iter()
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        let title = harness
            .map(|harness| format!("{} usage", accounts::provider_name(harness)))
            .unwrap_or_else(|| "Usage".into());
        popover::popover_card(theme)
            .w(px(400.0))
            .flex()
            .flex_col()
            .child(popover::menu_heading(theme, &title))
            .children(rows.iter().enumerate().map(|(ix, account)| {
                let email: SharedString = account
                    .email
                    .clone()
                    .or_else(|| account.display_name.clone())
                    .unwrap_or_else(|| "Unknown account".into())
                    .into();
                let can_switch = !account.active && account.switchable;
                let mut meta: Vec<gpui::AnyElement> = account
                    .plan_label
                    .iter()
                    .map(|plan| {
                        div()
                            .child(SharedString::from(plan.clone()))
                            .into_any_element()
                    })
                    .collect();
                if account.active {
                    meta.push(
                        div()
                            .text_color(theme.accent)
                            .child(SharedString::from("In use"))
                            .into_any_element(),
                    );
                } else if let Some(reason) = account
                    .usage_error
                    .clone()
                    .filter(|_| account.usage_windows.is_empty())
                {
                    meta.push(div().child(SharedString::from(reason)).into_any_element());
                }
                let switch_to = account.clone();
                popover::menu_row(theme, account.active, format!("account-usage-row-{ix}"))
                    .id(("account-usage-row", ix))
                    .when(!can_switch, |row| row.cursor_default())
                    .when(can_switch, |row| {
                        row.on_click(cx.listener(move |usage, _, _, cx| {
                            usage.switch(&switch_to, cx);
                        }))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(12.5))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(email),
                            )
                            .when(!meta.is_empty(), |el| {
                                el.child(crate::settings::widgets::meta_line(theme, meta))
                            }),
                    )
                    .child(render_account_usage(account, theme))
            }))
            .children(self.error.clone().map(|error| {
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_size(px(12.0))
                    .text_color(theme.danger)
                    .child(error)
            }))
            .when(self.session_chat.is_some(), |card| {
                card.child(self.session_section(cx))
            })
    }
}

/// Which footer ring's popover is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterCard {
    Accounts,
    Context,
}

impl FooterCard {
    fn id(self) -> &'static str {
        match self {
            FooterCard::Accounts => "account-usage-menu",
            FooterCard::Context => "context-usage-menu",
        }
    }
}

/// The footer's ring cluster: account usage (accent arc, so the two read
/// apart), then context occupancy. Each opens its popover on click.
impl Render for AccountUsage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let context = self.state.read(cx).context_usage;
        let account = (self.harness.is_some() || self.session_chat.is_some()).then(|| {
            let measured = self.fraction(cx);
            let fraction = measured.unwrap_or(0.0);
            let level = usage_level(fraction);
            let chip = crate::context_usage::ring_chip(
                "account-usage",
                fraction,
                usage_color(level, &theme),
                match level {
                    UsageLevel::Normal => theme.text_muted,
                    _ => usage_color(level, &theme),
                },
                measured
                    .map(|fraction| format!("{}%", (fraction * 100.0).round() as u32))
                    .unwrap_or_else(|| "Usage".into()),
                self.popup.get() == Some(&FooterCard::Accounts),
                &theme,
            )
            .debug_selector(|| "account-usage-trigger".into());
            self.trigger(
                chip,
                FooterCard::Accounts,
                |usage, cx| usage.accounts_card(cx),
                cx,
            )
        });
        let context = crate::context_usage::has_window(context).then(|| {
            let chip = crate::context_usage::chip(
                context,
                self.popup.get() == Some(&FooterCard::Context),
                &theme,
            );
            self.trigger(
                chip,
                FooterCard::Context,
                move |_, cx| crate::context_usage::card(context, &Theme::of(cx).for_popup()),
                cx,
            )
        });
        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .when(self.cooldown.available, |row| {
                let enabled = self.cooldown.enabled;
                let label = if enabled {
                    self.cooldown
                        .resumes_at
                        .map(|at| {
                            format!(
                                "Resumes at {} · Cancel",
                                at.with_timezone(&chrono::Local).format("%H:%M")
                            )
                        })
                        .unwrap_or_else(|| "Waiting for quota · Cancel".into())
                } else {
                    "Auto-resume when available".into()
                };
                row.child(
                    popover::btn_ghost(&theme, &label, "cooldown-resume-button")
                        .id("cooldown-resume-button")
                        .on_click(cx.listener(move |usage, _, _, cx| {
                            usage.refresh_cooldown(Some(!enabled), cx)
                        })),
                )
            })
            .children(account)
            .children(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeren_proto::AgentUsageWindow;

    #[gpui::test]
    fn usage_indicator_opens_session_details_without_a_quota_measurement(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            gpui_base::init(cx);
            cx.set_global(Theme::default());
            cx.set_reduce_motion(true);
            cx.default_global::<AccountsSnapshotCache>();
        });
        let (usage, cx) = cx.add_window_view(|_, cx| {
            let state = cx.new(|_| {
                let mut state = AppState::new();
                state.selected_chat = Some("chat".into());
                state
            });
            let mut usage = AccountUsage::new(state, cx);
            usage.track(None, None, None, cx);
            usage
        });
        cx.update(|window, cx| window.draw(cx).clear());
        let trigger = cx
            .debug_bounds("account-usage-trigger")
            .expect("Usage remains accessible without plan quotas");
        cx.simulate_click(trigger.center(), gpui::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear());
        usage.update(cx, |usage, _| {
            assert_eq!(usage.popup.as_open(), Some(&FooterCard::Accounts))
        });
        assert!(
            cx.debug_bounds("usage-session-details").is_some(),
            "The usage popup must contain session details"
        );
    }

    fn account(harness: HarnessId, active: bool, used: &[f32]) -> AgentAccount {
        serde_json::from_value(serde_json::json!({
            "id": format!("{harness:?}-{active}"),
            "harness": harness,
            "email": null,
            "planLabel": null,
            "active": active,
            "switchable": true,
            "usageWindows": used
                .iter()
                .map(|fraction| AgentUsageWindow {
                    label: "5h".into(),
                    used_fraction: *fraction,
                    resets_at: None,
                })
                .collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    #[test]
    fn ring_shows_the_most_used_window() {
        assert_eq!(used_fraction(&account(HarnessId::Codex, true, &[])), None);
        assert_eq!(
            used_fraction(&account(HarnessId::Codex, true, &[0.12, 0.64])),
            Some(0.64)
        );
        assert_eq!(
            used_fraction(&account(HarnessId::Codex, true, &[1.4])),
            Some(1.0)
        );
    }

    #[test]
    fn antigravity_ring_uses_the_selected_models_quota_group() {
        let mut account = account(HarnessId::Antigravity, true, &[0.12, 1.0]);
        account.usage_windows[0].label = "Gemini Models · Week".into();
        account.usage_windows[1].label = "Claude and GPT models · Week".into();
        assert_eq!(
            used_fraction_for_model(&account, Some("gemini-3.8-flash")),
            Some(0.12)
        );
        assert_eq!(
            used_fraction_for_model(&account, Some("claude-opus-5-5")),
            Some(1.0)
        );
    }

    #[test]
    fn active_account_is_scoped_to_the_harness() {
        let snapshot = AgentAccountsSnapshot {
            accounts: vec![
                account(HarnessId::ClaudeCode, true, &[0.3]),
                account(HarnessId::Codex, false, &[0.1]),
                account(HarnessId::Codex, true, &[0.2]),
            ],
            warnings: Vec::new(),
        };
        assert_eq!(
            active_account(&snapshot, HarnessId::Codex).map(|a| a.id.as_str()),
            Some("Codex-true")
        );
        assert!(active_account(&snapshot, HarnessId::Cursor).is_none());
    }
}
