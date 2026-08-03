#![forbid(unsafe_code)]
//! Trusted desktop boundary for the P5 Split workspace.

use crate::merge_commands::DesktopState;
use pincerpdf_desktop_api::{
    CommandError, PickedSplitDestination, PickedSplitSource, SplitRuleKind, SplitRunRequest,
    SplitRunResult,
};
use pincerpdf_domain::ErrorCode;
use pincerpdf_engine_api::{InspectOptions, PdfEnginePort};
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::{CancellationToken, ExecutionControl};
use pincerpdf_split::{SplitRule, plan_split};
use std::num::{NonZeroU32, NonZeroU64};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// Opens the trusted native single-file picker and inspects the Split source.
#[tauri::command]
pub async fn pick_split_source(app: AppHandle) -> Result<Option<PickedSplitSource>, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .add_filter("PDF document", &["pdf"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|error| {
            CommandError::new(
                "unsupported_file_path",
                format!("The selected file path is unsupported: {error}"),
            )
        })?;
        let engine = QpdfAdapter::discover().map_err(|error| {
            engine_error(error.code(), format!("PDF engine unavailable: {error}"))
        })?;
        Ok(Some(inspect_split_source(&state, &engine, &path)?))
    })
    .await
    .map_err(|error| internal_error(format!("The Split source picker failed: {error}")))?
}

/// Opens the trusted native directory picker for Split outputs.
#[tauri::command]
pub async fn pick_split_destination(
    app: AppHandle,
) -> Result<Option<PickedSplitDestination>, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| {
                let path = path.into_path().map_err(|error| {
                    CommandError::new(
                        "unsupported_file_path",
                        format!("The selected folder path is unsupported: {error}"),
                    )
                })?;
                let path_token = state.register_path(path.clone())?;
                Ok(PickedSplitDestination {
                    path_token,
                    display_path: path.display().to_string(),
                })
            })
            .transpose()
    })
    .await
    .map_err(|error| internal_error(format!("The Split destination picker failed: {error}")))?
}

/// Runs the verified Split service away from the `WebView` event loop.
#[tauri::command]
pub async fn run_split(
    app: AppHandle,
    request: SplitRunRequest,
) -> Result<SplitRunResult, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    let operation_id = request.operation_id.clone();
    let cancellation = CancellationToken::default();
    state.begin_task(&operation_id, cancellation.clone())?;
    let task_state = state.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        execute_split(&task_state, request, cancellation)
    })
    .await
    .map_err(|error| internal_error(format!("The Split worker failed: {error}")))
    .and_then(|result| result);
    state.finish_task(&operation_id);
    result
}

/// Requests cooperative cancellation for an active Split operation.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri command injection and deserialization require owned handler parameters"
)]
pub fn cancel_split(app: AppHandle, operation_id: String) -> Result<bool, CommandError> {
    app.state::<Arc<DesktopState>>().cancel_task(&operation_id)
}

fn inspect_split_source(
    state: &DesktopState,
    engine: &QpdfAdapter,
    path: &Path,
) -> Result<PickedSplitSource, CommandError> {
    let path_token = state.register_path(path.to_path_buf())?;
    let file_name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let display_path = path.display().to_string();
    match engine.inspect(path, InspectOptions::default()) {
        Ok(metadata) => Ok(PickedSplitSource {
            path_token,
            file_name,
            display_path,
            page_count: Some(metadata.page_count),
            has_bookmarks: metadata.has_bookmarks,
            issue: None,
        }),
        Err(error) => Ok(PickedSplitSource {
            path_token,
            file_name,
            display_path,
            page_count: None,
            has_bookmarks: false,
            issue: Some(engine_error(error.code(), error.to_string())),
        }),
    }
}

fn execute_split(
    state: &DesktopState,
    request: SplitRunRequest,
    cancellation: CancellationToken,
) -> Result<SplitRunResult, CommandError> {
    let source = state.resolve_path(&request.source_token)?;
    let output_directory = state.resolve_path(&request.output_directory_token)?;
    let engine = QpdfAdapter::discover()
        .map_err(|error| engine_error(error.code(), format!("PDF engine unavailable: {error}")))?;
    let metadata = engine
        .inspect(&source, InspectOptions::default())
        .map_err(|error| engine_error(error.code(), error.to_string()))?;
    let control = ExecutionControl::new(Duration::from_mins(10), 64 * 1024, cancellation);
    let rule =
        match request.rule {
            SplitRuleKind::EveryPage => SplitRule::EveryPage,
            SplitRuleKind::FixedPageCount => {
                let count = request.fixed_page_count.ok_or_else(|| {
                    invalid_input("A fixed page count is required for this Split mode.")
                })?;
                SplitRule::FixedPageCount(NonZeroU32::new(count).ok_or_else(|| {
                    invalid_input("The fixed page count must be greater than zero.")
                })?)
            }
            SplitRuleKind::Ranges => request
                .page_ranges
                .ok_or_else(|| invalid_input("At least one page range is required."))?
                .parse::<SplitRule>()
                .map_err(|error| invalid_input(error.to_string()))?,
            SplitRuleKind::Bookmarks => SplitRule::Bookmarks(
                engine
                    .inspect_bookmark_boundaries(&source, &control)
                    .map_err(|error| engine_error(error.code(), error.to_string()))?,
            ),
            SplitRuleKind::BySize => {
                let max_bytes = request
                    .max_output_bytes
                    .and_then(NonZeroU64::new)
                    .ok_or_else(|| invalid_input("The output-size limit must be positive."))?;
                let estimates = engine
                    .estimate_page_sizes(&source, metadata.page_count, &control)
                    .map_err(|error| engine_error(error.code(), error.to_string()))?
                    .estimates;
                SplitRule::BySize {
                    max_bytes,
                    page_estimates: estimates,
                }
            }
        };
    let plan = plan_split(&source, metadata.page_count, &rule)
        .map_err(|error| invalid_input(error.to_string()))?;
    let size_limit_bytes = plan.size_limit_bytes.map(NonZeroU64::get);
    let report = engine
        .split(&plan, &output_directory, &control)
        .map_err(|error| engine_error(error.code(), error.to_string()))?;
    let identity = engine.identity();
    Ok(SplitRunResult {
        outputs: report
            .outputs
            .into_iter()
            .map(|path| path.display().to_string())
            .collect(),
        part_count: plan.parts.len(),
        page_count: metadata.page_count,
        size_limit_bytes,
        engine_id: identity.id,
        engine_version: identity.version,
    })
}

fn invalid_input(message: impl Into<String>) -> CommandError {
    engine_error(ErrorCode::InvalidInput, message)
}

fn engine_error(code: ErrorCode, message: impl Into<String>) -> CommandError {
    CommandError::new(code.as_str(), message)
}

fn internal_error(message: impl Into<String>) -> CommandError {
    engine_error(ErrorCode::Internal, message)
}
