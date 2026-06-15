use gpui::{App, Context, Entity, SharedString, Task as GpuiTask, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, IndexPath, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogClose,
    h_flex,
    input::{Input, InputState},
    progress::Progress,
    select::{Select, SelectItem, SelectState},
    spinner::Spinner,
    v_flex,
};

use crate::{
    ai::{self, GeneratedTask},
    helpers::i18n_ai,
    state::{AiSettings, SidebarSelection, Task, TideDataStore, TideStore, update_data_and_save},
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

struct AiTaskDialog {
    description_input: Entity<InputState>,
    group_select: Entity<SelectState<Vec<GroupOption>>>,
    loading: bool,
    error: Option<String>,
    generation_task: Option<GpuiTask<()>>,
}

impl AiTaskDialog {
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
            loading: false,
            error: None,
            generation_task: None,
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

        let Some(group_id) = self.group_select.read(cx).selected_value().cloned() else {
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
        cx.notify();

        let weak = cx.entity().downgrade();
        self.generation_task = Some(cx.spawn_in(window, async move |_, cx| {
            let result = cx
                .background_spawn(async move { ai::generate_tasks(&settings, &description) })
                .await;

            let _ = weak.update_in(cx, |this, window, cx| {
                this.loading = false;
                match result {
                    Ok(tasks) if !tasks.is_empty() => {
                        save_generated_tasks(cx, group_id, tasks);
                        window.close_dialog(cx);
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
}

impl Render for AiTaskDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
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
                        .child(i18n_ai(cx, "hint")),
                )
            })
            .when(self.loading, |this| {
                this.child(
                    v_flex()
                        .min_h(px(250.))
                        .w_full()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .child(Spinner::new().large().color(cx.theme().primary))
                        .child(
                            div()
                                .text_base()
                                .font_medium()
                                .child(i18n_ai(cx, "generating_title")),
                        )
                        .child(
                            div()
                                .max_w(px(360.))
                                .text_center()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(i18n_ai(cx, "generating_desc")),
                        )
                        .child(
                            Progress::new("ai-task-generation-progress")
                                .small()
                                .loading(true)
                                .mt_2()
                                .max_w(px(320.)),
                        ),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(error))
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        div().h_8().w(px(104.)).child(
                            DialogClose::new().child(
                                Button::new("cancel-ai-tasks")
                                    .label(if self.loading {
                                        i18n_ai(cx, "cancel_generation")
                                    } else {
                                        i18n_ai(cx, "cancel")
                                    })
                                    .w_full(),
                            ),
                        ),
                    )
                    .when(!self.loading, |this| {
                        this.child(
                            Button::new("generate-ai-tasks")
                                .label(i18n_ai(cx, "generate"))
                                .primary()
                                .w(px(136.))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.generate(window, cx);
                                })),
                        )
                    }),
            )
    }
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
                data.insert_subtask(&parent_id, subtask_index, subtask);
            }
        }
    });
}

pub fn open_ai_task_dialog(window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| AiTaskDialog::new(window, cx));
    let dialog_height = px(480.);
    let margin_top = ((window.viewport_size().height - dialog_height) / 2.).max(px(0.));

    window.open_dialog(cx, move |dialog, _, cx| {
        dialog
            .title(i18n_ai(cx, "dialog_title"))
            .child(view.clone())
            .w(px(560.))
            .margin_top(margin_top)
    });
}
