use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::Local;
use gpui::{
    Anchor, AnyElement, App, Bounds, Context, ElementId, Entity, SharedString, Subscription,
    Task as GpuiTask, Window, WindowBounds, WindowKind, WindowOptions, div, prelude::*, px, size,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, IndexPath, Root, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    calendar::{Calendar, CalendarEvent, CalendarState, Date},
    h_flex,
    input::{Input, InputEvent, InputState},
    popover::Popover,
    progress::Progress,
    scroll::ScrollableElement,
    select::{Select, SelectItem, SelectState},
    spinner::Spinner,
    v_flex,
};

use crate::{
    ai::{self, AiStreamEvent, GeneratedSubtask, GeneratedTask},
    assets::CustomIconName,
    components::{DateTag, DueTimeInput},
    helpers::{due_date_label, i18n_ai, i18n_content, parse_due_time},
    state::{
        AiSettings, DueDate, SidebarSelection, Task, TideDataStore, TideStore, data::new_id,
        tide::update_status, update_data_and_save,
    },
};

#[derive(Clone)]
struct GroupOption {
    id: String,
    name: SharedString,
}

impl SelectItem for GroupOption {
    type Value = String;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }
}

struct DraftTask {
    draft_id: String,
    title_input: Entity<InputState>,
    details_input: Entity<InputState>,
    time_input: Entity<InputState>,
    calendar_state: Entity<CalendarState>,
    due_date: Option<DueDate>,
    is_starred: bool,
    subtasks: Vec<DraftSubtask>,
    _subs: Vec<Subscription>,
}

struct DraftSubtask {
    draft_id: String,
    title_input: Entity<InputState>,
    details_input: Entity<InputState>,
    time_input: Entity<InputState>,
    calendar_state: Entity<CalendarState>,
    due_date: Option<DueDate>,
    is_starred: bool,
    _subs: Vec<Subscription>,
}

struct GenerationActivity {
    provider: String,
    model: String,
    prompt_chars: usize,
    started_at: Instant,
    tick: u64,
    streamed_text: String,
    reasoning_text: String,
    tool_events: Vec<String>,
    preview_tasks: Vec<GeneratedTask>,
}

impl GenerationActivity {
    fn new(settings: &AiSettings, prompt_chars: usize) -> Self {
        Self {
            provider: settings.provider.label().to_string(),
            model: settings.model.clone(),
            prompt_chars,
            started_at: Instant::now(),
            tick: 0,
            streamed_text: String::new(),
            reasoning_text: String::new(),
            tool_events: Vec::new(),
            preview_tasks: Vec::new(),
        }
    }

    fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }
}

struct AiTaskWindow {
    description_input: Entity<InputState>,
    group_select: Entity<SelectState<Vec<GroupOption>>>,
    draft_tasks: Vec<DraftTask>,
    generation_activity: Option<GenerationActivity>,
    reasoning_expanded: bool,
    loading: bool,
    error: Option<String>,
    generation_task: Option<GpuiTask<()>>,
    activity_task: Option<GpuiTask<()>>,
}

impl AiTaskWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (groups, selected_index) = {
            let data = cx.global::<TideDataStore>().read(cx);
            let selected_id = match data.sidebar_selection() {
                SidebarSelection::Group(id) => Some(id.as_str()),
                _ => None,
            };
            let groups = data
                .task_groups()
                .iter()
                .map(|group| GroupOption {
                    id: group.id.clone(),
                    name: group.name.clone().into(),
                })
                .collect::<Vec<_>>();
            let selected_index = selected_id
                .and_then(|id| groups.iter().position(|group| group.id == id))
                .or(if groups.is_empty() { None } else { Some(0) })
                .map(|row| IndexPath::default().row(row));
            (groups, selected_index)
        };

        let description_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .placeholder(i18n_ai(cx, "description_placeholder"))
        });
        let group_select =
            cx.new(|cx| SelectState::new(groups, selected_index, window, cx).searchable(true));

        description_input.update(cx, |input, cx| input.focus(window, cx));

        Self {
            description_input,
            group_select,
            draft_tasks: Vec::new(),
            generation_activity: None,
            reasoning_expanded: false,
            loading: false,
            error: None,
            generation_task: None,
            activity_task: None,
        }
    }

    fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }

        let description = self.description_input.read(cx).value().trim().to_string();
        if description.is_empty() {
            self.error = Some(i18n_ai(cx, "description_required"));
            cx.notify();
            return;
        }

        if self.group_select.read(cx).selected_value().is_none() {
            self.error = Some(i18n_ai(cx, "group_required"));
            cx.notify();
            return;
        };

        let settings = cx.global::<TideStore>().read(cx).ai().clone();
        if !ai_settings_ready(&settings) {
            self.error = Some(i18n_ai(cx, "settings_required"));
            cx.notify();
            return;
        }

        self.loading = true;
        self.error = None;
        self.draft_tasks.clear();
        self.reasoning_expanded = false;
        self.generation_activity = Some(GenerationActivity::new(
            &settings,
            description.chars().count(),
        ));
        let stream_events = Arc::new(Mutex::new(Vec::new()));
        self.start_activity_ticker(stream_events.clone(), window, cx);
        cx.notify();

        let weak = cx.entity().downgrade();
        let stream_events_for_generation = stream_events.clone();
        let stream_events_for_finish = stream_events;
        self.generation_task = Some(cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move {
                    ai::generate_tasks_streaming(&settings, &description, move |event| {
                        if let Ok(mut events) = stream_events_for_generation.lock() {
                            events.push(event);
                        }
                    })
                })
                .await;

            let _ = weak.update_in(cx, |this, window, cx| {
                this.drain_stream_events(&stream_events_for_finish);
                this.loading = false;
                match result {
                    Ok(tasks) if !tasks.is_empty() => {
                        this.draft_tasks = tasks
                            .into_iter()
                            .map(|task| draft_task_from_generated(task, window, cx))
                            .collect();
                        this.error = None;
                        cx.notify();
                    }
                    Ok(_) => {
                        this.error = Some(i18n_ai(cx, "no_tasks"));
                        cx.notify();
                    }
                    Err(error) => {
                        this.error =
                            Some(format!("{} {error:#}", i18n_ai(cx, "generation_failed")));
                        cx.notify();
                    }
                }
            });
        }));
    }

    fn start_activity_ticker(
        &mut self,
        stream_events: Arc<Mutex<Vec<AiStreamEvent>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let weak = cx.entity().downgrade();
        self.activity_task = Some(cx.spawn_in(window, async move |_, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let should_continue = weak
                    .update_in(cx, |this, _, cx| {
                        if !this.loading {
                            return false;
                        }

                        if let Some(activity) = &mut this.generation_activity {
                            activity.tick += 1;
                        }
                        let had_events = this.drain_stream_events(&stream_events);
                        if had_events || this.generation_activity.is_some() {
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }
            }
        }));
    }

    fn drain_stream_events(&mut self, stream_events: &Arc<Mutex<Vec<AiStreamEvent>>>) -> bool {
        let Ok(mut events) = stream_events.lock() else {
            return false;
        };
        if events.is_empty() {
            return false;
        }
        let drained = events.drain(..).collect::<Vec<_>>();
        drop(events);
        for event in drained {
            self.apply_stream_event(event);
        }
        true
    }

    fn apply_stream_event(&mut self, event: AiStreamEvent) {
        let Some(activity) = &mut self.generation_activity else {
            return;
        };

        match event {
            AiStreamEvent::Text(text) => {
                activity.streamed_text.push_str(&text);
                activity.preview_tasks =
                    ai::preview_generated_tasks_from_stream(&activity.streamed_text);
            }
            AiStreamEvent::Reasoning(reasoning) => {
                activity.reasoning_text.push_str(&reasoning);
            }
            AiStreamEvent::ToolCall(tool_call) => {
                if !tool_call.trim().is_empty() {
                    activity.tool_events.push(tool_call);
                    if activity.tool_events.len() > 6 {
                        activity.tool_events.remove(0);
                    }
                }
            }
        }
    }

    fn confirm_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(group_id) = self.group_select.read(cx).selected_value().cloned() else {
            self.error = Some(i18n_ai(cx, "group_required"));
            cx.notify();
            return;
        };

        let tasks = self.collect_draft_tasks(cx);
        if tasks.is_empty() {
            self.error = Some(i18n_ai(cx, "no_tasks"));
            cx.notify();
            return;
        }

        save_generated_tasks(cx, group_id, tasks);
        window.remove_window();
    }

    fn collect_draft_tasks(&self, cx: &App) -> Vec<GeneratedTask> {
        self.draft_tasks
            .iter()
            .filter_map(|task| {
                let title = task.title_input.read(cx).value().trim().to_string();
                if title.is_empty() {
                    return None;
                }

                let subtasks = task
                    .subtasks
                    .iter()
                    .filter_map(|subtask| {
                        let title = subtask.title_input.read(cx).value().trim().to_string();
                        (!title.is_empty()).then(|| GeneratedSubtask {
                            title,
                            details: clean_input(&subtask.details_input, cx),
                            due_date: subtask.due_date,
                            is_starred: subtask.is_starred,
                        })
                    })
                    .collect();

                Some(GeneratedTask {
                    title,
                    details: clean_input(&task.details_input, cx),
                    due_date: task.due_date,
                    is_starred: task.is_starred,
                    subtasks,
                })
            })
            .collect()
    }

    fn find_draft_subtask_index(&self, draft_id: &str) -> Option<(usize, usize)> {
        self.draft_tasks
            .iter()
            .enumerate()
            .find_map(|(task_index, task)| {
                task.subtasks
                    .iter()
                    .position(|subtask| subtask.draft_id == draft_id)
                    .map(|subtask_index| (task_index, subtask_index))
            })
    }

    fn render_draft(&self, cx: &mut Context<Self>) -> AnyElement {
        let task_count = self.draft_tasks.len();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_medium()
                                    .child(i18n_ai(cx, "draft_title")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} {task_count}", i18n_ai(cx, "draft_count"))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(i18n_ai(cx, "draft_hint")),
                    ),
            )
            .child(
                v_flex().gap_3().children(
                    self.draft_tasks
                        .iter()
                        .enumerate()
                        .map(|(index, task)| self.render_draft_task(index, task, cx)),
                ),
            )
            .into_any_element()
    }

    fn render_draft_task(
        &self,
        index: usize,
        task: &DraftTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .mt_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}.", index + 1)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_2()
                            .child(Input::new(&task.title_input).w_full())
                            .child(Input::new(&task.details_input).h(px(58.)).w_full())
                            .child(self.render_draft_task_meta(index, task, cx)),
                    )
                    .child(
                        Button::new(ElementId::Name(format!("remove-ai-draft-{index}").into()))
                            .icon(CustomIconName::Trash)
                            .ghost()
                            .small()
                            .tooltip(i18n_ai(cx, "remove_draft_task"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if index < this.draft_tasks.len() {
                                    this.draft_tasks.remove(index);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(!task.subtasks.is_empty(), |this| {
                this.child(
                    v_flex()
                        .ml_6()
                        .gap_2()
                        .children(task.subtasks.iter().enumerate().map(
                            |(subtask_index, subtask)| {
                                self.render_draft_subtask(index, subtask_index, subtask, cx)
                            },
                        )),
                )
            })
            .into_any_element()
    }

    fn render_draft_subtask(
        &self,
        task_index: usize,
        subtask_index: usize,
        subtask: &DraftSubtask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .mt_2()
                    .size(px(5.))
                    .rounded_full()
                    .bg(cx.theme().muted_foreground),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_2()
                    .child(Input::new(&subtask.title_input).w_full())
                    .child(Input::new(&subtask.details_input).h(px(46.)).w_full())
                    .child(self.render_draft_subtask_meta(task_index, subtask_index, subtask, cx)),
            )
            .into_any_element()
    }

    fn render_draft_task_meta(
        &self,
        index: usize,
        task: &DraftTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(self.render_due_controls(
                format!("ai-draft-task-{index}"),
                task.due_date,
                task.calendar_state.clone(),
                task.time_input.clone(),
                move |this, due| {
                    if let Some(task) = this.draft_tasks.get_mut(index) {
                        task.due_date = due;
                    }
                },
                cx,
            ))
            .child(
                Button::new(ElementId::Name(format!("ai-draft-star-{index}").into()))
                    .icon(if task.is_starred {
                        CustomIconName::Star
                    } else {
                        CustomIconName::StarOutline
                    })
                    .small()
                    .ghost()
                    .tooltip(i18n_ai(cx, "toggle_star"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(task) = this.draft_tasks.get_mut(index) {
                            task.is_starred = !task.is_starred;
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn render_draft_subtask_meta(
        &self,
        task_index: usize,
        subtask_index: usize,
        subtask: &DraftSubtask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(self.render_due_controls(
                format!("ai-draft-subtask-{task_index}-{subtask_index}"),
                subtask.due_date,
                subtask.calendar_state.clone(),
                subtask.time_input.clone(),
                move |this, due| {
                    if let Some(task) = this.draft_tasks.get_mut(task_index)
                        && let Some(subtask) = task.subtasks.get_mut(subtask_index)
                    {
                        subtask.due_date = due;
                    }
                },
                cx,
            ))
            .child(
                Button::new(ElementId::Name(
                    format!("ai-draft-subtask-star-{task_index}-{subtask_index}").into(),
                ))
                .icon(if subtask.is_starred {
                    CustomIconName::Star
                } else {
                    CustomIconName::StarOutline
                })
                .small()
                .ghost()
                .tooltip(i18n_ai(cx, "toggle_star"))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(task) = this.draft_tasks.get_mut(task_index)
                        && let Some(subtask) = task.subtasks.get_mut(subtask_index)
                    {
                        subtask.is_starred = !subtask.is_starred;
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    fn render_due_controls(
        &self,
        id_prefix: String,
        due_date: Option<DueDate>,
        calendar_state: Entity<CalendarState>,
        time_input: Entity<InputState>,
        set_due_date: impl Fn(&mut Self, Option<DueDate>) + Copy + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let today = Local::now().date_naive();
        let tomorrow = today + chrono::Duration::days(1);
        let current_time = due_date.and_then(|due| due.time);
        let entity = cx.entity();
        let entity_for_remove = entity.clone();
        let entity_for_time = entity;
        let calendar_for_today = calendar_state.clone();
        let time_for_today = time_input.clone();
        let calendar_for_tomorrow = calendar_state.clone();
        let time_for_tomorrow = time_input.clone();
        let calendar_for_remove = calendar_state.clone();
        let time_for_remove = time_input.clone();
        let trigger = Button::new(ElementId::Name(format!("{id_prefix}-calendar").into()))
            .icon(IconName::Calendar)
            .ghost()
            .small()
            .cursor_pointer();

        let content = h_flex()
            .gap_2()
            .items_center()
            .when_some(due_date, |this, due| {
                this.child(
                    DateTag::new(due)
                        .removable()
                        .remove_id(ElementId::Name(format!("{id_prefix}-remove-due").into()))
                        .on_remove(move |window, cx| {
                            sync_draft_due_inputs(
                                None,
                                &calendar_for_remove,
                                &time_for_remove,
                                window,
                                cx,
                            );
                            let _ = entity_for_remove.update(cx, |this, cx| {
                                set_due_date(this, None);
                                cx.notify();
                            });
                        }),
                )
            })
            .when(due_date.is_none(), |this| {
                this.child(draft_due_pill(
                    ElementId::Name(format!("{id_prefix}-today").into()),
                    i18n_content(cx, "today"),
                    cx.theme().border,
                    cx.listener(move |this, _, window, cx| {
                        let due = Some(DueDate::new(today, current_time));
                        sync_draft_due_inputs(
                            due,
                            &calendar_for_today,
                            &time_for_today,
                            window,
                            cx,
                        );
                        set_due_date(this, due);
                        cx.notify();
                    }),
                ))
                .child(draft_due_pill(
                    ElementId::Name(format!("{id_prefix}-tomorrow").into()),
                    i18n_content(cx, "tomorrow"),
                    cx.theme().border,
                    cx.listener(move |this, _, window, cx| {
                        let due = Some(DueDate::new(tomorrow, current_time));
                        sync_draft_due_inputs(
                            due,
                            &calendar_for_tomorrow,
                            &time_for_tomorrow,
                            window,
                            cx,
                        );
                        set_due_date(this, due);
                        cx.notify();
                    }),
                ))
            })
            .child(draft_calendar_popover(
                format!("{id_prefix}-calendar-popover"),
                trigger,
                calendar_state,
                time_input,
                due_date,
                entity_for_time,
                set_due_date,
            ));

        content.into_any_element()
    }

    fn render_task_meta(
        &self,
        due_date: Option<DueDate>,
        is_starred: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        h_flex()
            .gap_2()
            .flex_wrap()
            .when_some(due_date, |this, due_date| {
                this.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .border_1()
                        .border_color(cx.theme().border)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(due_date_label(cx, due_date)),
                )
            })
            .when(is_starred, |this| {
                this.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .border_1()
                        .border_color(cx.theme().warning)
                        .text_xs()
                        .text_color(cx.theme().warning)
                        .child(i18n_ai(cx, "starred")),
                )
            })
            .into_any_element()
    }

    fn render_generation_activity(&self, cx: &mut Context<Self>) -> AnyElement {
        let activity = self.generation_activity.as_ref();
        let elapsed = activity.map_or(0, GenerationActivity::elapsed_secs);
        let has_streamed_text = activity.is_some_and(|activity| !activity.streamed_text.is_empty());
        let stage_key = if has_streamed_text {
            "activity_stage_streaming"
        } else {
            match elapsed {
                0..=2 => "activity_stage_sent",
                3..=8 => "activity_stage_waiting",
                9..=20 => "activity_stage_structuring",
                _ => "activity_stage_long",
            }
        };

        v_flex()
            .min_h(px(360.))
            .w_full()
            .justify_center()
            .gap_4()
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(Spinner::new().large().color(cx.theme().primary))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_medium()
                                    .child(i18n_ai(cx, "generating_title")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} {elapsed}s",
                                        i18n_ai(cx, "activity_elapsed")
                                    )),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .overflow_hidden()
                    .gap_2()
                    .p_3()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .child(activity_line(
                        i18n_ai(cx, "activity_request_sent"),
                        true,
                        cx,
                    ))
                    .when_some(activity, |this, activity| {
                        this.child(activity_line(
                            format!(
                                "{} {} / {}",
                                i18n_ai(cx, "activity_model"),
                                activity.provider,
                                activity.model
                            ),
                            true,
                            cx,
                        ))
                        .child(activity_line(
                            format!(
                                "{} {}",
                                i18n_ai(cx, "activity_prompt_size"),
                                activity.prompt_chars
                            ),
                            true,
                            cx,
                        ))
                    })
                    .child(activity_line(i18n_ai(cx, stage_key), true, cx))
                    .child(activity_line(
                        i18n_ai(cx, "activity_stream_note"),
                        false,
                        cx,
                    )),
            )
            .when_some(activity, |this, activity| {
                this.child(self.render_stream_output(activity, cx.entity(), true, cx))
            })
            .child(
                Progress::new("ai-task-generation-progress")
                    .small()
                    .loading(true)
                    .max_w(px(360.)),
            )
            .into_any_element()
    }

    fn render_stream_output(
        &self,
        activity: &GenerationActivity,
        entity: Entity<Self>,
        show_preview: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_2()
            .when(!activity.reasoning_text.trim().is_empty(), |this| {
                let reasoning = activity.reasoning_text.trim().to_string();
                let expanded = self.reasoning_expanded;
                this.child(
                    v_flex()
                        .w_full()
                        .overflow_hidden()
                        .rounded_lg()
                        .border_1()
                        .border_color(cx.theme().border)
                        .shadow_md()
                        .bg(cx.theme().background)
                        .child(
                            h_flex()
                                .id("ai-task-reasoning-toggle")
                                .items_center()
                                .justify_between()
                                .px_4()
                                .h(px(52.))
                                .cursor_pointer()
                                .when(expanded, |this| {
                                    this.border_b_1().border_color(cx.theme().border)
                                })
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.reasoning_expanded = !this.reasoning_expanded;
                                        cx.notify();
                                    });
                                })
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Icon::new(CustomIconName::Brain)
                                                .size_4()
                                                .text_color(cx.theme().muted_foreground),
                                        )
                                        .child(
                                            div()
                                                .text_base()
                                                .font_medium()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(i18n_ai(cx, "activity_reasoning")),
                                        ),
                                )
                                .child(
                                    Icon::new(if expanded {
                                        IconName::ChevronUp
                                    } else {
                                        IconName::ChevronDown
                                    })
                                    .size_4()
                                    .text_color(cx.theme().muted_foreground),
                                ),
                        )
                        .when(expanded, |this| {
                            this.child(
                                div()
                                    .px_4()
                                    .py_3()
                                    .text_sm()
                                    .whitespace_normal()
                                    .text_color(cx.theme().foreground)
                                    .child(reasoning),
                            )
                        }),
                )
            })
            .when(show_preview, |this| {
                this.child(self.render_live_draft_preview(activity, cx))
            })
            .when(!activity.tool_events.is_empty(), |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .p_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_xs()
                                .font_medium()
                                .text_color(cx.theme().muted_foreground)
                                .child(i18n_ai(cx, "activity_tool_events")),
                        )
                        .children(activity.tool_events.iter().map(|event| {
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(event.clone())
                        })),
                )
            })
            .into_any_element()
    }

    fn render_live_draft_preview(
        &self,
        activity: &GenerationActivity,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let task_count = activity.preview_tasks.len();

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .child(i18n_ai(cx, "activity_draft_preview")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} {task_count}", i18n_ai(cx, "draft_count"))),
                    ),
            )
            .when(activity.preview_tasks.is_empty(), |this| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(i18n_ai(cx, "activity_draft_waiting")),
                )
            })
            .children(
                activity
                    .preview_tasks
                    .iter()
                    .enumerate()
                    .map(|(index, task)| self.render_preview_task(index, task, cx)),
            )
            .into_any_element()
    }

    fn render_preview_task(
        &self,
        index: usize,
        task: &GeneratedTask,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .mt_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{}.", index + 1)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(div().text_sm().font_medium().child(task.title.clone()))
                            .when_some(task.details.clone(), |this, details| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .whitespace_normal()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(details),
                                )
                            })
                            .child(self.render_task_meta(task.due_date, task.is_starred, cx)),
                    ),
            )
            .when(!task.subtasks.is_empty(), |this| {
                this.child(
                    v_flex()
                        .ml_6()
                        .gap_1()
                        .children(task.subtasks.iter().map(|subtask| {
                            h_flex()
                                .items_start()
                                .gap_2()
                                .child(
                                    div()
                                        .mt_2()
                                        .size(px(5.))
                                        .rounded_full()
                                        .bg(cx.theme().muted_foreground),
                                )
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_medium()
                                                .child(subtask.title.clone()),
                                        )
                                        .when_some(subtask.details.clone(), |this, details| {
                                            this.child(
                                                div()
                                                    .text_xs()
                                                    .whitespace_normal()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(details),
                                            )
                                        })
                                        .child(self.render_task_meta(
                                            subtask.due_date,
                                            subtask.is_starred,
                                            cx,
                                        )),
                                )
                        })),
                )
            })
            .into_any_element()
    }

    fn render_footer(&self, entity: Entity<Self>, cx: &App) -> AnyElement {
        let has_draft = !self.draft_tasks.is_empty();
        let confirm_entity = entity.clone();
        let generate_entity = entity;

        h_flex()
            .w_full()
            .justify_end()
            .gap_2()
            .child(
                div().h_8().w(px(104.)).child(
                    Button::new("cancel-ai-tasks")
                        .label(if self.loading {
                            i18n_ai(cx, "cancel_generation")
                        } else {
                            i18n_ai(cx, "cancel")
                        })
                        .w_full()
                        .on_click(|_, window, _| window.remove_window()),
                ),
            )
            .when(!self.loading, |this| {
                this.when(has_draft, |this| {
                    this.child(
                        Button::new("confirm-ai-draft")
                            .label(i18n_ai(cx, "create_draft"))
                            .primary()
                            .w(px(152.))
                            .on_click(move |_, window, cx| {
                                confirm_entity.update(cx, |this, cx| {
                                    this.confirm_draft(window, cx);
                                });
                            }),
                    )
                })
                .child(
                    Button::new("generate-ai-tasks")
                        .label(if has_draft {
                            i18n_ai(cx, "regenerate")
                        } else {
                            i18n_ai(cx, "generate")
                        })
                        .when(!has_draft, |this| this.primary())
                        .w(px(136.))
                        .on_click(move |_, window, cx| {
                            generate_entity.update(cx, |this, cx| {
                                this.generate(window, cx);
                            });
                        }),
                )
            })
            .into_any_element()
    }
}

impl Render for AiTaskWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_draft = !self.draft_tasks.is_empty();
        let entity = cx.entity();

        let content = v_flex()
            .w_full()
            .gap_4()
            .when(!self.loading, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(i18n_ai(cx, "description_label")),
                        )
                        .child(Input::new(&self.description_input).h(px(156.)).w_full()),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .font_medium()
                                .child(i18n_ai(cx, "group_label")),
                        )
                        .child(
                            Select::new(&self.group_select)
                                .placeholder(i18n_ai(cx, "group_placeholder"))
                                .w_full(),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if has_draft {
                            i18n_ai(cx, "draft_edit_hint")
                        } else {
                            i18n_ai(cx, "hint")
                        }),
                )
            })
            .when(self.loading, |this| {
                this.child(self.render_generation_activity(cx))
            })
            .when(!self.loading && has_draft, |this| {
                this.when_some(self.generation_activity.as_ref(), |this, activity| {
                    this.child(self.render_stream_output(activity, cx.entity(), false, cx))
                })
                .child(self.render_draft(cx))
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            });

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .py_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_base()
                            .font_medium()
                            .child(i18n_ai(cx, "dialog_title")),
                    ),
            )
            .child(
                div().flex_1().min_h_0().overflow_hidden().child(
                    v_flex()
                        .id("ai-task-window-scroll")
                        .size_full()
                        .overflow_y_scrollbar()
                        .px_5()
                        .py_4()
                        .child(content),
                ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .px_5()
                    .py_4()
                    .child(self.render_footer(entity, cx)),
            )
    }
}

fn activity_line(label: impl Into<SharedString>, active: bool, cx: &App) -> AnyElement {
    h_flex()
        .items_start()
        .gap_2()
        .child(div().mt_1().size(px(6.)).rounded_full().bg(if active {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        }))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .line_clamp(2)
                .text_sm()
                .text_color(if active {
                    cx.theme().foreground
                } else {
                    cx.theme().muted_foreground
                })
                .child(label.into()),
        )
        .into_any_element()
}

fn draft_due_pill(
    id: ElementId,
    label: String,
    border: gpui::Hsla,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_full()
        .border_1()
        .border_color(border)
        .text_xs()
        .cursor_pointer()
        .hover(|this| this.bg(gpui::rgba(0x00000010)))
        .child(label)
        .on_click(on_click)
        .into_any_element()
}

fn draft_calendar_popover(
    id: String,
    trigger: Button,
    calendar_state: Entity<CalendarState>,
    time_input: Entity<InputState>,
    due_date: Option<DueDate>,
    entity: Entity<AiTaskWindow>,
    set_due_date: impl Fn(&mut AiTaskWindow, Option<DueDate>) + Copy + 'static,
) -> AnyElement {
    let popover = Popover::new(id)
        .anchor(Anchor::TopLeft)
        .trigger(trigger)
        .content(move |_, _, _| {
            div()
                .p_2()
                .child(Calendar::new(&calendar_state).number_of_months(1))
                .when(due_date.is_some(), |this| {
                    this.child(div().pt_2().child(
                        DueTimeInput::new("ai-draft-due-time", time_input.clone()).on_select({
                            let entity = entity.clone();
                            move |value, _window, cx| {
                                let Some(mut due) = due_date else {
                                    return;
                                };
                                due.time = parse_due_time(value);
                                let _ = entity.update(cx, |this, cx| {
                                    set_due_date(this, Some(due));
                                    cx.notify();
                                });
                            }
                        }),
                    ))
                })
        })
        .on_open_change(move |open, _, cx| {
            let is_open = *open;
            update_status(cx, move |status, _| {
                status.set_task_calendar_open(is_open);
            });
        });

    popover.into_any_element()
}

fn sync_draft_due_inputs(
    due: Option<DueDate>,
    calendar_state: &Entity<CalendarState>,
    time_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    calendar_state.update(cx, |state, cx| {
        state.set_date(Date::Single(due.map(|due| due.date)), window, cx);
    });
    time_input.update(cx, |state, cx| {
        let value = due
            .and_then(|due| due.time)
            .map(|time| time.format("%H:%M").to_string())
            .unwrap_or_default();
        state.set_value(value, window, cx);
    });
}

fn draft_task_from_generated(
    task: GeneratedTask,
    window: &mut Window,
    cx: &mut Context<AiTaskWindow>,
) -> DraftTask {
    let draft_id = new_id();
    let title_input = draft_input(
        &task.title,
        i18n_ai(cx, "draft_title_placeholder"),
        false,
        window,
        cx,
    );
    let details_input = draft_input(
        task.details.as_deref().unwrap_or_default(),
        i18n_ai(cx, "draft_details_placeholder"),
        true,
        window,
        cx,
    );
    let time_input = draft_time_input(task.due_date, window, cx);
    let calendar_state = draft_calendar_state(task.due_date, window, cx);
    let mut subs = Vec::new();

    {
        let draft_id = draft_id.clone();
        subs.push(cx.subscribe_in(
            &calendar_state,
            window,
            move |this: &mut AiTaskWindow, _, event: &CalendarEvent, _window, cx| {
                let CalendarEvent::Selected(date) = event;
                let Some(picked) = date.start() else {
                    return;
                };
                let Some(index) = this
                    .draft_tasks
                    .iter()
                    .position(|task| task.draft_id == draft_id)
                else {
                    return;
                };
                let time_value = this.draft_tasks[index]
                    .time_input
                    .read(cx)
                    .value()
                    .to_string();
                this.draft_tasks[index].due_date =
                    Some(DueDate::new(picked, parse_due_time(&time_value)));
                cx.notify();
            },
        ));
    }

    {
        let draft_id = draft_id.clone();
        subs.push(cx.subscribe_in(
            &time_input,
            window,
            move |this: &mut AiTaskWindow, _, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let Some(index) = this
                    .draft_tasks
                    .iter()
                    .position(|task| task.draft_id == draft_id)
                else {
                    return;
                };
                let Some(mut due) = this.draft_tasks[index].due_date else {
                    return;
                };
                let value = this.draft_tasks[index]
                    .time_input
                    .read(cx)
                    .value()
                    .to_string();
                due.time = parse_due_time(&value);
                this.draft_tasks[index].due_date = Some(due);
                cx.notify();
            },
        ));
    }

    let subtasks = task
        .subtasks
        .into_iter()
        .map(|subtask| draft_subtask_from_generated(subtask, window, cx))
        .collect();

    DraftTask {
        draft_id,
        title_input,
        details_input,
        time_input,
        calendar_state,
        due_date: task.due_date,
        is_starred: task.is_starred,
        subtasks,
        _subs: subs,
    }
}

fn draft_subtask_from_generated(
    subtask: GeneratedSubtask,
    window: &mut Window,
    cx: &mut Context<AiTaskWindow>,
) -> DraftSubtask {
    let draft_id = new_id();
    let title_input = draft_input(
        &subtask.title,
        i18n_ai(cx, "draft_subtask_title_placeholder"),
        false,
        window,
        cx,
    );
    let details_input = draft_input(
        subtask.details.as_deref().unwrap_or_default(),
        i18n_ai(cx, "draft_details_placeholder"),
        true,
        window,
        cx,
    );
    let time_input = draft_time_input(subtask.due_date, window, cx);
    let calendar_state = draft_calendar_state(subtask.due_date, window, cx);
    let mut subs = Vec::new();

    {
        let draft_id = draft_id.clone();
        subs.push(cx.subscribe_in(
            &calendar_state,
            window,
            move |this: &mut AiTaskWindow, _, event: &CalendarEvent, _window, cx| {
                let CalendarEvent::Selected(date) = event;
                let Some(picked) = date.start() else {
                    return;
                };
                let Some((task_index, subtask_index)) = this.find_draft_subtask_index(&draft_id)
                else {
                    return;
                };
                let time_value = this.draft_tasks[task_index].subtasks[subtask_index]
                    .time_input
                    .read(cx)
                    .value()
                    .to_string();
                this.draft_tasks[task_index].subtasks[subtask_index].due_date =
                    Some(DueDate::new(picked, parse_due_time(&time_value)));
                cx.notify();
            },
        ));
    }

    {
        let draft_id = draft_id.clone();
        subs.push(cx.subscribe_in(
            &time_input,
            window,
            move |this: &mut AiTaskWindow, _, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::Change) {
                    return;
                }
                let Some((task_index, subtask_index)) = this.find_draft_subtask_index(&draft_id)
                else {
                    return;
                };
                let Some(mut due) = this.draft_tasks[task_index].subtasks[subtask_index].due_date
                else {
                    return;
                };
                let value = this.draft_tasks[task_index].subtasks[subtask_index]
                    .time_input
                    .read(cx)
                    .value()
                    .to_string();
                due.time = parse_due_time(&value);
                this.draft_tasks[task_index].subtasks[subtask_index].due_date = Some(due);
                cx.notify();
            },
        ));
    }

    DraftSubtask {
        draft_id,
        title_input,
        details_input,
        time_input,
        calendar_state,
        due_date: subtask.due_date,
        is_starred: subtask.is_starred,
        _subs: subs,
    }
}

fn draft_input(
    value: &str,
    placeholder: String,
    multi_line: bool,
    window: &mut Window,
    cx: &mut Context<AiTaskWindow>,
) -> Entity<InputState> {
    let input = cx.new(|cx| {
        let input = InputState::new(window, cx).placeholder(placeholder);
        if multi_line {
            input.multi_line(true)
        } else {
            input
        }
    });
    input.update(cx, |input, cx| {
        input.set_value(value, window, cx);
    });
    input
}

fn draft_time_input(
    due: Option<DueDate>,
    window: &mut Window,
    cx: &mut Context<AiTaskWindow>,
) -> Entity<InputState> {
    let input = cx.new(|cx| InputState::new(window, cx));
    input.update(cx, |input, cx| {
        let value = due
            .and_then(|due| due.time)
            .map(|time| time.format("%H:%M").to_string())
            .unwrap_or_default();
        input.set_value(value, window, cx);
    });
    input
}

fn draft_calendar_state(
    due: Option<DueDate>,
    window: &mut Window,
    cx: &mut Context<AiTaskWindow>,
) -> Entity<CalendarState> {
    let calendar_state = cx.new(|cx| CalendarState::new(window, cx));
    calendar_state.update(cx, |state, cx| {
        state.set_date(Date::Single(due.map(|due| due.date)), window, cx);
    });
    calendar_state
}

fn clean_input(input: &Entity<InputState>, cx: &App) -> Option<String> {
    let value = input.read(cx).value().trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn ai_settings_ready(settings: &AiSettings) -> bool {
    !settings.endpoint.trim().is_empty()
        && !settings.model.trim().is_empty()
        && (!settings.provider.requires_api_key() || !settings.api_key.trim().is_empty())
}

fn save_generated_tasks(cx: &App, group_id: String, generated: Vec<GeneratedTask>) {
    update_data_and_save(cx, "create_ai_tasks", move |data, _| {
        for (index, generated_task) in generated.iter().enumerate() {
            let mut task = Task::new(group_id.clone(), generated_task.title.clone());
            task.details = generated_task.details.clone();
            task.due_date = generated_task.due_date;
            task.is_starred = generated_task.is_starred;
            let parent_id = task.id.clone();
            data.insert_task(index, task);

            for (subtask_index, generated_subtask) in generated_task.subtasks.iter().enumerate() {
                let mut subtask = Task::new(group_id.clone(), generated_subtask.title.clone());
                subtask.details = generated_subtask.details.clone();
                subtask.due_date = generated_subtask.due_date;
                subtask.is_starred = generated_subtask.is_starred;
                data.insert_subtask(&parent_id, subtask_index, subtask);
            }
        }
    });
}

pub fn open_ai_task_window(cx: &mut App) {
    let window_size = size(px(640.), px(760.));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        focus: true,
        show: true,
        is_resizable: true,
        is_movable: true,
        kind: WindowKind::Normal,
        window_min_size: Some(size(px(520.), px(520.))),
        ..Default::default()
    };

    let _ = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| AiTaskWindow::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
}
