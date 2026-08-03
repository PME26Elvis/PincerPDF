#![forbid(unsafe_code)]
//! Accessible P5 Split workspace with a deterministic browser adapter.

use crate::native_bridge::{call_with_args, call_without_args, is_tauri};
use leptos::ev;
use leptos::prelude::*;
use pincerpdf_desktop_api::{
    CommandError, PickedSplitDestination, PickedSplitSource, SplitRuleKind, SplitRunRequest,
    SplitRunResult,
};
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

#[derive(Clone)]
enum TaskState {
    Idle,
    Running {
        operation_id: String,
        cancelling: bool,
    },
    Completed(SplitRunResult),
    Failed(CommandError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunSplitArgs {
    request: SplitRunRequest,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelSplitArgs {
    operation_id: String,
}

/// Renders the first usable Split workflow while keeping all rule capabilities explicit.
#[component]
#[allow(
    clippy::too_many_lines,
    reason = "the Split view keeps source, rule, output and verification state together"
)]
pub(crate) fn SplitWorkspace(
    engine_status: ReadSignal<pincerpdf_desktop_api::MergeEngineStatus>,
) -> impl IntoView {
    let (source, set_source) = signal(Option::<PickedSplitSource>::None);
    let (destination, set_destination) = signal(Option::<PickedSplitDestination>::None);
    let (rule, set_rule) = signal(SplitRuleKind::EveryPage);
    let (fixed_count, set_fixed_count) = signal("2".to_owned());
    let (ranges, set_ranges) = signal("1-2;3-6".to_owned());
    let (bookmark_depth, set_bookmark_depth) = signal("0".to_owned());
    let (max_bytes, set_max_bytes) = signal("100000".to_owned());
    let (task, set_task) = signal(TaskState::Idle);
    let (picker_busy, set_picker_busy) = signal(false);
    let (next_operation_id, set_next_operation_id) = signal(1_u64);
    let native = is_tauri();

    let choose_source = move |_| {
        if picker_busy.get_untracked() || matches!(task.get_untracked(), TaskState::Running { .. })
        {
            return;
        }
        set_picker_busy.set(true);
        if native {
            leptos::task::spawn_local(async move {
                match call_without_args::<Option<PickedSplitSource>>("pick_split_source").await {
                    Ok(Some(picked)) => {
                        set_source.set(Some(picked));
                        set_task.set(TaskState::Idle);
                    }
                    Ok(None) => {}
                    Err(error) => set_task.set(TaskState::Failed(error)),
                }
                set_picker_busy.set(false);
            });
        } else {
            set_source.set(Some(browser_source()));
            set_task.set(TaskState::Idle);
            set_picker_busy.set(false);
        }
    };

    let choose_destination = move |_| {
        if matches!(task.get_untracked(), TaskState::Running { .. }) {
            return;
        }
        if native {
            leptos::task::spawn_local(async move {
                match call_without_args::<Option<PickedSplitDestination>>("pick_split_destination")
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
            set_destination.set(Some(PickedSplitDestination {
                path_token: "browser-split-output".to_owned(),
                display_path: "Demo outputs / split".to_owned(),
            }));
            set_task.set(TaskState::Idle);
        }
    };

    let run_split = move |_| {
        let source_value = source.get_untracked();
        let destination_value = destination.get_untracked();
        let current_rule = rule.get_untracked();
        if !can_run(
            source_value.as_ref(),
            destination_value.as_ref(),
            current_rule,
            &fixed_count.get_untracked(),
            &ranges.get_untracked(),
            &bookmark_depth.get_untracked(),
            &max_bytes.get_untracked(),
            &engine_status.get_untracked(),
            &task.get_untracked(),
        ) {
            set_task.set(TaskState::Failed(CommandError::new(
                "invalid_input",
                validation_message(current_rule),
            )));
            return;
        }
        let number = next_operation_id.get_untracked();
        set_next_operation_id.set(number + 1);
        let operation_id = format!("split-ui-{number}");
        let request = SplitRunRequest {
            operation_id: operation_id.clone(),
            source_token: source_value
                .as_ref()
                .expect("can_run requires a source")
                .path_token
                .clone(),
            output_directory_token: destination_value
                .as_ref()
                .expect("can_run requires a destination")
                .path_token
                .clone(),
            rule: current_rule,
            fixed_page_count: (current_rule == SplitRuleKind::FixedPageCount).then(|| {
                fixed_count
                    .get_untracked()
                    .trim()
                    .parse()
                    .unwrap_or_default()
            }),
            page_ranges: (current_rule == SplitRuleKind::Ranges)
                .then(|| ranges.get_untracked().trim().to_owned()),
            bookmark_depth: (current_rule == SplitRuleKind::Bookmarks).then(|| {
                bookmark_depth
                    .get_untracked()
                    .trim()
                    .parse()
                    .unwrap_or_default()
            }),
            max_output_bytes: (current_rule == SplitRuleKind::BySize)
                .then(|| max_bytes.get_untracked().trim().parse().unwrap_or_default()),
        };
        set_task.set(TaskState::Running {
            operation_id: operation_id.clone(),
            cancelling: false,
        });
        if native {
            leptos::task::spawn_local(async move {
                let result =
                    call_with_args::<SplitRunResult, _>("run_split", &RunSplitArgs { request })
                        .await;
                match result {
                    Ok(report) => set_task.set(TaskState::Completed(report)),
                    Err(error) => set_task.set(TaskState::Failed(error)),
                }
            });
        } else {
            complete_browser_split_after_delay(
                operation_id,
                browser_result(
                    source_value.as_ref().and_then(|picked| picked.page_count),
                    current_rule,
                ),
                task,
                set_task,
            );
        }
    };

    let cancel_split = move |_| {
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
                let result =
                    call_with_args::<bool, _>("cancel_split", &CancelSplitArgs { operation_id })
                        .await;
                if let Err(error) = result {
                    set_task.set(TaskState::Failed(error));
                }
            });
        } else {
            set_task.set(TaskState::Failed(CommandError::new(
                "cancelled",
                "The deterministic Split task was cancelled.",
            )));
        }
    };

    view! {
        <section class="merge-workspace split-workspace" aria-labelledby="split-title" data-testid="split-workspace">
            <div class="merge-heading">
                <div>
                    <p class="eyebrow">"P5 · Split workspace"</p>
                    <h1 id="split-title">"Partition a PDF with a rule you can audit."</h1>
                    <p>"Choose one source, select an explicit page policy, and finalize every output only after verification."</p>
                </div>
                <div class="merge-engine-card" data-testid="split-engine-status">
                    <span class=move || if engine_status.get().ready { "status-dot is-ready" } else { "status-dot" }></span>
                    <span>
                        <strong>{move || if engine_status.get().ready { "Split engine ready" } else { "Split engine unavailable" }}</strong>
                        <small>{move || engine_status.get().engine_version.unwrap_or_else(|| "Check the local QPDF installation".to_owned())}</small>
                    </span>
                </div>
            </div>

            <div class="merge-layout">
                <div class="merge-main">
                    <section class="merge-card source-card" aria-labelledby="split-source-title">
                        <div class="merge-card-heading">
                            <div><span class="step-number">"1"</span><span><h2 id="split-source-title">"Source document"</h2><p>"The source is inspected before any output is created."</p></span></div>
                            <button type="button" class="secondary-action" data-testid="choose-split-source" disabled=move || picker_busy.get() on:click=choose_source>
                                {move || if picker_busy.get() { "Opening…" } else { "Choose PDF" }}
                            </button>
                        </div>
                        <Show when=move || source.get().is_some() fallback=move || view! {
                            <div class="merge-empty" data-testid="split-empty-state"><span class="empty-icon" aria-hidden="true">"PDF"</span><div><strong>"No source document yet"</strong><p>"Browser verification loads a deterministic six-page sample."</p></div><button type="button" class="text-action" on:click=choose_source>"Choose file"</button></div>
                        }>
                            <div class="split-source-summary" data-testid="split-source-summary">
                                <strong>{move || source.get().map_or_else(String::new, |picked| picked.file_name)}</strong>
                                <small>{move || source.get().map_or_else(String::new, |picked| format!("{} · {}", picked.display_path, picked.page_count.map_or_else(|| "page count unavailable".to_owned(), |pages| format!("{pages} pages"))))}</small>
                                <Show when=move || source.get().is_some_and(|picked| picked.has_bookmarks)><span class="source-badges"><span>"Bookmarks detected"</span></span></Show>
                            </div>
                        </Show>
                    </section>

                    <section class="merge-card" aria-labelledby="split-rule-title">
                        <div class="merge-card-heading"><div><span class="step-number">"2"</span><span><h2 id="split-rule-title">"Split rule"</h2><p>"The selected policy is serialized into the trusted native request."</p></span></div></div>
                        <fieldset class="bookmark-policy split-rule-options" data-testid="split-rule-options">
                            <legend>"Partition policy"</legend>
                            <label><input type="radio" name="split-rule" prop:checked=move || rule.get() == SplitRuleKind::EveryPage on:change=move |_| set_rule.set(SplitRuleKind::EveryPage) data-testid="split-rule-every-page"/><span><strong>"Every page"</strong><small>"One verified PDF per source page."</small></span></label>
                            <label><input type="radio" name="split-rule" prop:checked=move || rule.get() == SplitRuleKind::FixedPageCount on:change=move |_| set_rule.set(SplitRuleKind::FixedPageCount) data-testid="split-rule-fixed"/><span><strong>"Fixed page count"</strong><small>"Consecutive groups with a maximum page count."</small></span></label>
                            <Show when=move || rule.get() == SplitRuleKind::FixedPageCount><label class="range-field"><span>"Pages per output"</span><input type="number" min="1" step="1" prop:value=move || fixed_count.get() on:input=move |event| set_fixed_count.set(event_target_value(&event)) data-testid="split-fixed-count"/></label></Show>
                            <label><input type="radio" name="split-rule" prop:checked=move || rule.get() == SplitRuleKind::Ranges on:change=move |_| set_rule.set(SplitRuleKind::Ranges) data-testid="split-rule-ranges"/><span><strong>"Explicit ranges"</strong><small>"Keep each semicolon-separated range as its own output."</small></span></label>
                            <Show when=move || rule.get() == SplitRuleKind::Ranges><label class="range-field"><span>"Ranges"</span><input type="text" placeholder="1-2;3-6" prop:value=move || ranges.get() on:input=move |event| set_ranges.set(event_target_value(&event)) data-testid="split-page-ranges"/></label></Show>
                            <label><input type="radio" name="split-rule" prop:checked=move || rule.get() == SplitRuleKind::Bookmarks on:change=move |_| set_rule.set(SplitRuleKind::Bookmarks) data-testid="split-rule-bookmarks"/><span><strong>"Bookmarks"</strong><small>"Start outputs at validated outline destinations."</small></span></label>
                            <Show when=move || rule.get() == SplitRuleKind::Bookmarks><label class="range-field"><span>"Bookmark depth"</span><input type="number" min="0" step="1" prop:value=move || bookmark_depth.get() on:input=move |event| set_bookmark_depth.set(event_target_value(&event)) data-testid="split-bookmark-depth"/><small>"0 is top-level; deeper levels opt into nested sections."</small></label></Show>
                            <label><input type="radio" name="split-rule" prop:checked=move || rule.get() == SplitRuleKind::BySize on:change=move |_| set_rule.set(SplitRuleKind::BySize) data-testid="split-rule-size"/><span><strong>"Estimated output size"</strong><small>"Use conservative per-page serialization estimates and verify before rename."</small></span></label>
                            <Show when=move || rule.get() == SplitRuleKind::BySize><label class="range-field"><span>"Maximum bytes"</span><input type="number" min="1" step="1024" prop:value=move || max_bytes.get() on:input=move |event| set_max_bytes.set(event_target_value(&event)) data-testid="split-max-bytes"/></label></Show>
                        </fieldset>
                    </section>

                    <section class="merge-card output-card" aria-labelledby="split-output-title">
                        <div class="merge-card-heading"><div><span class="step-number">"3"</span><span><h2 id="split-output-title">"Output directory"</h2><p>"Each part is written through a hidden sibling and atomically finalized."</p></span></div></div>
                        <div class="destination-control"><div><span class="field-label">"Destination folder"</span><strong data-testid="split-output-path">{move || destination.get().map_or_else(|| "No destination selected".to_owned(), |picked| picked.display_path)}</strong></div><button type="button" class="secondary-action" data-testid="choose-split-output" on:click=choose_destination>"Choose folder"</button></div>
                    </section>
                </div>
                <aside class="merge-run-card" aria-labelledby="split-run-title">
                    <span class="step-number">"4"</span><h2 id="split-run-title">"Create split outputs"</h2><p>"The action unlocks only when the source, rule and destination are valid."</p>
                    <div class="run-checks">{run_check("Source inspected", move || source.get().is_some_and(|picked| picked.page_count.is_some() && picked.issue.is_none()))}{run_check("Rule valid", move || rule_valid(rule.get(), &fixed_count.get(), &ranges.get(), &bookmark_depth.get(), &max_bytes.get(), source.get().as_ref()))}{run_check("Output selected", move || destination.get().is_some())}{run_check("Native engine ready", move || engine_status.get().ready)}</div>
                    <button type="button" class="primary-action run-action" data-testid="run-split" disabled=move || !can_run(source.get().as_ref(), destination.get().as_ref(), rule.get(), &fixed_count.get(), &ranges.get(), &bookmark_depth.get(), &max_bytes.get(), &engine_status.get(), &task.get()) on:click=run_split>{move || if matches!(task.get(), TaskState::Running { .. }) { "Splitting…" } else { "Create split outputs" }}</button>
                    {task_status(task, Callback::new(cancel_split))}
                </aside>
            </div>
        </section>
    }
}

fn run_check(
    label: &'static str,
    ready: impl Fn() -> bool + Send + Sync + 'static,
) -> impl IntoView {
    let ready = Memo::new(move |_| ready());
    view! { <div class=move || if ready.get() { "run-check is-ready" } else { "run-check" }><span aria-hidden="true">{move || if ready.get() { "✓" } else { "·" }}</span><span>{label}</span></div> }
}

fn task_status(task: ReadSignal<TaskState>, cancel: Callback<ev::MouseEvent>) -> impl IntoView {
    view! { <div class="task-status" role="status" aria-live="polite" data-testid="split-task-status">{move || match task.get() {
        TaskState::Idle => view! { <div class="task-idle"><span aria-hidden="true">"◎"</span><span><strong>"Ready when you are"</strong><small>"Nothing runs until you confirm."</small></span></div> }.into_any(),
        TaskState::Running { cancelling, .. } => view! { <div class="task-running"><span class="progress-spinner" aria-hidden="true"></span><span><strong>{if cancelling { "Cancelling safely…" } else { "Verifying and splitting…" }}</strong><small>"The destination remains untouched until verification passes."</small></span><button type="button" class="text-action" disabled=cancelling on:click=move |event| cancel.run(event)>{if cancelling { "Cancelling" } else { "Cancel" }}</button></div> }.into_any(),
        TaskState::Completed(report) => view! { <div class="task-complete"><span class="success-check" aria-hidden="true">"✓"</span><span><strong>"Split outputs created"</strong><small data-testid="split-result-summary">{format!("{} pages · {} parts · {}{}", report.page_count, report.part_count, report.engine_id, report.size_limit_bytes.map_or_else(String::new, |limit| format!(" · max {limit} bytes")))}</small></span></div> }.into_any(),
        TaskState::Failed(error) => view! { <div class="task-failed" role="alert"><span aria-hidden="true">"!"</span><span><strong>"Split did not complete"</strong><small>{error.message}</small></span></div> }.into_any(),
    }}</div> }
}

fn rule_valid(
    rule: SplitRuleKind,
    fixed: &str,
    ranges: &str,
    bookmark_depth: &str,
    max_bytes: &str,
    source: Option<&PickedSplitSource>,
) -> bool {
    match rule {
        SplitRuleKind::EveryPage => true,
        SplitRuleKind::FixedPageCount => fixed.trim().parse::<u32>().is_ok_and(|value| value > 0),
        SplitRuleKind::Ranges => !ranges.trim().is_empty(),
        SplitRuleKind::Bookmarks => {
            source.is_some_and(|picked| picked.has_bookmarks)
                && bookmark_depth.trim().parse::<u32>().is_ok()
        }
        SplitRuleKind::BySize => max_bytes.trim().parse::<u64>().is_ok_and(|value| value > 0),
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "the run gate mirrors every visible Split prerequisite"
)]
fn can_run(
    source: Option<&PickedSplitSource>,
    destination: Option<&PickedSplitDestination>,
    rule: SplitRuleKind,
    fixed: &str,
    ranges: &str,
    bookmark_depth: &str,
    max_bytes: &str,
    engine: &pincerpdf_desktop_api::MergeEngineStatus,
    task: &TaskState,
) -> bool {
    source.is_some_and(|picked| picked.page_count.is_some() && picked.issue.is_none())
        && destination.is_some()
        && rule_valid(rule, fixed, ranges, bookmark_depth, max_bytes, source)
        && engine.ready
        && !matches!(task, TaskState::Running { .. })
}

fn validation_message(rule: SplitRuleKind) -> &'static str {
    match rule {
        SplitRuleKind::EveryPage => "Choose a valid source and output folder.",
        SplitRuleKind::FixedPageCount => "Enter a positive page count.",
        SplitRuleKind::Ranges => "Enter at least one page range.",
        SplitRuleKind::Bookmarks => {
            "The source must contain usable bookmarks at the selected depth."
        }
        SplitRuleKind::BySize => "Enter a positive output-size limit.",
    }
}

fn browser_source() -> PickedSplitSource {
    PickedSplitSource {
        path_token: "browser-split-source".to_owned(),
        file_name: "quarterly-report.pdf".to_owned(),
        display_path: "Demo files / quarterly-report.pdf".to_owned(),
        page_count: Some(6),
        has_bookmarks: true,
        issue: None,
    }
}

fn browser_result(page_count: Option<u32>, rule: SplitRuleKind) -> SplitRunResult {
    let pages = page_count.unwrap_or_default();
    let parts = match rule {
        SplitRuleKind::EveryPage => pages,
        SplitRuleKind::FixedPageCount => pages.div_ceil(2),
        SplitRuleKind::Ranges | SplitRuleKind::Bookmarks | SplitRuleKind::BySize => 2,
    } as usize;
    SplitRunResult {
        outputs: (1..=parts)
            .map(|part| format!("split-{part:02}.pdf"))
            .collect(),
        part_count: parts,
        page_count: pages,
        size_limit_bytes: (rule == SplitRuleKind::BySize).then_some(100_000),
        engine_id: "deterministic-browser-adapter".to_owned(),
        engine_version: "P5.1".to_owned(),
    }
}

fn complete_browser_split_after_delay(
    operation_id: String,
    report: SplitRunResult,
    task: ReadSignal<TaskState>,
    set_task: WriteSignal<TaskState>,
) {
    let callback = Closure::once_into_js(move || {
        if matches!(task.get_untracked(), TaskState::Running { operation_id: current, .. } if current == operation_id)
        {
            set_task.set(TaskState::Completed(report));
        }
    });
    if let Some(window) = web_sys::window() {
        let _ = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), 650);
    }
}

