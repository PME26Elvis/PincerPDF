#![forbid(unsafe_code)]
//! Accessible P4.2 Merge workspace with native and deterministic browser adapters.

use crate::native_bridge::{call_with_args, call_without_args, is_tauri};
use leptos::ev;
use leptos::prelude::*;
use pincerpdf_desktop_api::{
    CommandError, MergeEngineStatus, MergeInputRequest, MergeRunRequest, MergeRunResult,
    MergeSourceFeatures, PickedMergeDestination, PickedMergeSource,
};
use pincerpdf_domain::PageSelection;
use serde::Serialize;
use std::str::FromStr;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

#[derive(Clone)]
struct SourceRow {
    id: u64,
    picked: PickedMergeSource,
    selection: String,
    password: String,
}

#[derive(Clone)]
enum TaskState {
    Idle,
    Running {
        operation_id: String,
        cancelling: bool,
    },
    Completed(MergeRunResult),
    Failed(CommandError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunMergeArgs {
    request: MergeRunRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelMergeArgs {
    operation_id: String,
}

/// Renders the first usable PDF operation without weakening later parity gates.
#[component]
#[allow(
    clippy::too_many_lines,
    reason = "the Leptos view keeps one accessible three-step workflow together"
)]
pub(crate) fn MergeWorkspace(engine_status: ReadSignal<MergeEngineStatus>) -> impl IntoView {
    let (sources, set_sources) = signal(Vec::<SourceRow>::new());
    let (destination, set_destination) = signal(Option::<PickedMergeDestination>::None);
    let (task, set_task) = signal(TaskState::Idle);
    let (next_source_id, set_next_source_id) = signal(1_u64);
    let (next_operation_id, set_next_operation_id) = signal(1_u64);
    let (advanced_open, set_advanced_open) = signal(false);
    let (replace_existing, set_replace_existing) = signal(false);
    let (picker_busy, set_picker_busy) = signal(false);
    let native = is_tauri();

    let add_sources = move |_| {
        if picker_busy.get_untracked() || matches!(task.get_untracked(), TaskState::Running { .. })
        {
            return;
        }
        set_picker_busy.set(true);
        if native {
            leptos::task::spawn_local(async move {
                match call_without_args::<Vec<PickedMergeSource>>("pick_merge_sources").await {
                    Ok(picked) => {
                        append_sources(picked, set_sources, next_source_id, set_next_source_id);
                    }
                    Err(error) => set_task.set(TaskState::Failed(error)),
                }
                set_picker_busy.set(false);
            });
        } else {
            append_sources(
                browser_sources(),
                set_sources,
                next_source_id,
                set_next_source_id,
            );
            set_picker_busy.set(false);
        }
    };

    let choose_destination = move |_| {
        if matches!(task.get_untracked(), TaskState::Running { .. }) {
            return;
        }
        if native {
            leptos::task::spawn_local(async move {
                match call_without_args::<Option<PickedMergeDestination>>("pick_merge_destination")
                    .await
                {
                    Ok(Some(picked)) => {
                        set_destination.set(Some(picked));
                        set_task.set(TaskState::Idle);
                    }
                    Ok(None) => {}
                    Err(error) => set_task.set(TaskState::Failed(error)),
                }
            });
        } else {
            set_destination.set(Some(PickedMergeDestination {
                path_token: "browser-output".to_owned(),
                display_path: "merged-document.pdf".to_owned(),
            }));
            set_task.set(TaskState::Idle);
        }
    };

    let run_merge = move |_| {
        if !can_run(
            &sources.get_untracked(),
            destination.get_untracked().as_ref(),
            &engine_status.get_untracked(),
            &task.get_untracked(),
        ) {
            set_task.set(TaskState::Failed(CommandError::new(
                "invalid_input",
                "Add at least two valid sources and choose an output file.",
            )));
            return;
        }
        let operation_number = next_operation_id.get_untracked();
        set_next_operation_id.set(operation_number + 1);
        let operation_id = format!("merge-ui-{operation_number}");
        let request = make_request(
            operation_id.clone(),
            &sources.get_untracked(),
            destination
                .get_untracked()
                .as_ref()
                .expect("can_run requires a destination"),
            replace_existing.get_untracked(),
        );
        set_task.set(TaskState::Running {
            operation_id: operation_id.clone(),
            cancelling: false,
        });

        if native {
            leptos::task::spawn_local(async move {
                let result =
                    call_with_args::<MergeRunResult, _>("run_merge", &RunMergeArgs { request })
                        .await;
                match result {
                    Ok(report) => set_task.set(TaskState::Completed(report)),
                    Err(error) => set_task.set(TaskState::Failed(error)),
                }
            });
        } else {
            complete_browser_merge_after_delay(
                operation_id,
                browser_result(&sources.get_untracked()),
                task,
                set_task,
            );
        }
    };

    let cancel_merge = move |_| {
        let TaskState::Running {
            operation_id,
            cancelling: false,
        } = task.get_untracked()
        else {
            return;
        };
        set_task.set(TaskState::Running {
            operation_id: operation_id.clone(),
            cancelling: true,
        });
        if native {
            leptos::task::spawn_local(async move {
                let result = call_with_args::<bool, _>(
                    "cancel_merge",
                    &CancelMergeArgs {
                        operation_id: operation_id.clone(),
                    },
                )
                .await;
                if let Err(error) = result {
                    set_task.set(TaskState::Failed(error));
                }
            });
        } else {
            set_task.set(TaskState::Failed(CommandError::new(
                "cancelled",
                "The deterministic Merge task was cancelled.",
            )));
        }
    };

    view! {
        <section class="merge-workspace" aria-labelledby="merge-title" data-testid="merge-workspace">
            <div class="merge-heading">
                <div>
                    <p class="eyebrow">"P4 · Merge workspace"</p>
                    <h1 id="merge-title">"Combine PDFs in the exact order you choose."</h1>
                    <p>
                        "Add documents, refine each page range, then create one verified output. "
                        "Duplicates are intentional and source files are never modified."
                    </p>
                </div>
                <div class="merge-engine-card" data-testid="merge-engine-status">
                    <span class=move || {
                        if engine_status.get().ready { "status-dot is-ready" } else { "status-dot" }
                    }></span>
                    <span>
                        <strong>
                            {move || if engine_status.get().ready {
                                "Merge engine ready"
                            } else {
                                "Merge engine unavailable"
                            }}
                        </strong>
                        <small>
                            {move || engine_status.get().engine_version.unwrap_or_else(|| {
                                "Check the local QPDF installation".to_owned()
                            })}
                        </small>
                    </span>
                </div>
            </div>

            <div class="merge-layout">
                <div class="merge-main">
                    <section class="merge-card source-card" aria-labelledby="source-list-title">
                        <div class="merge-card-heading">
                            <div>
                                <span class="step-number">"1"</span>
                                <span>
                                    <h2 id="source-list-title">"Source documents"</h2>
                                    <p>"Order and page ranges are preserved exactly."</p>
                                </span>
                            </div>
                            <button
                                type="button"
                                class="secondary-action"
                                data-testid="add-merge-sources"
                                disabled=move || {
                                    picker_busy.get() || matches!(task.get(), TaskState::Running { .. })
                                }
                                on:click=add_sources
                            >
                                {move || if picker_busy.get() { "Opening…" } else { "Add PDF files" }}
                            </button>
                        </div>

                        <Show
                            when=move || !sources.get().is_empty()
                            fallback=move || view! {
                                <div class="merge-empty" data-testid="merge-empty-state">
                                    <span class="empty-icon" aria-hidden="true">"PDF"</span>
                                    <div>
                                        <strong>"No source documents yet"</strong>
                                        <p>"Choose at least two PDFs. Browser verification loads deterministic samples."</p>
                                    </div>
                                    <button type="button" class="text-action" on:click=add_sources>
                                        "Choose files"
                                    </button>
                                </div>
                            }
                        >
                            <ol class="source-list" data-testid="merge-source-list">
                                <For
                                    each=move || sources.get()
                                    key=|row| row.id
                                    children=move |row| source_row(
                                        row,
                                        sources,
                                        set_sources,
                                        next_source_id,
                                        set_next_source_id,
                                        task,
                                    )
                                />
                            </ol>
                        </Show>

                        <div class="source-summary" aria-live="polite">
                            <span>
                                <strong data-testid="merge-source-count">
                                    {move || sources.get().len()}
                                </strong>
                                " documents"
                            </span>
                            <span>
                                <strong data-testid="merge-page-total">
                                    {move || planned_page_count(&sources.get()).map_or_else(
                                        || "—".to_owned(),
                                        |pages| pages.to_string(),
                                    )}
                                </strong>
                                " planned pages"
                            </span>
                        </div>
                    </section>

                    <section class="merge-card output-card" aria-labelledby="output-title">
                        <div class="merge-card-heading">
                            <div>
                                <span class="step-number">"2"</span>
                                <span>
                                    <h2 id="output-title">"Output"</h2>
                                    <p>"A temporary sibling is verified before finalization."</p>
                                </span>
                            </div>
                        </div>
                        <div class="destination-control">
                            <div>
                                <span class="field-label">"Destination PDF"</span>
                                <strong data-testid="merge-output-path">
                                    {move || destination.get().map_or_else(
                                        || "No destination selected".to_owned(),
                                        |picked| picked.display_path,
                                    )}
                                </strong>
                            </div>
                            <button
                                type="button"
                                class="secondary-action"
                                data-testid="choose-merge-output"
                                disabled=move || matches!(task.get(), TaskState::Running { .. })
                                on:click=choose_destination
                            >
                                "Choose output"
                            </button>
                        </div>
                        <p class="safety-note" data-testid="merge-output-safety">
                            <span aria-hidden="true">"◇"</span>
                            {move || if replace_existing.get() {
                                "Existing output will be replaced only after the temporary PDF passes verification."
                            } else {
                                "If the destination already exists, the merge stops without replacing it."
                            }}
                        </p>
                    </section>

                    <section class="merge-card advanced-card">
                        <button
                            type="button"
                            class="accordion-trigger"
                            aria-expanded=move || advanced_open.get().to_string()
                            data-testid="merge-advanced-toggle"
                            on:click=move |_| set_advanced_open.update(|open| *open = !*open)
                        >
                            <span>
                                <strong>"Advanced safety policy"</strong>
                                <small>"Bookmarks, forms and conflicts remain explicit."</small>
                            </span>
                            <span class="accordion-chevron" aria-hidden="true">
                                {move || if advanced_open.get() { "−" } else { "+" }}
                            </span>
                        </button>
                        <Show when=move || advanced_open.get()>
                            <div data-testid="merge-advanced-panel">
                                <label class="overwrite-choice">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || replace_existing.get()
                                        disabled=move || matches!(task.get(), TaskState::Running { .. })
                                        data-testid="replace-existing-output"
                                        on:change=move |event| {
                                            set_replace_existing.set(event_target_checked(&event));
                                        }
                                    />
                                    <span>
                                        <strong>"Replace an existing destination"</strong>
                                        <small>
                                            "Explicit opt-in · verify and flush the temporary PDF before atomic replacement."
                                        </small>
                                    </span>
                                </label>
                                <dl class="policy-grid">
                                    <div><dt>"Bookmarks"</dt><dd>"Discard and report"</dd></div>
                                    <div><dt>"Interactive forms"</dt><dd>"Reject before processing"</dd></div>
                                    <div>
                                        <dt>"Existing output"</dt>
                                        <dd>{move || if replace_existing.get() {
                                            "Atomic replacement"
                                        } else {
                                            "Stop safely"
                                        }}</dd>
                                    </div>
                                    <div><dt>"Finalization"</dt><dd>"Verify, flush, atomic rename"</dd></div>
                                </dl>
                            </div>
                        </Show>
                    </section>
                </div>

                <aside class="merge-run-card" aria-labelledby="run-title">
                    <span class="step-number">"3"</span>
                    <h2 id="run-title">"Create merged PDF"</h2>
                    <p>"The action unlocks only when every visible requirement is satisfied."</p>

                    <div class="run-checks">
                        {run_check(
                            "At least two sources",
                            move || sources.get().len() >= 2,
                        )}
                        {run_check(
                            "Every source is valid",
                            move || sources.get().iter().all(|row| validate_source(row).is_none()),
                        )}
                        {run_check(
                            "Output selected",
                            move || destination.get().is_some(),
                        )}
                        {run_check(
                            "Native engine ready",
                            move || engine_status.get().ready,
                        )}
                    </div>

                    <button
                        type="button"
                        class="primary-action run-action"
                        data-testid="run-merge"
                        disabled=move || !can_run(
                            &sources.get(),
                            destination.get().as_ref(),
                            &engine_status.get(),
                            &task.get(),
                        )
                        on:click=run_merge
                    >
                        {move || if matches!(task.get(), TaskState::Running { .. }) {
                            "Merging…"
                        } else {
                            "Create merged PDF"
                        }}
                    </button>

                    {task_status(task, Callback::new(cancel_merge))}
                </aside>
            </div>
        </section>
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one ordered source row keeps its labels and controls in a single semantic list item"
)]
fn source_row(
    row: SourceRow,
    sources: ReadSignal<Vec<SourceRow>>,
    set_sources: WriteSignal<Vec<SourceRow>>,
    next_source_id: ReadSignal<u64>,
    set_next_source_id: WriteSignal<u64>,
    task: ReadSignal<TaskState>,
) -> impl IntoView {
    let id = row.id;
    let error = Memo::new(move |_| {
        sources
            .get()
            .iter()
            .find(|candidate| candidate.id == id)
            .and_then(validate_source)
    });
    let disabled = move || matches!(task.get(), TaskState::Running { .. });
    let file_name = row.picked.file_name.clone();
    let display_path = row.picked.display_path.clone();
    let page_count = row.picked.page_count;
    let encrypted = row.picked.features.encrypted;
    let bookmarks = row.picked.features.has_bookmarks;
    let password_required = row.picked.password_required;
    let initial_selection = row.selection.clone();
    let initial_password = StoredValue::new(row.password.clone());
    let pages_label = format!("Pages from {}", row.picked.file_name);
    let password_label = StoredValue::new(format!("Password for {}", row.picked.file_name));
    let move_up_label = format!("Move {} up", row.picked.file_name);
    let move_down_label = format!("Move {} down", row.picked.file_name);
    let duplicate_label = format!("Duplicate {}", row.picked.file_name);
    let remove_label = format!("Remove {}", row.picked.file_name);

    view! {
        <li
            class=move || if error.get().is_some() { "source-row has-error" } else { "source-row" }
            data-testid="merge-source-row"
        >
            <span class="drag-handle" aria-hidden="true">"⋮⋮"</span>
            <span class="source-order">
                {move || sources.get().iter().position(|candidate| candidate.id == id)
                    .map_or(0, |index| index + 1)}
            </span>
            <div class="source-identity">
                <strong>{file_name}</strong>
                <small title=display_path.clone()>{display_path.clone()}</small>
                <span class="source-badges">
                    <span>{page_count.map_or_else(|| "Locked".to_owned(), |pages| format!("{pages} pages"))}</span>
                    <Show when=move || encrypted>
                        <span class="is-warning">"Encrypted"</span>
                    </Show>
                    <Show when=move || bookmarks>
                        <span>"Bookmarks"</span>
                    </Show>
                </span>
            </div>
            <label class="range-field">
                <span>"Pages"</span>
                <input
                    type="text"
                    inputmode="numeric"
                    placeholder="All pages"
                    aria-label=pages_label
                    prop:value=initial_selection
                    disabled=disabled
                    data-testid="merge-page-selection"
                    on:input=move |event| {
                        update_source(set_sources, id, |source| {
                            source.selection = event_target_value(&event);
                        });
                    }
                />
            </label>
            <Show when=move || password_required>
                <label class="password-field">
                    <span>"Password"</span>
                    <input
                        type="password"
                        autocomplete="off"
                        aria-label=move || password_label.get_value()
                        prop:value=move || initial_password.get_value()
                        disabled=disabled
                        data-testid="merge-source-password"
                        on:input=move |event| {
                            update_source(set_sources, id, |source| {
                                source.password = event_target_value(&event);
                            });
                        }
                    />
                </label>
            </Show>
            <div class="source-actions" aria-label="Source order controls">
                <button
                    type="button"
                    title="Move up"
                    aria-label=move_up_label
                    disabled=disabled
                    on:click=move |_| move_source(set_sources, id, -1)
                >"↑"</button>
                <button
                    type="button"
                    title="Move down"
                    aria-label=move_down_label
                    disabled=disabled
                    on:click=move |_| move_source(set_sources, id, 1)
                >"↓"</button>
                <button
                    type="button"
                    title="Duplicate"
                    aria-label=duplicate_label
                    disabled=disabled
                    data-testid="duplicate-merge-source"
                    on:click=move |_| duplicate_source(
                        set_sources,
                        id,
                        next_source_id,
                        set_next_source_id,
                    )
                >"⧉"</button>
                <button
                    type="button"
                    title="Remove"
                    aria-label=remove_label
                    disabled=disabled
                    data-testid="remove-merge-source"
                    on:click=move |_| set_sources.update(|rows| rows.retain(|source| source.id != id))
                >"×"</button>
            </div>
            <Show when=move || error.get().is_some()>
                <p class="source-error" role="alert">{move || error.get().unwrap_or_default()}</p>
            </Show>
        </li>
    }
}

fn run_check(
    label: &'static str,
    ready: impl Fn() -> bool + Send + Sync + 'static,
) -> impl IntoView {
    let ready = Memo::new(move |_| ready());
    view! {
        <div class=move || if ready.get() { "run-check is-ready" } else { "run-check" }>
            <span aria-hidden="true">{move || if ready.get() { "✓" } else { "·" }}</span>
            <span>{label}</span>
        </div>
    }
}

fn task_status(
    task: ReadSignal<TaskState>,
    cancel_merge: Callback<ev::MouseEvent>,
) -> impl IntoView {
    view! {
        <div class="task-status" role="status" aria-live="polite" data-testid="merge-task-status">
            {move || match task.get() {
                TaskState::Idle => view! {
                    <div class="task-idle">
                        <span aria-hidden="true">"◎"</span>
                        <span><strong>"Ready when you are"</strong><small>"Nothing runs until you confirm."</small></span>
                    </div>
                }.into_any(),
                TaskState::Running { cancelling, .. } => view! {
                    <div class="task-running">
                        <span class="progress-spinner" aria-hidden="true"></span>
                        <span>
                            <strong>{if cancelling { "Cancelling safely…" } else { "Verifying and merging…" }}</strong>
                            <small>"The destination remains untouched until verification passes."</small>
                        </span>
                        <button
                            type="button"
                            class="text-action"
                            disabled=cancelling
                            on:click=move |event| cancel_merge.run(event)
                        >
                            {if cancelling { "Cancelling" } else { "Cancel" }}
                        </button>
                    </div>
                }.into_any(),
                TaskState::Completed(report) => view! {
                    <div class="task-complete">
                        <span class="success-check" aria-hidden="true">"✓"</span>
                        <span>
                            <strong>"Merged PDF created"</strong>
                            <small data-testid="merge-result-summary">
                                {format!(
                                    "{} pages · {} sources · {}",
                                    report.page_count,
                                    report.source_count,
                                    report.output_display,
                                )}
                            </small>
                        </span>
                    </div>
                }.into_any(),
                TaskState::Failed(error) => view! {
                    <div class="task-failed" role="alert">
                        <span aria-hidden="true">"!"</span>
                        <span>
                            <strong>"Merge did not complete"</strong>
                            <small>{error.message}</small>
                        </span>
                    </div>
                }.into_any(),
            }}
        </div>
    }
}

fn append_sources(
    picked: Vec<PickedMergeSource>,
    set_sources: WriteSignal<Vec<SourceRow>>,
    next_source_id: ReadSignal<u64>,
    set_next_source_id: WriteSignal<u64>,
) {
    let mut id = next_source_id.get_untracked();
    set_sources.update(|sources| {
        sources.extend(picked.into_iter().map(|picked| {
            let row = SourceRow {
                id,
                picked,
                selection: String::new(),
                password: String::new(),
            };
            id += 1;
            row
        }));
    });
    set_next_source_id.set(id);
}

fn update_source(
    set_sources: WriteSignal<Vec<SourceRow>>,
    id: u64,
    update: impl FnOnce(&mut SourceRow),
) {
    set_sources.update(|sources| {
        if let Some(source) = sources.iter_mut().find(|source| source.id == id) {
            update(source);
        }
    });
}

fn move_source(set_sources: WriteSignal<Vec<SourceRow>>, id: u64, offset: isize) {
    set_sources.update(|sources| {
        let Some(index) = sources.iter().position(|source| source.id == id) else {
            return;
        };
        let target = index.saturating_add_signed(offset);
        if target < sources.len() {
            sources.swap(index, target);
        }
    });
}

fn duplicate_source(
    set_sources: WriteSignal<Vec<SourceRow>>,
    id: u64,
    next_source_id: ReadSignal<u64>,
    set_next_source_id: WriteSignal<u64>,
) {
    let new_id = next_source_id.get_untracked();
    set_next_source_id.set(new_id + 1);
    set_sources.update(|sources| {
        let Some(index) = sources.iter().position(|source| source.id == id) else {
            return;
        };
        let mut duplicate = sources[index].clone();
        duplicate.id = new_id;
        sources.insert(index + 1, duplicate);
    });
}

fn validate_source(source: &SourceRow) -> Option<String> {
    if let Some(issue) = &source.picked.issue {
        return Some(issue.message.clone());
    }
    if source.picked.features.has_forms {
        return Some("Interactive forms are not yet supported by Merge.".to_owned());
    }
    if source.picked.password_required && source.password.is_empty() {
        return Some("Enter the PDF password before running Merge.".to_owned());
    }
    let selection = source.selection.trim();
    if selection.is_empty() {
        return None;
    }
    let parsed = PageSelection::from_str(selection).map_err(|error| error.to_string());
    match (parsed, source.picked.page_count) {
        (Err(error), _) => Some(error),
        (Ok(selection), Some(total)) => selection
            .resolve(total)
            .err()
            .map(|error| error.to_string()),
        (Ok(_), None) => None,
    }
}

fn planned_page_count(sources: &[SourceRow]) -> Option<u32> {
    sources.iter().try_fold(0_u32, |total, source| {
        if validate_source(source).is_some() {
            return None;
        }
        let pages = match (source.selection.trim(), source.picked.page_count) {
            (_, None) => return None,
            ("", Some(page_count)) => page_count,
            (selection, Some(page_count)) => u32::try_from(
                PageSelection::from_str(selection)
                    .ok()?
                    .resolve(page_count)
                    .ok()?
                    .len(),
            )
            .ok()?,
        };
        total.checked_add(pages)
    })
}

fn can_run(
    sources: &[SourceRow],
    destination: Option<&PickedMergeDestination>,
    engine: &MergeEngineStatus,
    task: &TaskState,
) -> bool {
    sources.len() >= 2
        && sources
            .iter()
            .all(|source| validate_source(source).is_none())
        && destination.is_some()
        && engine.ready
        && !matches!(task, TaskState::Running { .. })
}

fn make_request(
    operation_id: String,
    sources: &[SourceRow],
    destination: &PickedMergeDestination,
    replace_existing: bool,
) -> MergeRunRequest {
    MergeRunRequest {
        operation_id,
        sources: sources
            .iter()
            .map(|source| MergeInputRequest {
                path_token: source.picked.path_token.clone(),
                page_selection: (!source.selection.trim().is_empty())
                    .then(|| source.selection.trim().to_owned()),
                password: (!source.password.is_empty()).then(|| source.password.clone()),
            })
            .collect(),
        output_token: destination.path_token.clone(),
        replace_existing,
    }
}

fn browser_sources() -> Vec<PickedMergeSource> {
    vec![
        PickedMergeSource {
            path_token: "browser-quarterly".to_owned(),
            file_name: "quarterly-report.pdf".to_owned(),
            display_path: "Demo files / quarterly-report.pdf".to_owned(),
            page_count: Some(6),
            features: MergeSourceFeatures {
                encrypted: false,
                has_bookmarks: true,
                has_forms: false,
            },
            password_required: false,
            issue: None,
        },
        PickedMergeSource {
            path_token: "browser-appendix".to_owned(),
            file_name: "appendix.pdf".to_owned(),
            display_path: "Demo files / appendix.pdf".to_owned(),
            page_count: Some(3),
            features: MergeSourceFeatures::default(),
            password_required: false,
            issue: None,
        },
    ]
}

fn browser_result(sources: &[SourceRow]) -> MergeRunResult {
    MergeRunResult {
        output_display: "merged-document.pdf".to_owned(),
        source_count: sources.len(),
        page_count: planned_page_count(sources).unwrap_or_default(),
        bookmark_sources_discarded: sources
            .iter()
            .filter(|source| source.picked.features.has_bookmarks)
            .count(),
        engine_id: "deterministic-browser-adapter".to_owned(),
        engine_version: "P4.2".to_owned(),
    }
}

fn complete_browser_merge_after_delay(
    operation_id: String,
    report: MergeRunResult,
    task: ReadSignal<TaskState>,
    set_task: WriteSignal<TaskState>,
) {
    let callback = Closure::once_into_js(move || {
        let still_running = matches!(
            task.get_untracked(),
            TaskState::Running {
                operation_id: current,
                ..
            } if current == operation_id
        );
        if still_running {
            set_task.set(TaskState::Completed(report));
        }
    });
    if let Some(window) = web_sys::window() {
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 650);
    }
}
