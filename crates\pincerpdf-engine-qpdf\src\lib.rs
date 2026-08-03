#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
//! Process-isolated QPDF adapter for proven `PincerPDF` capabilities.

use pincerpdf_domain::{ErrorCode, PageNumber};
use pincerpdf_engine_api::{
    CapabilitySet, EngineError, EngineIdentity, InspectOptions, PdfCapability, PdfEnginePort,
    PdfMetadata,
};
use pincerpdf_filesystem::{ExistingOutputPolicy, OutputPathPlan, plan_output_path};
use pincerpdf_merge::{
    BookmarkPolicy, CancellationToken, CommandEvidence, ExecutionControl, MergeEngineInput,
    MergeEnginePort, MergeEngineRequest, MergeEngineResult, MergeTocPolicy, SecretString,
};
use pincerpdf_split::{BookmarkBoundary, PageSizeEstimate, SplitPlan};
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
// Keeps combined-output serialization overhead from invalidating a
// single-page measurement used by the engine-independent size planner.
const SPLIT_SIZE_SAFETY_MARGIN_BYTES: u64 = 4096;

/// Configuration for external QPDF execution.
#[derive(Clone, Debug)]
pub struct QpdfConfig {
    /// QPDF executable name or path.
    pub executable: PathBuf,
    /// `MuPDF` text extractor used for collision-aware overlay placement.
    pub text_executable: PathBuf,
    /// Limits used for discovery and inspection commands.
    pub inspection_control: ExecutionControl,
}

impl Default for QpdfConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("qpdf"),
            text_executable: PathBuf::from("mutool"),
            inspection_control: ExecutionControl::new(
                Duration::from_secs(30),
                64 * 1024,
                CancellationToken::default(),
            ),
        }
    }
}

/// QPDF-backed inspection and Merge adapter.
#[derive(Clone, Debug)]
pub struct QpdfAdapter {
    config: QpdfConfig,
    identity: EngineIdentity,
}

/// Verified outputs and redacted QPDF evidence for one split materialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitMaterializationReport {
    /// Atomically finalized output paths in plan order.
    pub outputs: Vec<PathBuf>,
    /// External-command evidence for each extraction and count verification.
    pub evidence: Vec<CommandEvidence>,
}

/// Conservative single-page size estimates used by size-based split planning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitSizeEstimateReport {
    /// One estimate per source page, in one-based order.
    pub estimates: Vec<PageSizeEstimate>,
    /// Redacted QPDF evidence for each single-page materialization.
    pub evidence: Vec<CommandEvidence>,
}

struct SplitOutputGuard {
    plans: Vec<OutputPathPlan>,
    finalized: Vec<PathBuf>,
    committed: bool,
}

impl SplitOutputGuard {
    fn new(plans: Vec<OutputPathPlan>) -> Self {
        Self {
            plans,
            finalized: Vec::new(),
            committed: false,
        }
    }
}

impl Drop for SplitOutputGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for plan in &self.plans {
            let _ = fs::remove_file(&plan.temporary_path);
        }
        for output in &self.finalized {
            let _ = fs::remove_file(output);
        }
    }
}

impl QpdfAdapter {
    /// Discovers the default `qpdf` executable and records its exact version.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when QPDF cannot be started or queried.
    pub fn discover() -> Result<Self, EngineError> {
        Self::from_config(QpdfConfig::default())
    }

    /// Creates an adapter from an explicit executable and inspection policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the executable cannot report a version.
    pub fn from_config(config: QpdfConfig) -> Result<Self, EngineError> {
        let capture = run_process(
            &config.executable,
            &[OsString::from("--version")],
            vec!["--version".to_owned()],
            &config.inspection_control,
        )
        .map_err(|failure| map_process_failure(&failure, false))?;
        let version = capture
            .evidence
            .stdout
            .lines()
            .next()
            .unwrap_or("qpdf version unknown")
            .trim()
            .to_owned();
        Ok(Self {
            config,
            identity: EngineIdentity {
                id: "qpdf-process".to_owned(),
                version,
            },
        })
    }

    fn run_qpdf(
        &self,
        args: &[OsString],
        display_args: Vec<String>,
        control: &ExecutionControl,
        password_supplied: bool,
    ) -> Result<ProcessCapture, EngineError> {
        run_process(&self.config.executable, args, display_args, control)
            .map_err(|failure| map_process_failure(&failure, password_supplied))
    }

    /// Materializes an engine-independent split plan into atomically finalized
    /// one-output-per-part PDFs.
    ///
    /// Existing destination files are rejected. Every QPDF invocation writes
    /// to a hidden sibling first, verifies its page count, then renames it into
    /// place. If any part fails, already-created outputs are removed so a
    /// partial split is never reported as successful.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the output directory, source alias, QPDF
    /// process, page count or finalization policy is invalid.
    #[allow(clippy::too_many_lines)]
    pub fn split(
        &self,
        plan: &SplitPlan,
        output_directory: &Path,
        control: &ExecutionControl,
    ) -> Result<SplitMaterializationReport, EngineError> {
        if !output_directory.is_dir() {
            return Err(EngineError::new(
                ErrorCode::OutputWriteFailed,
                format!(
                    "split output directory is not a directory: {}",
                    output_directory.display()
                ),
            ));
        }
        if plan.parts.is_empty() {
            return Err(EngineError::new(
                ErrorCode::InvalidInput,
                "split plan contains no output parts",
            ));
        }
        let canonical_source = plan.source.canonicalize().map_err(|error| {
            EngineError::new(
                ErrorCode::InputUnreadable,
                format!(
                    "cannot resolve split source {}: {error}",
                    plan.source.display()
                ),
            )
        })?;
        let mut planned = Vec::with_capacity(plan.parts.len());
        for part in &plan.parts {
            let final_path = output_directory.join(format!("{}.pdf", part.filename_stem));
            let final_exists = final_path.try_exists().map_err(|error| {
                EngineError::new(ErrorCode::OutputWriteFailed, error.to_string())
            })?;
            let final_canonical = final_path.canonicalize().ok();
            if final_canonical.as_deref() == Some(canonical_source.as_path()) {
                return Err(EngineError::new(
                    ErrorCode::OutputWriteFailed,
                    "split output would overwrite its source PDF",
                ));
            }
            let token = format!("split_{}", part.ordinal);
            let output_plan = plan_output_path(
                &final_path,
                final_exists,
                ExistingOutputPolicy::Fail,
                &token,
            )
            .map_err(|error| EngineError::new(ErrorCode::OutputConflict, error.to_string()))?;
            planned.push(output_plan);
        }

        let mut guard = SplitOutputGuard::new(planned.clone());
        let mut evidence = Vec::new();
        let outline_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=outlines"),
                qpdf_path(&plan.source),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=outlines".to_owned(),
                plan.source.display().to_string(),
            ],
            &ExecutionControl::new(
                control.timeout(),
                control.output_limit_bytes().max(32 * 1024 * 1024),
                control.cancellation().clone(),
            ),
            false,
        )?;
        ensure_complete_json(&outline_capture.evidence, "split source bookmark tree")?;
        let outline_json = outline_capture.evidence.stdout.clone();
        evidence.push(outline_capture.evidence);

        let document_title = plan.source.file_name().map_or_else(
            || "document.pdf".to_owned(),
            |name| name.to_string_lossy().into_owned(),
        );
        for (part, output_plan) in plan.parts.iter().zip(&planned) {
            let page_spec = page_specification(&part.pages);
            let raw_output = TemporaryPath::new("split-raw", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private split staging directory: {error}"),
                )
            })?;
            let capture = self.run_qpdf(
                &[
                    OsString::from("--empty"),
                    OsString::from("--pages"),
                    qpdf_path(&plan.source),
                    OsString::from(&page_spec),
                    OsString::from("--"),
                    qpdf_path(raw_output.path()),
                ],
                vec![
                    "--empty".to_owned(),
                    "--pages".to_owned(),
                    plan.source.display().to_string(),
                    page_spec,
                    "--".to_owned(),
                    raw_output.path().display().to_string(),
                ],
                control,
                false,
            )?;
            evidence.push(capture.evidence);

            let split_input = MergeEngineInput {
                source: plan.source.clone(),
                document_title: document_title.clone(),
                metadata_title: None,
                pages: part.pages.clone(),
                password: None,
            };
            let bookmark_plan = BookmarkPlan {
                // Bookmark page positions are one-based throughout the QPDF
                // update and verification boundary, including one-page parts.
                roots: parse_source_bookmarks_rejecting_ambiguous_duplicates(
                    &outline_json,
                    &split_input,
                    1,
                )?,
            };
            if bookmark_plan.roots.is_empty() {
                stage_split_output(raw_output.path(), &output_plan.temporary_path)?;
            } else {
                let bookmarked_output =
                    TemporaryPath::new("split-bookmarked", "pdf").map_err(|error| {
                        EngineError::new(
                            ErrorCode::Internal,
                            format!("cannot create private bookmark staging directory: {error}"),
                        )
                    })?;
                self.add_bookmark_plan(
                    raw_output.path(),
                    bookmarked_output.path(),
                    &bookmark_plan,
                    std::slice::from_ref(&split_input),
                    control,
                    &mut evidence,
                )?;
                stage_split_output(bookmarked_output.path(), &output_plan.temporary_path)?;
            }
            let page_capture = self.run_qpdf(
                &[
                    OsString::from("--show-npages"),
                    qpdf_path(&output_plan.temporary_path),
                ],
                vec![
                    "--show-npages".to_owned(),
                    output_plan.temporary_path.display().to_string(),
                ],
                control,
                false,
            )?;
            let actual = parse_qpdf_page_count(&page_capture.evidence.stdout, "split")?;
            evidence.push(page_capture.evidence);
            let expected = u32::try_from(part.pages.len()).map_err(|_| {
                EngineError::new(ErrorCode::InvalidInput, "split part page count overflowed")
            })?;
            if actual != expected {
                return Err(EngineError::new(
                    ErrorCode::EngineFailure,
                    format!(
                        "split output page conservation failed: expected {expected}, got {actual}"
                    ),
                ));
            }
            if let Some(limit) = plan.size_limit_bytes {
                let actual_bytes = fs::metadata(&output_plan.temporary_path)
                    .map_err(|error| {
                        EngineError::new(
                            ErrorCode::OutputWriteFailed,
                            format!("cannot inspect split output size: {error}"),
                        )
                    })?
                    .len();
                if actual_bytes > limit.get() {
                    return Err(EngineError::new(
                        ErrorCode::OutputWriteFailed,
                        format!(
                            "split output exceeded byte limit: expected at most {}, got {actual_bytes}",
                            limit.get()
                        ),
                    ));
                }
            }
            fs::rename(&output_plan.temporary_path, &output_plan.final_path).map_err(|error| {
                EngineError::new(
                    ErrorCode::OutputWriteFailed,
                    format!("cannot finalize split output: {error}"),
                )
            })?;
            guard.finalized.push(output_plan.final_path.clone());
        }
        let outputs = guard.finalized.clone();
        guard.committed = true;
        Ok(SplitMaterializationReport { outputs, evidence })
    }

    /// Extracts ordered top-level bookmark page boundaries for bookmark-based
    /// split planning. This remains the compatibility default; callers that
    /// deliberately opt into a nested outline level should use
    /// [`Self::inspect_bookmark_boundaries_at_depth`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when QPDF cannot produce complete outline JSON
    /// or a selected outline lacks a usable page destination/title.
    pub fn inspect_bookmark_boundaries(
        &self,
        source: &Path,
        control: &ExecutionControl,
    ) -> Result<Vec<BookmarkBoundary>, EngineError> {
        let capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=outlines"),
                qpdf_path(source),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=outlines".to_owned(),
                source.display().to_string(),
            ],
            control,
            false,
        )?;
        ensure_complete_json(&capture.evidence, "split bookmark boundaries")?;
        parse_bookmark_boundaries(&capture.evidence.stdout)
    }

    /// Extracts ordered bookmark boundaries at one explicit zero-based outline
    /// depth. A depth of `0` is equivalent to
    /// [`Self::inspect_bookmark_boundaries`]. Nodes at other depths are still
    /// traversed so a nested policy cannot accidentally ignore a valid child.
    /// Destinations are validated only for the selected depth; a parent or
    /// intermediate node without a destination may still contain usable
    /// descendants.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when QPDF cannot produce complete outline JSON
    /// or a selected outline lacks a usable page destination/title.
    pub fn inspect_bookmark_boundaries_at_depth(
        &self,
        source: &Path,
        depth: u32,
        control: &ExecutionControl,
    ) -> Result<Vec<BookmarkBoundary>, EngineError> {
        let capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=outlines"),
                qpdf_path(source),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=outlines".to_owned(),
                source.display().to_string(),
            ],
            control,
            false,
        )?;
        ensure_complete_json(&capture.evidence, "split bookmark boundaries")?;
        parse_bookmark_boundaries_at_depth(&capture.evidence.stdout, depth)
    }

    /// Materializes each source page once to measure a conservative size
    /// estimate for size-based split planning. The estimates deliberately use
    /// the same QPDF page assembly path as final split outputs rather than
    /// treating source object byte spans as interchangeable with serialized
    /// output bytes.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a page cannot be materialized or its
    /// temporary output has no measurable bytes.
    pub fn estimate_page_sizes(
        &self,
        source: &Path,
        total_pages: u32,
        control: &ExecutionControl,
    ) -> Result<SplitSizeEstimateReport, EngineError> {
        if total_pages == 0 {
            return Err(EngineError::new(
                ErrorCode::InvalidInput,
                "cannot estimate sizes for a zero-page source",
            ));
        }
        let mut estimates = Vec::with_capacity(usize::try_from(total_pages).unwrap_or(0));
        let mut evidence = Vec::with_capacity(usize::try_from(total_pages).unwrap_or(0));
        for page in 1..=total_pages {
            let temporary = TemporaryPath::new("split-size-estimate", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::OutputWriteFailed,
                    format!("cannot create private split-size estimate: {error}"),
                )
            })?;
            let page_number = PageNumber::new(page).map_err(|_| {
                EngineError::new(ErrorCode::InvalidInput, "split page number overflowed")
            })?;
            let page_spec = page_specification(&[page_number]);
            let capture = self.run_qpdf(
                &[
                    OsString::from("--empty"),
                    OsString::from("--pages"),
                    qpdf_path(source),
                    OsString::from(&page_spec),
                    OsString::from("--"),
                    qpdf_path(temporary.path()),
                ],
                vec![
                    "--empty".to_owned(),
                    "--pages".to_owned(),
                    source.display().to_string(),
                    page_spec,
                    "--".to_owned(),
                    "<private-split-size-estimate>".to_owned(),
                ],
                control,
                false,
            )?;
            evidence.push(capture.evidence);
            let bytes = fs::metadata(temporary.path())
                .map_err(|error| {
                    EngineError::new(
                        …22323 tokens truncated…arguments: Vec<String>, duration: Duration) -> CommandEvidence {
    CommandEvidence {
        program: program.display().to_string(),
        arguments,
        exit_code: None,
        duration_ms: duration.as_millis(),
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn evidence(
    program: &Path,
    arguments: Vec<String>,
    status: ExitStatus,
    duration: Duration,
    stdout: BoundedText,
    stderr: BoundedText,
) -> CommandEvidence {
    CommandEvidence {
        program: program.display().to_string(),
        arguments,
        exit_code: status.code(),
        duration_ms: duration.as_millis(),
        stdout: stdout.text,
        stderr: stderr.text,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    }
}

fn map_process_failure(failure: &ProcessFailure, password_supplied: bool) -> EngineError {
    let combined =
        format!("{} {}", failure.evidence.stdout, failure.evidence.stderr).to_ascii_lowercase();
    let code = match failure.kind {
        ProcessFailureKind::Cancelled => ErrorCode::Cancelled,
        ProcessFailureKind::Spawn | ProcessFailureKind::TimedOut | ProcessFailureKind::Join => {
            ErrorCode::EngineFailure
        }
        ProcessFailureKind::Exit
            if combined.contains("invalid password")
                || combined.contains("incorrect password")
                || combined.contains("password is incorrect") =>
        {
            ErrorCode::IncorrectPassword
        }
        ProcessFailureKind::Exit if combined.contains("password") && !password_supplied => {
            ErrorCode::PasswordRequired
        }
        ProcessFailureKind::Exit => ErrorCode::EngineFailure,
    };
    let command = format!(
        "{} {}",
        failure.evidence.program,
        failure.evidence.arguments.join(" ")
    );
    EngineError::new(code, format!("{}; command: {command}", failure.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use pincerpdf_merge::CancellationToken;

    #[cfg(unix)]
    #[test]
    fn bounded_capture_drains_but_retains_only_the_configured_limit() {
        let control =
            ExecutionControl::new(Duration::from_secs(2), 5, CancellationToken::default());
        let capture = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("printf 1234567890")],
            vec!["-c".to_owned(), "printf <test-data>".to_owned()],
            &control,
        )
        .expect("process succeeds");
        assert_eq!(capture.evidence.stdout, "12345");
        assert!(capture.evidence.stdout_truncated);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_the_child_process() {
        let control = ExecutionControl::new(
            Duration::from_millis(40),
            1024,
            CancellationToken::default(),
        );
        let failure = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("sleep 1")],
            vec!["-c".to_owned(), "sleep 1".to_owned()],
            &control,
        )
        .expect_err("process must time out");
        assert!(matches!(failure.kind, ProcessFailureKind::TimedOut));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_kills_the_child_process() {
        let token = CancellationToken::default();
        let cancellation = token.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            cancellation.cancel();
        });
        let control = ExecutionControl::new(Duration::from_secs(2), 1024, token);
        let failure = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("sleep 1")],
            vec!["-c".to_owned(), "sleep 1".to_owned()],
            &control,
        )
        .expect_err("process must be cancelled");
        handle.join().expect("cancellation thread joins");
        assert!(matches!(failure.kind, ProcessFailureKind::Cancelled));
    }

    #[test]
    fn page_specification_preserves_order_and_duplicates() {
        let pages = [
            PageNumber::new(3).expect("valid"),
            PageNumber::new(1).expect("valid"),
            PageNumber::new(3).expect("valid"),
        ];
        assert_eq!(page_specification(&pages), "3,1,3");
    }

    #[test]
    fn page_geometry_parser_accepts_boxes_rotation_and_parent_reference() {
        let page = "<< /Type /Page /Parent 12 0 R /MediaBox [ -10 -20 612 792 ] /CropBox [ 0 0 600 700 ] /Rotate 270 >>";
        assert_eq!(
            parse_pdf_number_array(page, "/MediaBox").expect("media box"),
            Some([
                "-10".to_owned(),
                "-20".to_owned(),
                "612".to_owned(),
                "792".to_owned(),
            ])
        );
        assert_eq!(
            parse_pdf_number_array(page, "/CropBox").expect("crop box"),
            Some([
                "0".to_owned(),
                "0".to_owned(),
                "600".to_owned(),
                "700".to_owned(),
            ])
        );
        assert_eq!(
            parse_pdf_integer(page, "/Rotate").expect("rotation"),
            Some(270)
        );
        assert_eq!(
            parse_pdf_reference(page, "/Parent").expect("parent"),
            Some((12, 0))
        );
    }

    #[test]
    fn footer_position_prefers_the_visible_crop_box() {
        let geometry = PdfPageGeometry {
            media_box: Some([
                "-10".to_owned(),
                "-20".to_owned(),
                "612".to_owned(),
                "792".to_owned(),
            ]),
            crop_box: Some([
                "20".to_owned(),
                "30".to_owned(),
                "580".to_owned(),
                "760".to_owned(),
            ]),
            rotate: None,
        };
        assert_eq!(footer_position(&geometry), (44.0, 48.0));
    }

    #[test]
    fn footer_position_moves_to_top_when_bottom_band_is_occupied() {
        let geometry = PdfPageGeometry {
            media_box: Some([
                "0".to_owned(),
                "0".to_owned(),
                "600".to_owned(),
                "800".to_owned(),
            ]),
            crop_box: None,
            rotate: None,
        };
        let bounds = TextBounds {
            top: 60.0,
            bottom: 790.0,
        };
        assert_eq!(
            footer_position_with_text(&geometry, Some(bounds)),
            (24.0, 776.0)
        );
    }

    #[test]
    fn structured_text_bounds_are_bounded_to_block_boxes() {
        let text = r#"<page id="page1">
<block bbox="72 52.65 168.01 77.38">
<line bbox="72 702 200 720">
</line>
</block>
</page>"#;
        assert_eq!(
            parse_stext_bounds(text),
            Some(TextBounds {
                top: 52.65,
                bottom: 720.0,
            })
        );
    }

    #[test]
    fn qdf_document_title_decodes_literal_and_utf16_hex_values() {
        assert_eq!(
            qdf_document_title(b"4 0 obj\n<< /Title (Quarterly \\(draft\\)) >>\nendobj"),
            Some("Quarterly (draft)".to_owned())
        );
        assert_eq!(
            qdf_document_title(b"4 0 obj\n<< /Title <FEFF00500069006E0063> >>\nendobj"),
            Some("Pinc".to_owned())
        );
    }

    #[test]
    fn qdf_document_title_ignores_empty_and_malformed_values() {
        assert_eq!(qdf_document_title(b"<< /Title () >>"), None);
        assert_eq!(qdf_document_title(b"<< /Title (unterminated >>"), None);
        assert_eq!(qdf_document_title(b"<< /Title <GG> >>"), None);
    }

    #[test]
    fn document_title_contents_falls_back_to_filename() {
        let input = MergeEngineInput {
            source: PathBuf::from("report.pdf"),
            document_title: "report.pdf".to_owned(),
            metadata_title: Some("  ".to_owned()),
            pages: vec![PageNumber::new(1).expect("valid")],
            password: None,
        };
        assert_eq!(toc_title(&input, MergeTocPolicy::DocumentTitles), "report");
        assert_eq!(toc_title(&input, MergeTocPolicy::FileNames), "report");
    }

    #[test]
    fn source_bookmark_plan_prunes_excluded_leaves_and_maps_selected_pages() {
        let input = MergeEngineInput {
            source: PathBuf::from("bookmarks.pdf"),
            document_title: "bookmarks.pdf".to_owned(),
            metadata_title: None,
            pages: vec![
                PageNumber::new(2).expect("valid"),
                PageNumber::new(3).expect("valid"),
            ],
            password: None,
        };
        let source = r#"{
          "outlines": [
            {"title":"Chapter 1","destpageposfrom1":1,"kids":[]},
            {"title":"Chapter 2","destpageposfrom1":2,"kids":[
              {"title":"Appendix","destpageposfrom1":3,"kids":[]}
            ]}
          ]
        }"#;

        assert_eq!(
            parse_source_bookmarks(source, &input, 4).expect("source plan"),
            vec![BookmarkPlanNode {
                title: "Chapter 2".to_owned(),
                page_position: Some(4),
                children: vec![BookmarkPlanNode {
                    title: "Appendix".to_owned(),
                    page_position: Some(5),
                    children: Vec::new(),
                }],
            }]
        );
        assert_eq!(document_bookmark_title("bookmarks.pdf"), "bookmarks");
    }

    #[test]
    fn source_bookmark_parser_rejects_named_destination_leaves() {
        let input = MergeEngineInput {
            source: PathBuf::from("named-destination.pdf"),
            document_title: "named-destination.pdf".to_owned(),
            metadata_title: None,
            pages: vec![PageNumber::new(1).expect("valid")],
            password: None,
        };
        let source = r#"{
          "outlines": [{
            "title":"Named chapter",
            "dest":"u:chapter-one",
            "kids":[]
          }]
        }"#;

        let error = parse_source_bookmarks(source, &input, 1)
            .expect_err("named destinations must not be silently discarded");
        assert_eq!(error.code(), ErrorCode::CapabilityUnavailable);
        assert!(error.to_string().contains("named destination"));
    }

    #[test]
    fn source_bookmark_parser_rejects_action_or_unresolved_destinations() {
        let input = MergeEngineInput {
            source: PathBuf::from("action-bookmark.pdf"),
            document_title: "action-bookmark.pdf".to_owned(),
            metadata_title: None,
            pages: vec![PageNumber::new(1).expect("valid")],
            password: None,
        };
        let source = r#"{
          "outlines": [{
            "title":"Open web resource",
            "dest":null,
            "action":{"type":"/URI"},
            "kids":[]
          }]
        }"#;

        let error = parse_source_bookmarks(source, &input, 1)
            .expect_err("actions must not be silently discarded");
        assert_eq!(error.code(), ErrorCode::CapabilityUnavailable);
        assert!(error.to_string().contains("action"));
    }

    #[test]
    fn source_bookmark_parser_keeps_unresolved_container_with_surviving_child() {
        let input = MergeEngineInput {
            source: PathBuf::from("container-bookmark.pdf"),
            document_title: "container-bookmark.pdf".to_owned(),
            metadata_title: None,
            pages: vec![PageNumber::new(1).expect("valid")],
            password: None,
        };
        let source = r#"{
          "outlines": [{
            "title":"Container",
            "dest":null,
            "kids":[{
              "title":"Page one",
              "dest":["3 0 R","/Fit"],
              "destpageposfrom1":1,
              "kids":[]
            }]
          }]
        }"#;

        let plan = parse_source_bookmarks(source, &input, 1).expect("container is valid");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].title, "Container");
        assert_eq!(plan[0].page_position, None);
        assert_eq!(plan[0].children[0].page_position, Some(1));
    }

    #[test]
    fn split_bookmark_parser_rejects_ambiguous_duplicate_page_destinations() {
        let input = MergeEngineInput {
            source: PathBuf::from("duplicate-pages.pdf"),
            document_title: "duplicate-pages.pdf".to_owned(),
            metadata_title: None,
            pages: vec![
                PageNumber::new(1).expect("valid"),
                PageNumber::new(1).expect("valid"),
            ],
            password: None,
        };
        let source = r#"{
          "outlines": [{
            "title":"Page one",
            "dest":["3 0 R","/Fit"],
            "destpageposfrom1":1,
            "kids":[]
          }]
        }"#;

        let error = parse_source_bookmarks_rejecting_ambiguous_duplicates(source, &input, 1)
            .expect_err("duplicate page destination must be explicit");
        assert_eq!(error.code(), ErrorCode::CapabilityUnavailable);
        assert!(
            error
                .to_string()
                .contains("destination identity is ambiguous")
        );
    }

    #[test]
    fn merge_bookmark_parser_retains_first_duplicate_page_occurrence() {
        let input = MergeEngineInput {
            source: PathBuf::from("duplicate-pages.pdf"),
            document_title: "duplicate-pages.pdf".to_owned(),
            metadata_title: None,
            pages: vec![
                PageNumber::new(1).expect("valid"),
                PageNumber::new(1).expect("valid"),
            ],
            password: None,
        };
        let source = r#"{
          "outlines": [{
            "title":"Page one",
            "dest":["3 0 R","/Fit"],
            "destpageposfrom1":1,
            "kids":[]
          }]
        }"#;

        let plan = parse_source_bookmarks(source, &input, 1).expect("merge policy is stable");
        assert_eq!(plan[0].page_position, Some(1));
    }

    #[test]
    fn bookmark_boundary_parser_keeps_only_ordered_top_level_page_targets() {
        let json = r#"{
          "outlines": [
            {"title":"Intro","destpageposfrom1":1,"kids":[]},
            {"title":"Chapter 2","destpageposfrom1":3,"kids":[
              {"title":"Appendix","destpageposfrom1":4,"kids":[]}
            ]}
          ]
        }"#;
        assert_eq!(
            parse_bookmark_boundaries(json).expect("valid boundaries"),
            vec![
                BookmarkBoundary {
                    title: "Intro".to_owned(),
                    page: PageNumber::new(1).expect("page"),
                    depth: 0,
                },
                BookmarkBoundary {
                    title: "Chapter 2".to_owned(),
                    page: PageNumber::new(3).expect("page"),
                    depth: 0,
                },
            ]
        );
    }

    #[test]
    fn bookmark_boundary_parser_selects_nested_depth_and_preserves_order() {
        let json = r#"{
          "outlines": [
            {"title":"Chapter 1","destpageposfrom1":1,"kids":[
              {"title":"Section 1.1","destpageposfrom1":1,"kids":[]},
              {"title":"Section 1.2","destpageposfrom1":2,"kids":[]}
            ]},
            {"title":"Chapter 2","destpageposfrom1":3,"kids":[
              {"title":"Section 2.1","destpageposfrom1":3,"kids":[]}
            ]}
          ]
        }"#;
        assert_eq!(
            parse_bookmark_boundaries_at_depth(json, 1).expect("nested boundaries"),
            vec![
                BookmarkBoundary {
                    title: "Section 1.1".to_owned(),
                    page: PageNumber::new(1).expect("page"),
                    depth: 1,
                },
                BookmarkBoundary {
                    title: "Section 1.2".to_owned(),
                    page: PageNumber::new(2).expect("page"),
                    depth: 1,
                },
                BookmarkBoundary {
                    title: "Section 2.1".to_owned(),
                    page: PageNumber::new(3).expect("page"),
                    depth: 1,
                },
            ]
        );
    }

    #[test]
    fn bookmark_boundary_parser_allows_unresolved_intermediate_destinations() {
        let json = r#"{
          "outlines": [{"title":"Chapter","kids":[
            {"title":"Section","destpageposfrom1":2,"kids":[]}
          ]}]
        }"#;
        let boundaries = parse_bookmark_boundaries_at_depth(json, 1).expect("child boundary");
        assert_eq!(boundaries[0].title, "Section");
        assert_eq!(boundaries[0].page.get(), 2);
    }

    #[test]
    fn bookmark_boundary_parser_rejects_missing_selected_destination() {
        let json = r#"{
          "outlines": [{"title":"Chapter","destpageposfrom1":1,"kids":[
            {"title":"Section","kids":[]}
          ]}]
        }"#;
        let error = parse_bookmark_boundaries_at_depth(json, 1).expect_err("missing target");
        assert!(error.to_string().contains("depth 1"));
        assert!(error.to_string().contains("valid page destination"));
    }

    #[test]
    fn bookmark_update_preserves_catalog_and_does_not_replace_the_trailer() {
        let layout = OutlineLayout {
            metadata: json!({
                "jsonversion": 2,
                "pdfversion": "1.7",
                "maxobjectid": 7
            }),
            max_object_id: 7,
            catalog_reference: "1 0 R".to_owned(),
            catalog_object: 1,
            catalog_generation: 0,
            page_objects: vec!["3 0 R".to_owned(), "5 0 R".to_owned(), "7 0 R".to_owned()],
        };
        let catalog = json!({
            "/Pages": "2 0 R",
            "/Type": "/Catalog",
            "/Lang": "u:en"
        })
        .as_object()
        .expect("catalog object")
        .clone();
        let expected = vec![
            BookmarkPlanNode {
                title: "one".to_owned(),
                page_position: Some(1),
                children: vec![BookmarkPlanNode {
                    title: "child".to_owned(),
                    page_position: Some(3),
                    children: Vec::new(),
                }],
            },
            BookmarkPlanNode {
                title: "two".to_owned(),
                page_position: Some(2),
                children: Vec::new(),
            },
        ];

        let update = build_bookmark_update(&layout, catalog, &expected).expect("bookmark update");
        let objects = update["qpdf"][1].as_object().expect("update objects");

        assert!(!objects.contains_key("trailer"));
        assert_eq!(
            objects["obj:1 0 R"]["value"]["/Pages"],
            Value::String("2 0 R".to_owned())
        );
        assert_eq!(
            objects["obj:1 0 R"]["value"]["/Lang"],
            Value::String("u:en".to_owned())
        );
        assert_eq!(objects["obj:8 0 R"]["value"]["/Count"], Value::from(2));
        assert_eq!(
            objects["obj:9 0 R"]["value"]["/Dest"][0],
            Value::String("3 0 R".to_owned())
        );
        assert_eq!(
            objects["obj:10 0 R"]["value"]["/Dest"][0],
            Value::String("7 0 R".to_owned())
        );
        assert_eq!(
            objects["obj:11 0 R"]["value"]["/Dest"][0],
            Value::String("5 0 R".to_owned())
        );
    }
}

