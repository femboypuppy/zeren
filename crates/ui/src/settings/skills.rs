//! Install and manage shared Agent Skills from GitHub.
use super::widgets;
use crate::{
    composer::{ComposerInput, ComposerInputEvent},
    popover,
    state::AppState,
    theme::Theme,
};
use gpui::{
    Context, Entity, Render, SharedString, Subscription, Task, Window, div, prelude::*, px,
};
use zeren_engine::skill_library::InstalledSkill;
use zeren_rpc::methods;

pub struct SkillsPage {
    state: Entity<AppState>,
    source: Entity<ComposerInput>,
    skills: Vec<InstalledSkill>,
    scroll: widgets::PageScroll,
    busy: bool,
    loaded: bool,
    error: Option<String>,
    message: Option<String>,
    task: Option<Task<()>>,
    _source: Subscription,
    _state: Subscription,
}
impl popover::ScrollRailHost for SkillsPage {
    fn rail_bar(&mut self) -> &mut popover::MenuScrollbarState {
        self.scroll.rail_bar()
    }
    fn rail_scroll(&self) -> Option<gpui::ScrollHandle> {
        Some(self.scroll.scroll.clone())
    }
}
impl SkillsPage {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let source = cx.new(|cx| {
            ComposerInput::new("https://github.com/owner/repository", cx).with_single_line()
        });
        let events = cx.subscribe(
            &source,
            |this: &mut Self, _, event: &ComposerInputEvent, cx| {
                if matches!(event, ComposerInputEvent::Submitted) {
                    this.install(cx);
                }
            },
        );
        let subscription = cx.observe(&state, |this: &mut Self, _, cx| {
            if !this.loaded && !this.busy {
                this.refresh(cx);
            }
        });
        let mut page = Self {
            state,
            source,
            skills: Vec::new(),
            scroll: Default::default(),
            busy: false,
            loaded: false,
            error: None,
            message: None,
            task: None,
            _source: events,
            _state: subscription,
        };
        page.refresh(cx);
        page
    }
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.call(methods::LIST_INSTALLED_SKILLS, serde_json::json!({}), cx);
    }
    fn install(&mut self, cx: &mut Context<Self>) {
        let source = self.source.read(cx).text().trim().to_owned();
        if source.is_empty() {
            self.error = Some("Paste a GitHub repository URL first.".into());
            cx.notify();
            return;
        }
        self.call(
            methods::INSTALL_SKILLS,
            serde_json::json!({"source":source}),
            cx,
        );
    }
    fn call(&mut self, method: &'static str, params: serde_json::Value, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(engine) = self.state.read(cx).engine().cloned() else {
            return;
        };
        self.busy = true;
        self.error = None;
        self.message = None;
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = engine
                .client()
                .call(method, params)
                .await
                .map_err(|error| error.to_string())
                .and_then(|value| {
                    serde_json::from_value::<Vec<InstalledSkill>>(value)
                        .map_err(|error| error.to_string())
                });
            let mut message = None;
            let result = if method == methods::INSTALL_SKILLS {
                match result {
                    Ok(installed) => {
                        message = Some(format!(
                            "Installed {}. Type $ in a chat to choose a skill.",
                            installed
                                .iter()
                                .map(|skill| skill.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                        engine
                            .client()
                            .call(methods::LIST_INSTALLED_SKILLS, serde_json::json!({}))
                            .await
                            .map_err(|error| error.to_string())
                            .and_then(|value| {
                                serde_json::from_value(value).map_err(|error| error.to_string())
                            })
                    }
                    Err(error) => Err(error),
                }
            } else {
                result
            };
            this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(skills) => {
                        this.skills = skills;
                        this.loaded = true;
                        this.message = message;
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }
}
impl Render for SkillsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let input = self.source.clone();
        let mut content = widgets::page_column().child(widgets::page_header(&theme,"Skills",Some(self.skills.len())))
            .child(widgets::page_subtitle(&theme,"Add reusable instructions from GitHub and use them across your agents."))
            .child(widgets::section_card(&theme).p(px(16.0))
                .child(div().text_size(px(13.0)).child("Import from GitHub"))
                .child(div().mt(px(10.0)).child(popover::dialog_field(input.into_any_element())))
                .child(div().mt(px(12.0)).flex().flex_wrap().gap(px(8.0))
                    .child(widgets::text_action(&theme,widgets::ActionTone::Solid,if self.busy {"Working…"} else {"Install skills"})
                        .id("skills-install").on_click(cx.listener(|this,_,_,cx|this.install(cx))))
                    .child(widgets::text_action(&theme,widgets::ActionTone::Quiet,"Try Ponytail")
                        .id("skills-ponytail").on_click(cx.listener(|this,_,_,cx| {this.source.update(cx,|input,cx|input.set_text("https://github.com/DietrichGebert/ponytail",cx));cx.notify();}))))
                .child(div().mt(px(10.0)).text_size(px(11.5)).text_color(theme.text_muted)
                    .child("Imports SKILL.md and supporting files into your shared skill library. Repository setup scripts and plugin hooks are not installed.")))
            .child(widgets::text_action(&theme,widgets::ActionTone::Quiet,"Refresh library").id("skills-refresh").mt(px(12.0))
                .on_click(cx.listener(|this,_,_,cx|this.refresh(cx))));
        if let Some(error) = &self.error {
            content = content.child(widgets::error_strip(&theme, error.clone()));
        }
        if let Some(message) = &self.message {
            content = content.child(widgets::page_subtitle(&theme, message.clone()));
        }
        if self.loaded && self.skills.is_empty() {
            content = content.child(widgets::page_subtitle(
                &theme,
                "No imported skills yet. Paste a repository link above to get started.",
            ));
        }
        content = content.child(widgets::section_label(&theme, "Installed skills"));
        for skill in self.skills.clone() {
            let name = skill.name.clone();
            let toggle = name.clone();
            let remove = name.clone();
            let enabled = skill.enabled;
            content = content.child(
                widgets::section_card(&theme)
                    .mt(px(8.0))
                    .p(px(14.0))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(10.0))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(14.0))
                                    .child(SharedString::from(name.clone())),
                            )
                            .child(
                                widgets::text_action(
                                    &theme,
                                    widgets::ActionTone::Quiet,
                                    if enabled { "Disable" } else { "Enable" },
                                )
                                .id(SharedString::from(format!("skill-toggle-{name}")))
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.call(
                                            methods::CHANGE_INSTALLED_SKILL,
                                            serde_json::json!({"name":toggle,"enabled":!enabled}),
                                            cx,
                                        )
                                    },
                                )),
                            )
                            .child(
                                widgets::text_action(&theme, widgets::ActionTone::Quiet, "Remove")
                                    .id(SharedString::from(format!("skill-remove-{name}")))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.call(
                                            methods::CHANGE_INSTALLED_SKILL,
                                            serde_json::json!({"name":remove}),
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(12.0))
                            .text_color(theme.text_muted)
                            .child(SharedString::from(skill.description)),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child(format!(
                                "{} · {} · {}",
                                skill.repository,
                                skill.revision.chars().take(8).collect::<String>(),
                                if enabled { "Enabled" } else { "Disabled" }
                            )),
                    ),
            );
        }
        let rail = popover::rail(self, "skills-page-rail", &theme, cx);
        div()
            .id("skills-page-host")
            .relative()
            .size_full()
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.scroll.set_list_hovered(*hovered) {
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id("skills-page")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll.scroll)
                    .child(content),
            )
            .children(rail)
    }
}
