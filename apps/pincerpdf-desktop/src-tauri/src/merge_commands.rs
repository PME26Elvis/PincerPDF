≠rá^—f•ñÿ¶{^Ïy 'v√Æ∂õ≠#![forbid(unsafe_code)]
//! Trusted desktop boundary for the P4.2 Merge workspace.

use pincerpdf_desktop_api::{
    CommandError, MergeBookmarkPolicy, MergeEngineStatus, MergeInputRequest, MergeRunRequest,
    MergeRunResult, MergeSourceFeatures, PickedMergeDestination, PickedMergeSource,
};
use pincerpdf_domain::{ErrorCode, PageSelection};
use pincerpdf_engine_api::{InspectOptions, PdfEnginePort};
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_filesystem::ExistingOutputPolicy;
use pincerpdf_merge::{
    BookmarkPolicy, CancellationToken, ExecutionControl, MergeError, MergeExecutionOptions,
    MergeRequest, MergeRequestError, MergeService, MergeSource, SecretString,
};
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

/// In-memory authority for opaque paths and active native tasks.
#[derive(Default)]
pub struct DesktopState {
    next_path_token: AtomicU64,
    paths: Mutex<BTreeMap<String, PathBuf>>,
    tasks: Mutex<BTreeMap<String, CancellationToken>>,
}

impl DesktopState {
    fn register_path(&self, path: PathBuf) -> Result<String, CommandError> {
        let token = format!(
            "path-{}",
            self.next_path_token.fetch_add(1, Ordering::Relaxed) + 1
        );
        self.paths()?.insert(token.clone(), path);
        Ok(token)
    }

    fn resolve_path(&self, token: &str) -> Result<PathBuf, CommandError> {
        self.paths()?
            .get(token)
            .cloned()
            .ok_or_else(|| CommandError::new("invalid_path_token", "The selected file expired."))
    }

    fn begin_task(
        &self,
        operation_id: &str,
        cancellation: CancellationToken,
    ) -> Result<(), CommandError> {
        if operation_id.is_empty()
            || operation_id.len() > 64
            || !operation_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(CommandError::new(
                "invalid_operation_id",
                "The Merge operation identifier is invalid.",
            ));
        }
        let mut tasks = self.tasks()?;
        match tasks.entry(operation_id.to_owned()) {
            Entry::Vacant(entry) => {
                entry.insert(cancellation);
                Ok(())
            }
            Entry::Occupied(_) => Err(CommandError::new(
                "operation_conflict",
                "A Merge operation with this identifier is already running.",
            )),
        }
    }

    fn finish_task(&self, operation_id: &str) {
        if let Ok(mut tasks) = self.tasks() {
            tasks.remove(operation_id);
        }
    }

    fn cancel_task(&self, operation_id: &str) -> Result<bool, CommandError> {
        let tasks = self.tasks()?;
        let Some(token) = tasks.get(operation_id) else {
            return Ok(false);
        };
        token.cancel();
        Ok(true)
    }

    fn paths(&self) -> Result<MutexGuard<'_, BTreeMap<String, PathBuf>>, CommandError> {
        self.paths
            .lock()
            .map_err(|_| internal_error("The selected-file registry is unavailable."))
    }

    fn tasks(&self) -> Result<MutexGuard<'_, BTreeMap<String, CancellationToken>>, CommandError> {
        self.tasks
            .lock()
            .map_err(|_| internal_error("The Merge task registry is unavailable."))
    }
}

/// Reports whether the native QPDF composition is available.
#[tauri::command]
pub fn merge_engine_status() -> MergeEngineStatus {
    match QpdfAdapter::discover() {
        Ok(engine) => {
            let identity = engine.identity();
            MergeEngineStatus {
                ready: true,
                engine_id: Some(identity.id),
                engine_version: Some(identity.version),
                issue: None,
            }
        }
        Err(error) => MergeEngineStatus {
            ready: false,
            engine_id: None,
            engine_version: None,
            issue: Some(engine_error(error.code(), error.to_string())),
        },
    }
}

/// Opens the trusted native multi-file picker and inspects selected PDFs.
#[tauri::command]
pub async fn pick_merge_sources(
    app: tauri::AppHandle,
) -> Result<Vec<PickedMergeSource>, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = app
            .dialog()
            .file()
            .add_filter("PDF documents", &["pdf"])
            .blocking_pick_files()
            .unwrap_or_default();
        let engine = QpdfAdapter::discover().map_err(|error| {
            engine_error(error.code(), format!("PDF engine unavailable: {error}"))
        })?;
        picked
            .into_iter()
            .map(|path| {
                let path = path.into_path().map_err(|error| {
                    CommandError::new(
                        "unsupported_file_path",
                        format!("The selected file path is unsupported: {error}"),
                    )
                })?;
                inspect_picked_source(&state, &engine, &path)
            })
            .collect()
    })
    .await
    .map_err(|error| internal_error(format!("The file picker task failed: {error}")))?
}

/// Opens the trusted native save dialog and registers an opaque destination token.
#[tauri::command]
pub async fn pick_merge_destination(
    app: tauri::AppHandle,
) -> Result<Option<PickedMergeDestination>, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("PDF document", &["pdf"])
            .set_file_name("merged-document.pdf")
            .blocking_save_file()
            .map(|path| {
                let path = path.into_path().map_err(|error| {
                    CommandError::new(
                        "unsupported_file_path",
                        format!("The selected output path is unsupported: {error}"),
                    )
                })?;
                let path_token = state.register_path(path.clone())?;
                Ok(PickedMergeDestination {
                    path_token,
                    display_path: path.display().to_string(),
                })
            })
            .transpose()
    })
    .await
    .map_err(|error| internal_error(format!("The save-dialog task failed: {error}")))?
}

/// Runs the verified Merge service away from the `WebView` event loop.
#[tauri::command]
pub async fn run_merge(
    app: tauri::AppHandle,
    request: MergeRunRequest,
) -> Result<MergeRunResult, CommandError> {
    let state = app.state::<Arc<DesktopState>>().inner().clone();
    let operation_id = request.operation_id.clone();
    let cancellation = CancellationToken::default();
    state.begin_task(&operation_id, cancellation.clone())?;
    let task_state = state.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        execute_merge(&task_state, request, cancellation)
    })
    .await
    .map_err(|error| internal_error(format!("The Merge worker failed: {error}")))
    .and_then(|result| result);
    state.finish_task(&operation_id);
    result
}

/// Requests cooperative cancellation for an active Merge operation.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri command injection and deserialization require owned handler parameters"
)]
pub fn cancel_merge(app: tauri::AppHandle, operation_id: String) -> Result<bool, CommandError> {
    app.state::<Arc<DesktopState>>().cancel_task(&operation_id)
}

fn inspect_picked_source(
    state: &DesktopState,
    engine: &QpdfAdapter,
    path: &Path,
) -> Result<PickedMergeSource, CommandError> {
    let path_token = state.register_path(path.to_path_buf())?;
    let file_name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let display_path = path.display().to_string();
    match engine.inspect(path, InspectOptions::default()) {
        Ok(metadata) => Ok(PickedMergeSource {
            path_token,
            file_name,
            display_path,
            page_count: Some(metadata.page_count),
            features: MergeSourceFeatures {
                encrypted: metadata.encrypted,
                has_bookmarks: metadata.has_bookmarks,
                has_forms: metadata.has_forms,
            },
            password_required: false,
            issue: metadata.has_forms.then(|| {
                CommandError::new(
                    "forms_unsupported",
                    "Interactive forms are not yet accepted by the verified Merge policy.",
                )
            }),
        }),
        Err(error)
            if matches!(
                error.code(),
                ErrorCode::PasswordRequired | ErrorCode::IncorrectPassword
            ) =>
        {
            Ok(PickedMergeSource {
                path_token,
                file_name,
                display_path,
                page_count: None,
                features: MergeSourceFeatures {
                    encrypted: true,
                    ..MergeSourceFeatures::default()
                },
                password_required: true,
                issue: None,
            })
        }
        Err(error) => Ok(PickedMergeSource {
            path_token,
            file_name,
            display_path,
            page_count: None,
            features: MergeSourceFeatures::default(),
            password_required: false,
            issue: Some(engine_error(error.code(), error.to_string())),
        }),
    }
}

fn execute_merge(
    state: &DesktopState,
    request: MergeRunRequest,
    cancellation: CancellationToken,
) -> Result<MergeRunResult, CommandError> {
    let output_policy = if request.replace_existing {
        ExistingOutputPolicy::Replace
    } else {
        ExistingOutputPolicy::Fail
    };
    let bookmark_policy = match request.bookmark_policy {
        MergeBookmarkPolicy::Discard => BookmarkPolicy::Discard,
        MergeBookmarkPolicy::OneEntryPerDocument => BookmarkPolicy::OneEntryPerDocument,
        MergeBookmarkPolicy::Retain => BookmarkPolicy::Retain,
        MergeBookmarkPolicy::RetainAsOneEntryPerDocument => {
            BookmarkPolicy::RetainAsOneEntryPerDocument
        }
    };
    let output = state.resolve_path(&request.output_token)?;
    let merge_request = build_merge_request(state, request.sources, output)?;
    let engine =
        QpdfAdapter::discover().map_err(|error| engine_error(error.code(), error.to_string()))?;
    let options = MergeExecutionOptions {
        output_policy,
        bookmark_policy,
        add_blank_page_if_odd: request.add_blank_page_if_odd,
        add_filename_footer: request.add_filename_footer,
        control: ExecutionControl::new(Duration::from_mins(10), 64 * 1024, cancellation),
    };
    let report = MergeService::new(&engine)
        .execute(&merge_request, &options)
        .map_err(|error| command_error_from_merge(&error))?;
    Ok(MergeRunResult {
        output_display: report.output.display().to_string(),
        source_count: report.source_count,
        page_count: report.page_count,
        bookmark_sources_discarded: report.bookmark_sources_discarded,
        bookmark_entries: report.bookmark_entries,
        engine_id: report.engine.id,
        engine_version: report.engine.version,
    })
}

fn build_merge_request(
    state: &DesktopState,
    inputs: Vec<MergeInputRequest>,
    output: PathBuf,
) -> Result<MergeRequest, CommandError> {
    let sources = inputs
        .into_iter()
        .enumerate()
        .map(|(source_index, input)| {
            let mut source = MergeSource::new(
                state
                    .resolve_path(&input.path_token)
                    .map_err(|error| error.with_source_index(source_index))?,
            );
            if let Some(selection) = input
                .page_selection
                .filter(|selection| !selection.trim().is_empty())
            {
                source = source.with_selection(PageSelection::from_str(&selection).map_err(
                    |error| {
                        CommandError::new("invalid_page_selection", error.to_string())
                            .with_source_index(source_index)
                    },
                )?);
            }
            if let Some(password) = input.password.filter(|password| !password.is_empty()) {
                source = source.with_password(SecretString::new(password).map_err(|error| {
                    CommandError::new("invalid_password", error.to_string())
                        .with_source_index(source_index)
                })?);
            }
            Ok(source)
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    MergeRequest::new(sources, output).map_err(command_error_from_request)
}

fn command_error_from_request(error: MergeRequestError) -> CommandError {
    let source_index = match error {
        MergeRequestError::EmptySourcePath { source_index }
        | MergeRequestError::OutputEqualsSource { source_index } => Some(source_index),
        MergeRequestError::NotEnoughSources | MergeRequestError::OutputMustBePdf => None,
    };
    CommandError {
        code: ErrorCode::InvalidInput.as_str().to_owned(),
        message: error.to_string(),
        source_index,
    }
}

fn command_error_from_merge(error: &MergeError) -> CommandError {
    let source_index = match error {
        MergeError::InspectSource { source_index, .. }
        | MergeError::OutputAliasesSource { source_index }
        | MergeError::EmptySourceDocument { source_index }
        | MergeError::FormsUnsupported { source_index }
        | MergeError::InvalidSelection { source_index, .. } => Some(*source_index),
        _ => None,
    };
    CommandError {
        code: error.code().as_str().to_owned(),
        message: error.to_string(),
        source_index,
    }
}

fn engine_error(code: ErrorCode, message: impl Into<String>) -> CommandError {
    CommandError::new(code.as_str(), message)
}

fn internal_error(message: impl Into<String>) -> CommandError {
    engine_error(ErrorCode::Internal, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn opaque_tokens_build_an_ordered_request_without_exposing_paths_to_the_ui() {
        let state = DesktopState::default();
        let first = state
            .register_path(PathBuf::from("first.pdf"))
            .expect("register first");
        let second = state
            .register_path(PathBuf::from("second.pdf"))
            .expect("register second");
        let request = build_merge_request(
            &state,
            vec![
                MergeInputRequest {
                    path_token: first,
                    page_selection: Some("3,1-2".to_owned()),
                    password: None,
                },
                MergeInputRequest {
                    path_token: second,
                    page_selection: None,
                    password: None,
                },
            ],
            PathBuf::from("output.pdf"),
        )
        .expect("valid request");
        assert_eq!(request.sources().len(), 2);
        assert_eq!(request.sources()[0].path(), Path::new("first.pdf"));
        assert!(request.sources()[0].selection().is_some());
    }

    #[test]
    fn unknown_tokens_fail_before_engine_access() {
        let error = build_merge_request(
            &DesktopState::default(),
            vec![MergeInputRequest {
                path_token: "unknown".to_owned(),
                page_selection: None,
                password: None,
            }],
            PathBuf::from("output.pdf"),
        )
        .expect_err("unknown token");
        assert_eq!(error.code, "invalid_path_token");
        assert_eq!(error.source_index, Some(0));
    }

    #[test]
    fn duplicate_operation_ids_are_rejected_and_can_be_cancelled() {
        let state = DesktopState::default();
        let first = CancellationToken::default();
        state
            .begin_task("merge-1", first.clone())
            .expect("first task");
        assert!(
            state
                .begin_task("merge-1", CancellationToken::default())
                .is_err()
        );
        assert!(state.cancel_task("merge-1").expect("cancel"));
        assert!(first.is_cancelled());
        state.finish_task("merge-1");
        assert!(!state.cancel_task("merge-1").expect("missing task"));
    }

    #[test]
    #[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
    fn native_command_boundary_merges_only_registered_paths() {
        let fixtures = PathBuf::from(
            std::env::var("PINCERPDF_PDF_FIXTURES")
                .expect("PINCERPDF_PDF_FIXTURES must point at generated fixtures"),
        );
        let evidence = PathBuf::from(
            std::env::var("PINCERPDF_MERGE_DESKTOP_EVIDENCE_DIR")
                .expect("PINCERPDF_MERGE_DESKTOP_EVIDENCE_DIR must be set"),
        );
        fs::create_dir_all(&evidence).expect("create desktop evidence directory");
        let output = evidence.join("desktop-command-output.pdf");
        if output.try_exists().expect("inspect prior output") {
            fs::remove_file(&output).expect("remove prior desktop contract output");
        }

        let state = DesktopState::default();
        let first = state
            .register_path(fixtures.join("plain-three-pages.pdf"))
            .expect("register first input");
        let second = state
            .register_path(fixtures.join("plain-three-pages.pdf"))
            .expect("register repeated input");
        let destination = state
            .register_path(output.clone())
            .expect("register destination");
        let report = execute_merge(
            &state,
            MergeRunRequest {
                operation_id: "native-contract".to_owned(),
                sources: vec![
                    MergeInputRequest {
                        path_token: first,
                        page_selection: Some("3,1".to_owned()),
                        password: None,
                    },
                    MergeInputRequest {
                        path_token: second,
                        page_selection: Some("2".to_owned()),
                        password: None,
                    },
                ],
                output_token: destination,
                replace_existing: false,
                bookmark_policy: MergeBookmarkPolicy::OneEntryPerDocument,
                add_blank_page_if_odd: false,
                add_filename_footer: false,
            },
            CancellationToken::default(),
        )
        .expect("desktop boundary merge");

        assert_eq!(report.source_count, 2);
        assert_eq!(report.page_count, 3);
        assert_eq!(report.bookmark_entries, 2);
        assert_eq!(report.output_display, output.display().to_string());
        assert_eq!(report.engine_id, "qpdf-process");
        assert!(output.is_file());
    }
}
