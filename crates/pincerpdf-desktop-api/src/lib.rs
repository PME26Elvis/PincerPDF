#![forbid(unsafe_code)]
//! Stable, serializable messages shared by the desktop host and Leptos UI.

use serde::{Deserialize, Serialize};

/// Availability of the native Merge engine composition.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeEngineStatus {
    /// Whether the proven QPDF composition can accept work.
    pub ready: bool,
    /// Stable adapter identifier when discovery succeeds.
    pub engine_id: Option<String>,
    /// Human-readable adapter version when discovery succeeds.
    pub engine_version: Option<String>,
    /// Safe diagnostic when discovery fails.
    pub issue: Option<CommandError>,
}

/// One source returned by the trusted native file picker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedMergeSource {
    /// Opaque session token resolved only by the native host.
    pub path_token: String,
    /// User-facing file name.
    pub file_name: String,
    /// User-facing path that must never be treated as authority.
    pub display_path: String,
    /// Page count when inspection succeeded without a password.
    pub page_count: Option<u32>,
    /// Inspected document features that affect Merge policy.
    pub features: MergeSourceFeatures,
    /// Whether a password is required before inspection can complete.
    pub password_required: bool,
    /// Safe per-row problem when the file cannot be accepted immediately.
    pub issue: Option<CommandError>,
}

/// Document features surfaced by the native source inspection.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeSourceFeatures {
    /// Whether the source declares encryption.
    pub encrypted: bool,
    /// Whether the source contains a bookmark outline.
    pub has_bookmarks: bool,
    /// Whether the source contains an interactive form.
    pub has_forms: bool,
}

/// Destination returned by the trusted native save dialog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedMergeDestination {
    /// Opaque session token resolved only by the native host.
    pub path_token: String,
    /// User-facing path that must never be treated as authority.
    pub display_path: String,
}

/// One source returned by the trusted native Split file picker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedSplitSource {
    /// Opaque session token resolved only by the native host.
    pub path_token: String,
    /// User-facing file name.
    pub file_name: String,
    /// User-facing path that must never be treated as authority.
    pub display_path: String,
    /// Page count when inspection succeeded without a password.
    pub page_count: Option<u32>,
    /// Whether the source has a usable bookmark outline at any depth.
    pub has_bookmarks: bool,
    /// Safe per-source problem when the file cannot be accepted immediately.
    pub issue: Option<CommandError>,
}

/// Destination directory returned by the trusted native Split picker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickedSplitDestination {
    /// Opaque session token resolved only by the native host.
    pub path_token: String,
    /// User-facing directory path.
    pub display_path: String,
}

/// Split rule selected by the UI or internal desktop adapter.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitRuleKind {
    /// Emit one output for every source page.
    #[default]
    EveryPage,
    /// Emit consecutive outputs containing at most a fixed number of pages.
    FixedPageCount,
    /// Split using explicit semicolon-separated page ranges.
    Ranges,
    /// Split using validated bookmark boundaries at the requested outline depth.
    Bookmarks,
    /// Split using conservative serialized-size estimates.
    BySize,
}

/// Complete Split intent crossing the IPC boundary.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitRunRequest {
    /// Client-generated operation identifier used for cancellation.
    pub operation_id: String,
    /// Opaque source token issued by the current native session.
    pub source_token: String,
    /// Opaque output-directory token issued by the current native session.
    pub output_directory_token: String,
    /// Selected high-level split rule.
    pub rule: SplitRuleKind,
    /// Required for `fixedPageCount`.
    pub fixed_page_count: Option<u32>,
    /// Required for `ranges`, using the planner's semicolon-separated syntax.
    pub page_ranges: Option<String>,
    /// Optional zero-based outline depth for `bookmarks`; `None` means the
    /// top-level outline (depth `0`).
    pub bookmark_depth: Option<u32>,
    /// Optional positive byte limit for `bySize`.
    pub max_output_bytes: Option<u64>,
}

/// Verified result returned after all Split outputs are atomically finalized.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitRunResult {
    /// User-facing finalized output paths in plan order.
    pub outputs: Vec<String>,
    /// Number of finalized output parts.
    pub part_count: usize,
    /// Number of source pages inspected for the operation.
    pub page_count: u32,
    /// Configured size limit when the by-size rule was used.
    pub size_limit_bytes: Option<u64>,
    /// Concrete adapter identifier.
    pub engine_id: String,
    /// Human-readable adapter version.
    pub engine_version: String,
}

/// One ordered Merge input crossing the IPC boundary.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeInputRequest {
    /// Opaque path token issued by the current native session.
    pub path_token: String,
    /// Optional ordered page-selection expression; blank means all pages.
    pub page_selection: Option<String>,
    /// Optional PDF password. This type intentionally does not implement `Debug`.
    pub password: Option<String>,
}

/// Bookmark policy selected in the Merge workspace.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeBookmarkPolicy {
    /// Emit no bookmark tree.
    #[default]
    Discard,
    /// Emit one top-level entry for each ordered source row.
    OneEntryPerDocument,
    /// Retain the relevant source outline hierarchy at the output root.
    Retain,
    /// Retain each relevant source outline hierarchy below one source entry.
    RetainAsOneEntryPerDocument,
}

/// Generated table-of-contents policy selected in the Merge workspace.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeTocPolicy {
    /// Do not prepend a generated contents page.
    #[default]
    None,
    /// List source filenames and their first output pages.
    FileNames,
    /// List source metadata titles and their first output pages.
    DocumentTitles,
}

/// Complete Merge intent crossing the IPC boundary.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRunRequest {
    /// Client-generated operation identifier used for cancellation.
    pub operation_id: String,
    /// Ordered inputs, including deliberate duplicate entries.
    pub sources: Vec<MergeInputRequest>,
    /// Opaque destination token issued by the current native session.
    pub output_token: String,
    /// Whether a pre-existing destination may be atomically replaced.
    pub replace_existing: bool,
    /// Explicit output bookmark behavior.
    pub bookmark_policy: MergeBookmarkPolicy,
    /// Whether to add a blank page after each odd-page source.
    pub add_blank_page_if_odd: bool,
    /// Whether each output page receives its source filename as a footer.
    pub add_filename_footer: bool,
    /// Generated table-of-contents policy.
    pub toc_policy: MergeTocPolicy,
}

/// Verified result returned after atomic finalization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRunResult {
    /// User-facing output path.
    pub output_display: String,
    /// Number of ordered input entries.
    pub source_count: usize,
    /// Verified final page count.
    pub page_count: u32,
    /// Number of source outline trees intentionally discarded by P4.1 policy.
    pub bookmark_sources_discarded: usize,
    /// Number of verified top-level bookmarks generated by policy.
    pub bookmark_entries: usize,
    /// Concrete adapter identifier.
    pub engine_id: String,
    /// Concrete adapter version.
    pub engine_version: String,
}

/// Stable, serializable command failure safe for UI display.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    /// Machine-readable error category.
    pub code: String,
    /// Secret-safe user-facing description.
    pub message: String,
    /// Optional zero-based source position.
    pub source_index: Option<usize>,
}

impl CommandError {
    /// Creates a safe command error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            source_index: None,
        }
    }

    /// Associates the error with an ordered source row.
    #[must_use]
    pub const fn with_source_index(mut self, source_index: usize) -> Self {
        self.source_index = Some(source_index);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_types_deliberately_omit_debug_for_password_safety() {
        fn assert_serializable<T: Serialize>() {}
        assert_serializable::<MergeInputRequest>();
        assert_serializable::<MergeRunRequest>();
        assert_serializable::<SplitRunRequest>();
        assert_serializable::<SplitRunResult>();
    }

    #[test]
    fn command_error_tracks_a_source_without_changing_its_code() {
        let error =
            CommandError::new("password_required", "Password required").with_source_index(2);
        assert_eq!(error.code, "password_required");
        assert_eq!(error.source_index, Some(2));
    }
}
