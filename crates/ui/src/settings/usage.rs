//! Usage history and the composer's per-conversation usage card.
use super::{accounts, widgets};
use crate::{popover, state::AppState, theme::Theme};
use gpui::{
    Context, Entity, Render, SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use std::time::Duration;
use zeren_engine::usage::{UsageReport, provider_label};
use zeren_proto::{AgentAccountsSnapshot, HarnessId};
use zeren_rpc::methods;

pub(crate) fn number(value: u64) -> String {
    if value >= 1_000_000_000 {
        format!("{:.2}B", value as f64 / 1_000_000_000.0)
    } else if value >= 1_000_000 {
        format!("{:.2}M", value as f64 / 1_000_000.0)
    } else if value >= 10_000 {
        format!("{:.1}K", value as f64 / 1000.0)
    } else {
        value.to_string()
    }
}
pub(crate) fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds >= 3600 {
        format!("{}h {}m", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{}m {}s", seconds / 60, seconds % 60)
    } else {
        format!("{seconds}s")
    }
}
fn metric(theme: &Theme, label: &str, value: String) -> gpui::Div {
    div()
        .min_w(px(135.0))
        .flex_1()
        .p(px(12.0))
        .rounded(px(10.0))
        .bg(theme.surface_card)
        .child(
            div()
                .text_size(px(20.0))
                .line_height(px(26.0))
                .text_color(theme.text)
                .child(SharedString::from(value)),
        )
        .child(
            div()
                .mt(px(4.0))
                .text_size(px(11.5))
                .line_height(px(17.0))
                .text_color(theme.text_muted)
                .child(SharedString::from(label.to_string())),
        )
}
pub(crate) fn session_summary(report: &UsageReport, theme: &Theme) -> gpui::Div {
    let totals = &report.totals;
    div().flex().flex_col().gap(px(8.0))
        .child(summary_line(theme,"Total tokens",number(totals.tokens())))
        .child(summary_line(theme,"Input",number(totals.input_tokens)))
        .child(summary_line(theme,"Output",number(totals.output_tokens)))
        .child(summary_line(theme,"Cached input",number(totals.cached_tokens)))
        .child(summary_line(theme,"Cache creation",number(totals.cache_write_tokens)))
        .child(summary_line(theme,"Turn time",duration(totals.active_ms)))
        .child(summary_line(theme,"Turns",totals.turns.to_string()))
        .child(summary_line(theme,"Estimated cost",totals.cost_usd.map(|cost| format!("${cost:.4}")).unwrap_or_else(|| "Not reported".into())))
        .child(div().mt(px(6.0)).text_size(px(11.0)).line_height(px(16.0)).text_color(theme.text_muted)
            .child(format!("Usage reported for {} of {} turns. Cost is a CLI-reported estimate; subscription charges differ.",totals.reported_turns,totals.turns)))
}
fn summary_line(theme: &Theme, label: &str, value: String) -> gpui::Div {
    div()
        .flex()
        .justify_between()
        .gap(px(12.0))
        .text_size(px(12.0))
        .line_height(px(18.0))
        .child(
            div()
                .text_color(theme.text_muted)
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .text_color(theme.text)
                .child(SharedString::from(value)),
        )
}

pub struct UsagePage {
    state: Entity<AppState>,
    scroll: widgets::PageScroll,
    report: Option<UsageReport>,
    accounts: Option<AgentAccountsSnapshot>,
    error: Option<String>,
    days: Option<u32>,
    harness: Option<HarnessId>,
    loading: bool,
    task: Option<Task<()>>,
    _state: Subscription,
    _poll: Task<()>,
}
impl popover::ScrollRailHost for UsagePage {
    fn rail_bar(&mut self) -> &mut popover::MenuScrollbarState {
        self.scroll.rail_bar()
    }
    fn rail_scroll(&self) -> Option<gpui::ScrollHandle> {
        Some(self.scroll.scroll.clone())
    }
}
impl UsagePage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&state, |this: &mut Self, _, cx| {
            if this.report.is_none() && !this.loading {
                this.load(cx);
            }
        });
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        if !this.loading {
                            this.load(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut page = Self {
            state,
            scroll: Default::default(),
            report: None,
            accounts: None,
            error: None,
            days: None,
            harness: None,
            loading: false,
            task: None,
            _state: subscription,
            _poll: poll,
        };
        page.load(cx);
        page
    }
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        self.loading = true;
        let params = serde_json::json!({"days":self.days,"harness":self.harness});
        self.task = Some(cx.spawn(async move |this, cx| {
            let (report, accounts) = futures::join!(
                engine.client().call(methods::GET_USAGE_REPORT, params),
                engine.client().call(
                    methods::LIST_AGENT_ACCOUNTS,
                    serde_json::json!({"forceUsage":false})
                )
            );
            this.update(cx, |this, cx| {
                this.loading = false;
                match report.map_err(|error| error.to_string()).and_then(|value| {
                    serde_json::from_value(value).map_err(|error| error.to_string())
                }) {
                    Ok(report) => {
                        this.report = Some(report);
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error),
                }
                if let Ok(value) = accounts {
                    this.accounts = serde_json::from_value(value).ok();
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
    fn filters(&self, theme: &Theme, cx: &mut Context<Self>) -> gpui::Div {
        let mut filters = div().mt(px(20.0)).flex().flex_col().gap(px(8.0));
        filters = filters.child(
            div().flex().flex_wrap().gap(px(6.0)).children(
                [
                    ("Today (UTC)", Some(1)),
                    ("7 days", Some(7)),
                    ("30 days", Some(30)),
                    ("All time", None),
                ]
                .into_iter()
                .map(|(label, days)| {
                    widgets::text_action(
                        theme,
                        if self.days == days {
                            widgets::ActionTone::Solid
                        } else {
                            widgets::ActionTone::Quiet
                        },
                        label,
                    )
                    .id(SharedString::from(format!("usage-window-{label}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.days = days;
                        this.load(cx);
                    }))
                }),
            ),
        );
        filters.child(
            div().flex().flex_wrap().gap(px(6.0)).children(
                [
                    ("All providers", None),
                    ("ChatGPT", Some(HarnessId::Codex)),
                    ("Claude", Some(HarnessId::ClaudeCode)),
                    ("Antigravity", Some(HarnessId::Antigravity)),
                    ("OpenCode", Some(HarnessId::Opencode)),
                    ("Cursor", Some(HarnessId::Cursor)),
                    ("Grok", Some(HarnessId::Grok)),
                    ("Devin", Some(HarnessId::Devin)),
                    ("Pi", Some(HarnessId::Pi)),
                    ("Hermes", Some(HarnessId::Hermes)),
                ]
                .into_iter()
                .map(|(label, harness)| {
                    widgets::text_action(
                        theme,
                        if self.harness == harness {
                            widgets::ActionTone::Solid
                        } else {
                            widgets::ActionTone::Quiet
                        },
                        label,
                    )
                    .id(SharedString::from(format!("usage-provider-{label}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.harness = harness;
                        this.load(cx);
                    }))
                }),
            ),
        )
    }
    fn body(&self, report: &UsageReport, theme: &Theme, cx: &gpui::App) -> gpui::Div {
        let t = &report.totals;
        let metrics = [
            ("Total tokens", number(t.tokens())),
            ("Input tokens", number(t.input_tokens)),
            ("Output tokens", number(t.output_tokens)),
            ("Cached input", number(t.cached_tokens)),
            ("Conversations", report.sessions.to_string()),
            ("Turns", t.turns.to_string()),
            ("Turn time", duration(t.active_ms)),
            ("Peak daily tokens", number(report.peak_day_tokens)),
            ("Longest task", duration(report.longest_task_ms)),
            ("Longest streak", format!("{} days", report.longest_streak)),
            ("Current streak", format!("{} days", report.current_streak)),
            (
                "Estimated cost",
                t.cost_usd
                    .map(|cost| format!("${cost:.2}"))
                    .unwrap_or_else(|| "Not reported".into()),
            ),
        ];
        let mut body = div().flex().flex_col().flex_none()
            .child(div().mt(px(20.0)).flex().flex_wrap().gap(px(12.0)).children(metrics.into_iter().map(|(label,value)| metric(theme,label,value))))
            .child(widgets::page_subtitle(theme,format!("Measured usage from {} of {} turns. Dates use UTC. Costs are CLI-reported estimates.",t.reported_turns,t.turns)).mt(px(14.0)).line_height(px(20.0)))
            .when(report.totals.turns == 0,|body| body.child(widgets::page_subtitle(theme,"No activity recorded yet. Your history grows as you chat in Zeren.").mt(px(10.0)).line_height(px(20.0))))
            .child(widgets::section(theme,"Token activity",activity(report,theme)));
        if let Some(accounts) = &self.accounts {
            let active: Vec<_> = accounts
                .accounts
                .iter()
                .filter(|account| {
                    account.active
                        && self
                            .harness
                            .is_none_or(|harness| harness == account.harness)
                })
                .collect();
            if !active.is_empty() {
                body = body.child(widgets::section(
                    theme,
                    "Account limits",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .children(active.into_iter().map(|account| {
                            widgets::section_card(theme)
                                .mt_0()
                                .p(px(16.0))
                                .child(
                                    div()
                                        .mb(px(12.0))
                                        .text_size(px(12.0))
                                        .line_height(px(18.0))
                                        .child(format!(
                                            "{} · {}",
                                            provider_label(account.harness),
                                            account
                                                .email
                                                .as_deref()
                                                .or(account.display_name.as_deref())
                                                .unwrap_or("Current account")
                                        )),
                                )
                                .child(accounts::render_account_usage(account, theme))
                        })),
                ));
            }
        }
        for (label, groups) in [
            ("By provider", &report.by_provider),
            ("By session model", &report.by_model),
        ] {
            if groups.is_empty() {
                continue;
            }
            body =
                body.child(widgets::section(
                    theme,
                    label,
                    widgets::section_card(theme)
                        .mt_0()
                        .children(groups.iter().enumerate().map(|(ix, group)| {
                            widgets::card_row(theme, ix == 0)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.0))
                                        .line_height(px(18.0))
                                        .child(SharedString::from(group.label.clone())),
                                )
                                .child(div().text_size(px(12.0)).line_height(px(18.0)).child(
                                    format!(
                                        "{} tokens · {}",
                                        number(group.totals.tokens()),
                                        duration(group.totals.active_ms)
                                    ),
                                ))
                        })),
                ));
        }
        body.when(!report.recent.is_empty(), |body| {
            body.child(widgets::section(
                theme,
                "Recent turns",
                widgets::section_card(theme)
                    .mt_0()
                    .children(report.recent.iter().enumerate().map(|(ix, record)| {
                        let title = self
                            .state
                            .read(cx)
                            .chats
                            .iter()
                            .find(|chat| chat.id == record.chat_id)
                            .and_then(|chat| chat.title.clone())
                            .unwrap_or_else(|| "Conversation".into());
                        let at = chrono::DateTime::from_timestamp_millis(record.started_at)
                            .map(|date| date.format("%b %d %H:%M UTC").to_string())
                            .unwrap_or_default();
                        widgets::card_row(theme, ix == 0)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_size(px(12.0)).line_height(px(18.0)).child(
                                        format!(
                                            "{} · {}",
                                            provider_label(record.harness),
                                            record.model
                                        ),
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .line_height(px(17.0))
                                            .mt(px(4.0))
                                            .text_color(theme.text_muted)
                                            .child(format!("{title} · {at}")),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .line_height(px(18.0))
                                    .child(format!(
                                        "{} tokens · {}",
                                        number(record.totals.tokens()),
                                        duration(record.totals.active_ms)
                                    )),
                            )
                    })),
            ))
        })
    }
}

fn activity_fill(theme: &Theme, tokens: u64, peak: u64) -> gpui::Hsla {
    if tokens == 0 {
        return theme.text_muted.opacity(0.12);
    }
    let intensity = (tokens as f64 / peak.max(1) as f64).min(1.0).sqrt() as f32;
    gpui::hsla(
        theme.accent.h,
        theme.accent.s.max(0.45),
        0.84 - 0.42 * intensity,
        1.0,
    )
}

fn activity(report: &UsageReport, theme: &Theme) -> gpui::Div {
    let today = chrono::Utc::now().date_naive();
    let start = today - chrono::Days::new(370);
    let peak = report
        .days
        .iter()
        .map(|day| day.totals.tokens())
        .max()
        .unwrap_or(0);
    div()
        .mx(px(8.0))
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(2.0))
                .children((0..53).map(|week| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .children((0..7).map(|day| {
                            let date = start + chrono::Days::new(week * 7 + day);
                            let key = date.format("%Y-%m-%d").to_string();
                            let tokens = report
                                .days
                                .iter()
                                .find(|group| group.label == key)
                                .map(|group| group.totals.tokens())
                                .unwrap_or(0);
                            let fill = activity_fill(theme, tokens, peak);
                            div()
                                .id(SharedString::from(format!("usage-day-{key}")))
                                .size(px(8.0))
                                .rounded(px(2.0))
                                .bg(fill)
                                .cursor_default()
                                .hover(|cell| cell.border_1().border_color(theme.text))
                                .tooltip(widgets::text_tooltip_above(format!(
                                    "{} (UTC): {tokens} tokens",
                                    date.format("%b %d, %Y")
                                )))
                        }))
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(4.0))
                .text_size(px(10.5))
                .line_height(px(16.0))
                .text_color(theme.text_muted)
                .child("Fewer tokens")
                .children([0, 1, 25, 50, 100].into_iter().map(|tokens| {
                    div()
                        .size(px(8.0))
                        .rounded(px(2.0))
                        .bg(activity_fill(theme, tokens, 100))
                }))
                .child("More tokens"),
        )
}

impl Render for UsagePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut content = widgets::page_column()
            .child(widgets::page_header(&theme, "Usage", None))
            .child(widgets::page_subtitle(
                &theme,
                "Your token history, models, activity and account limits on this device.",
            ))
            .child(self.filters(&theme, cx))
            .child(
                widgets::text_action(
                    &theme,
                    widgets::ActionTone::Quiet,
                    if self.loading {
                        "Refreshing…"
                    } else {
                        "Refresh"
                    },
                )
                .id("usage-refresh")
                .mt(px(10.0))
                .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
            );
        if let Some(error) = &self.error {
            content = content.child(widgets::error_strip(&theme, error.clone()));
        }
        if let Some(report) = &self.report {
            content = content.child(self.body(report, &theme, cx));
        }
        let rail = popover::rail(self, "usage-page-rail", &theme, cx);
        div()
            .id("usage-page-host")
            .relative()
            .size_full()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.scroll.set_list_hovered(*hovered) {
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("usage-page")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll.scroll)
                    .child(content),
            )
            .children(rail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_gets_darker_as_daily_tokens_increase() {
        for theme in [Theme::dark(), Theme::light()] {
            let low = activity_fill(&theme, 1, 100);
            let medium = activity_fill(&theme, 25, 100);
            let high = activity_fill(&theme, 100, 100);
            assert!(low.l > medium.l && medium.l > high.l);
            assert_eq!(high.a, 1.0);
            assert_eq!(high.h, theme.accent.h);
            assert_eq!(activity_fill(&theme, 0, 0), theme.text_muted.opacity(0.12));
            assert_eq!(activity_fill(&theme, 100, 0), high);
        }
    }

    #[test]
    fn formats_tokens_and_agent_time() {
        assert_eq!(number(779_700_000), "779.70M");
        assert_eq!(number(42), "42");
        assert_eq!(duration(6_240_000), "1h 44m");
    }
}
