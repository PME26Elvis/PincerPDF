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
                roots: parse_source_bookmarks(&outline_json, &split_input, 0)?,
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
                        ErrorCode::EngineFailure,
                        format!("cannot inspect split-size estimate for page {page}: {error}"),
                    )
                })?
                .len();
            let estimated_bytes = bytes
                .checked_add(SPLIT_SIZE_SAFETY_MARGIN_BYTES)
                .and_then(NonZeroU64::new)
                .ok_or_else(|| {
                    EngineError::new(
                        ErrorCode::EngineFailure,
                        format!("QPDF produced an invalid split-size estimate for page {page}"),
                    )
                })?;
            estimates.push(PageSizeEstimate {
                page: page_number,
                estimated_bytes,
            });
        }
        Ok(SplitSizeEstimateReport {
            estimates,
            evidence,
        })
    }

    fn prepare_merge_sources(
        &self,
        inputs: &[MergeEngineInput],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<Vec<PreparedSource>, EngineError> {
        inputs
            .iter()
            .map(|input| {
                if input.pages.is_empty() {
                    return Err(EngineError::new(
                        ErrorCode::InvalidInput,
                        "QPDF merge input resolved to zero pages",
                    ));
                }
                let Some(password) = input.password.as_ref() else {
                    return Ok(PreparedSource {
                        path: input.source.clone(),
                        pages: input.pages.clone(),
                        _password_file: None,
                        _decrypted: None,
                    });
                };
                let password_file = PasswordFile::create(password)
                    .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;
                let decrypted = TemporaryPath::new("decrypted-source", "pdf").map_err(|error| {
                    EngineError::new(
                        ErrorCode::Internal,
                        format!("cannot create private decryption directory: {error}"),
                    )
                })?;
                let capture = self.run_qpdf(
                    &[
                        password_file.argument(),
                        OsString::from("--decrypt"),
                        qpdf_path(&input.source),
                        qpdf_path(decrypted.path()),
                    ],
                    vec![
                        "--password-file=<redacted>".to_owned(),
                        "--decrypt".to_owned(),
                        input.source.display().to_string(),
                        decrypted.path().display().to_string(),
                    ],
                    control,
                    true,
                )?;
                evidence.push(capture.evidence);
                Ok(PreparedSource {
                    path: decrypted.path().to_path_buf(),
                    pages: input.pages.clone(),
                    _password_file: Some(password_file),
                    _decrypted: Some(decrypted),
                })
            })
            .collect()
    }

    fn bookmark_plan(
        &self,
        request: &MergeEngineRequest,
        prepared: &[PreparedSource],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<BookmarkPlan, EngineError> {
        let policy = request.bookmark_policy;
        let inputs = &request.inputs;
        let toc_pages = toc_page_count(request.toc_policy, inputs.len());
        if prepared.len() != inputs.len() {
            return Err(EngineError::new(
                ErrorCode::Internal,
                "prepared Merge sources no longer match the request",
            ));
        }
        let json_control = json_capture_control(control, inputs.len(), inputs);
        let mut output_offset = toc_pages.saturating_add(1);
        let mut roots = Vec::new();
        for (prepared_source, input) in prepared.iter().zip(inputs) {
            let source_roots = if matches!(
                policy,
                BookmarkPolicy::Retain | BookmarkPolicy::RetainAsOneEntryPerDocument
            ) {
                let capture = self.run_qpdf(
                    &[
                        OsString::from("--json=2"),
                        OsString::from("--json-key=outlines"),
                        qpdf_path(&prepared_source.path),
                    ],
                    vec![
                        "--json=2".to_owned(),
                        "--json-key=outlines".to_owned(),
                        prepared_source.path.display().to_string(),
                    ],
                    &json_control,
                    false,
                )?;
                ensure_complete_json(&capture.evidence, "source bookmark tree")?;
                let parsed =
                    parse_source_bookmarks(&capture.evidence.stdout, input, output_offset)?;
                evidence.push(capture.evidence);
                parsed
            } else {
                Vec::new()
            };

            match policy {
                BookmarkPolicy::Discard => {}
                BookmarkPolicy::OneEntryPerDocument => roots.push(BookmarkPlanNode {
                    title: document_bookmark_title(&input.document_title),
                    page_position: Some(output_offset),
                    children: Vec::new(),
                }),
                BookmarkPolicy::Retain => roots.extend(source_roots),
                BookmarkPolicy::RetainAsOneEntryPerDocument => roots.push(BookmarkPlanNode {
                    title: document_bookmark_title(&input.document_title),
                    page_position: Some(output_offset),
                    children: source_roots,
                }),
            }
            let contributed_pages = input.pages.len()
                + usize::from(request.add_blank_page_if_odd && input.pages.len() % 2 == 1);
            output_offset = output_offset
                .checked_add(contributed_pages)
                .ok_or_else(|| {
                    EngineError::new(
                        ErrorCode::InvalidInput,
                        "bookmark page-position plan overflowed",
                    )
                })?;
        }
        Ok(BookmarkPlan { roots })
    }

    fn page_geometry(
        &self,
        source: &Path,
        page: PageNumber,
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<PdfPageGeometry, EngineError> {
        let capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=pages"),
                qpdf_path(source),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=pages".to_owned(),
                source.display().to_string(),
            ],
            &json_capture_control(
                control,
                1,
                &[MergeEngineInput {
                    source: source.to_path_buf(),
                    document_title: String::new(),
                    metadata_title: None,
                    pages: vec![page],
                    password: None,
                }],
            ),
            false,
        )?;
        ensure_complete_json(&capture.evidence, "source page geometry")?;
        let page_object = parse_page_object_reference(&capture.evidence.stdout, page)?;
        evidence.push(capture.evidence);

        let mut geometry = PdfPageGeometry::default();
        let mut current = Some(page_object);
        for _ in 0..64 {
            let Some((object, generation)) = current else {
                break;
            };
            if generation != 0 {
                return Err(invalid_qpdf_json(
                    "source page object used an unsupported non-zero generation",
                ));
            }
            let option = format!("--show-object={object}");
            let capture = self.run_qpdf(
                &[
                    OsString::from(&option),
                    OsString::from("--"),
                    qpdf_path(source),
                ],
                vec![
                    option.clone(),
                    "--".to_owned(),
                    source.display().to_string(),
                ],
                control,
                false,
            )?;
            ensure_complete_text(&capture.evidence, "source page geometry")?;
            let object_text = capture.evidence.stdout.clone();
            evidence.push(capture.evidence);
            geometry.media_box = geometry
                .media_box
                .or(parse_pdf_number_array(&object_text, "/MediaBox")?);
            geometry.crop_box = geometry
                .crop_box
                .or(parse_pdf_number_array(&object_text, "/CropBox")?);
            geometry.rotate = geometry
                .rotate
                .or(parse_pdf_integer(&object_text, "/Rotate")?);
            current = parse_pdf_reference(&object_text, "/Parent")?;
            if geometry.media_box.is_some() && current.is_none() {
                break;
            }
        }
        let Some(media_box) = geometry.media_box else {
            return Err(invalid_qpdf_json(
                "source page geometry omitted an inherited /MediaBox",
            ));
        };
        Ok(PdfPageGeometry {
            media_box: Some(media_box),
            crop_box: geometry.crop_box,
            rotate: geometry.rotate,
        })
    }

    /// Reads bounded `MuPDF` structured-text output so footer placement can avoid
    /// the page's occupied bottom band. A missing renderer is deliberately a
    /// non-fatal capability downgrade: the deterministic geometry-only position
    /// remains safe and keeps QPDF-only installations usable.
    fn page_text_bounds(
        &self,
        source: &Path,
        page: PageNumber,
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Option<TextBounds> {
        let page = page.get().to_string();
        let capture = run_process(
            &self.config.text_executable,
            &[
                OsString::from("draw"),
                OsString::from("-F"),
                OsString::from("stext"),
                qpdf_path(source),
                OsString::from(&page),
            ],
            vec![
                "draw".to_owned(),
                "-F".to_owned(),
                "stext".to_owned(),
                source.display().to_string(),
                page,
            ],
            control,
        )
        .ok()?;
        let bounds = parse_stext_bounds(&capture.evidence.stdout);
        evidence.push(capture.evidence);
        bounds
    }

    fn add_bookmark_plan(
        &self,
        input: &Path,
        output: &Path,
        plan: &BookmarkPlan,
        inputs: &[MergeEngineInput],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<usize, EngineError> {
        let json_control = json_capture_control(control, plan.node_count(), inputs);
        let layout_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=pages"),
                OsString::from("--json-key=qpdf"),
                OsString::from("--json-object=trailer"),
                qpdf_path(input),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=pages".to_owned(),
                "--json-key=qpdf".to_owned(),
                "--json-object=trailer".to_owned(),
                input.display().to_string(),
            ],
            &json_control,
            false,
        )?;
        ensure_complete_json(&layout_capture.evidence, "bookmark page layout")?;
        let layout = parse_outline_layout(&layout_capture.evidence.stdout)?;
        evidence.push(layout_capture.evidence);

        let object_selector = format!(
            "--json-object={},{}",
            layout.catalog_object, layout.catalog_generation
        );
        let catalog_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=qpdf"),
                OsString::from(&object_selector),
                qpdf_path(input),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=qpdf".to_owned(),
                object_selector,
                input.display().to_string(),
            ],
            control,
            false,
        )?;
        ensure_complete_json(&catalog_capture.evidence, "catalog object")?;
        let catalog = parse_catalog(&catalog_capture.evidence.stdout, &layout.catalog_reference)?;
        evidence.push(catalog_capture.evidence);

        let update = build_bookmark_update(&layout, catalog, &plan.roots)?;
        let update_path = TemporaryPath::new("bookmark-update", "json").map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot create private bookmark update: {error}"),
            )
        })?;
        write_json(update_path.path(), &update)?;

        let mut update_argument = OsString::from("--update-from-json=");
        update_argument.push(update_path.path());
        let update_capture = self.run_qpdf(
            &[qpdf_path(input), update_argument, qpdf_path(output)],
            vec![
                input.display().to_string(),
                "--update-from-json=<private-bookmark-plan>".to_owned(),
                output.display().to_string(),
            ],
            control,
            false,
        )?;
        evidence.push(update_capture.evidence);

        let check_capture = self.run_qpdf(
            &[OsString::from("--check"), qpdf_path(output)],
            vec!["--check".to_owned(), output.display().to_string()],
            control,
            false,
        )?;
        evidence.push(check_capture.evidence);

        let outline_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=outlines"),
                qpdf_path(output),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=outlines".to_owned(),
                output.display().to_string(),
            ],
            &json_control,
            false,
        )?;
        ensure_complete_json(&outline_capture.evidence, "generated bookmark tree")?;
        verify_bookmark_plan(&outline_capture.evidence.stdout, &plan.roots)?;
        evidence.push(outline_capture.evidence);
        Ok(plan.roots.len())
    }
}

impl PdfEnginePort for QpdfAdapter {
    fn identity(&self) -> EngineIdentity {
        self.identity.clone()
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_capabilities([
            PdfCapability::Inspect,
            PdfCapability::Merge,
            PdfCapability::Encryption,
        ])
    }

    fn inspect(
        &self,
        source: &Path,
        options: InspectOptions<'_>,
    ) -> Result<PdfMetadata, EngineError> {
        if !source.is_file() {
            return Err(EngineError::new(
                ErrorCode::InputUnreadable,
                format!("PDF source is not a readable file: {}", source.display()),
            ));
        }
        let secret = options
            .password
            .map(SecretString::new)
            .transpose()
            .map_err(|error| EngineError::new(ErrorCode::InvalidInput, error.to_string()))?;
        let password_file = secret
            .as_ref()
            .map(PasswordFile::create)
            .transpose()
            .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;

        let (password_args, password_display) = password_arguments(password_file.as_ref());
        let mut page_args = password_args.clone();
        page_args.push(OsString::from("--show-npages"));
        page_args.push(qpdf_path(source));
        let mut page_display = password_display.clone();
        page_display.push("--show-npages".to_owned());
        page_display.push(source.display().to_string());
        let page_capture = self.run_qpdf(
            &page_args,
            page_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let page_count = page_capture
            .evidence
            .stdout
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                EngineError::new(
                    ErrorCode::EngineFailure,
                    "qpdf returned a non-integer page count",
                )
            })?;

        let mut encryption_args = password_args.clone();
        encryption_args.push(OsString::from("--show-encryption"));
        encryption_args.push(qpdf_path(source));
        let mut encryption_display = password_display.clone();
        encryption_display.push("--show-encryption".to_owned());
        encryption_display.push(source.display().to_string());
        let encryption_capture = self.run_qpdf(
            &encryption_args,
            encryption_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let encrypted = !encryption_capture
            .evidence
            .stdout
            .to_ascii_lowercase()
            .contains("file is not encrypted");

        let qdf_path = TemporaryPath::new("inspect-qdf", "pdf").map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot create private inspection directory: {error}"),
            )
        })?;
        let mut qdf_args = password_args;
        qdf_args.extend([
            OsString::from("--qdf"),
            OsString::from("--object-streams=disable"),
            qpdf_path(source),
            qpdf_path(qdf_path.path()),
        ]);
        let mut qdf_display = password_display;
        qdf_display.extend([
            "--qdf".to_owned(),
            "--object-streams=disable".to_owned(),
            source.display().to_string(),
            qdf_path.path().display().to_string(),
        ]);
        self.run_qpdf(
            &qdf_args,
            qdf_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let qdf = fs::read(qdf_path.path()).map_err(|error| {
            EngineError::new(
                ErrorCode::EngineFailure,
                format!("cannot read normalized QPDF output: {error}"),
            )
        })?;

        Ok(PdfMetadata {
            page_count,
            document_title: qdf_document_title(&qdf),
            encrypted,
            pdf_version: read_pdf_version(source),
            has_bookmarks: qdf_catalog_has_key(&qdf, "/Outlines"),
            has_forms: qdf_catalog_has_key(&qdf, "/AcroForm"),
        })
    }
}

impl MergeEnginePort for QpdfAdapter {
    #[allow(clippy::too_many_lines)]
    fn merge(
        &self,
        request: &MergeEngineRequest,
        control: &ExecutionControl,
    ) -> Result<MergeEngineResult, EngineError> {
        if request.inputs.len() < 2 {
            return Err(EngineError::new(
                ErrorCode::InvalidInput,
                "QPDF merge requires at least two inputs",
            ));
        }
        let mut evidence = Vec::new();
        let prepared = self.prepare_merge_sources(&request.inputs, control, &mut evidence)?;
        let toc_pages = toc_page_count(request.toc_policy, request.inputs.len());
        let bookmark_plan = self.bookmark_plan(request, &prepared, control, &mut evidence)?;
        let reconstruct_bookmarks = !bookmark_plan.roots.is_empty();

        let blank_pages = prepared
            .iter()
            .zip(&request.inputs)
            .map(|(prepared_source, input)| {
                if !request.add_blank_page_if_odd || input.pages.len() % 2 == 0 {
                    return Ok(None);
                }
                let last_page = input.pages.last().copied().ok_or_else(|| {
                    EngineError::new(
                        ErrorCode::InvalidInput,
                        "odd-page Merge input unexpectedly had no selected final page",
                    )
                })?;
                let geometry =
                    self.page_geometry(&prepared_source.path, last_page, control, &mut evidence)?;
                let blank_page = TemporaryPath::new("merge-blank-page", "pdf")
                    .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;
                write_blank_page(blank_page.path(), &geometry).map_err(|error| {
                    EngineError::new(
                        ErrorCode::Internal,
                        format!("cannot create geometry-matched blank page: {error}"),
                    )
                })?;
                Ok(Some(GeneratedBlankPage {
                    file: blank_page,
                    geometry,
                }))
            })
            .collect::<Result<Vec<_>, EngineError>>()?;
        let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
        let mut display_args = vec!["--empty".to_owned(), "--pages".to_owned()];
        for (source, blank_page) in prepared.iter().zip(&blank_pages) {
            let page_spec = page_specification(&source.pages);
            args.push(qpdf_path(&source.path));
            args.push(OsString::from(&page_spec));
            display_args.push(source.path.display().to_string());
            display_args.push(page_spec);
            if let Some(blank_page) = blank_page {
                args.push(qpdf_path(blank_page.file.path()));
                args.push(OsString::from("1"));
                display_args.push("<generated-blank-page>".to_owned());
                display_args.push("1".to_owned());
            }
        }
        let needs_staging = reconstruct_bookmarks
            || request.add_filename_footer
            || !matches!(request.toc_policy, MergeTocPolicy::None);
        let assembled = needs_staging
            .then(|| TemporaryPath::new("merge-assembly-base", "pdf"))
            .transpose()
            .map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private Merge staging directory: {error}"),
                )
            })?;
        let merge_output = assembled
            .as_ref()
            .map_or(request.output.as_path(), TemporaryPath::path);
        args.push(OsString::from("--"));
        args.push(qpdf_path(merge_output));
        display_args.push("--".to_owned());
        display_args.push(merge_output.display().to_string());
        let merge_capture = self.run_qpdf(&args, display_args, control, false)?;
        evidence.push(merge_capture.evidence);

        let footer_output = if request.add_filename_footer {
            let output = TemporaryPath::new("merge-footer-output", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private filename-footer output: {error}"),
                )
            })?;
            let overlay = TemporaryPath::new("merge-footer-overlay", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private filename-footer overlay: {error}"),
                )
            })?;
            let mut footer_pages = Vec::new();
            for ((source, input), blank_page) in
                prepared.iter().zip(&request.inputs).zip(&blank_pages)
            {
                let title = document_bookmark_title(&input.document_title);
                for page in &source.pages {
                    let geometry =
                        self.page_geometry(&source.path, *page, control, &mut evidence)?;
                    let text_bounds =
                        self.page_text_bounds(&source.path, *page, control, &mut evidence);
                    footer_pages.push(FooterOverlayPage {
                        position: Some(footer_position_with_text(&geometry, text_bounds)),
                        geometry,
                        text: Some(title.clone()),
                    });
                }
                if let Some(blank_page) = blank_page {
                    footer_pages.push(FooterOverlayPage {
                        geometry: blank_page.geometry.clone(),
                        text: None,
                        position: None,
                    });
                }
            }
            write_footer_overlay(overlay.path(), &footer_pages).map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create filename-footer overlay: {error}"),
                )
            })?;
            let overlay_capture = self.run_qpdf(
                &[
                    qpdf_path(merge_output),
                    OsString::from("--overlay"),
                    qpdf_path(overlay.path()),
                    OsString::from("--"),
                    qpdf_path(output.path()),
                ],
                vec![
                    merge_output.display().to_string(),
                    "--overlay".to_owned(),
                    "<generated-filename-footer-overlay>".to_owned(),
                    "--".to_owned(),
                    output.path().display().to_string(),
                ],
                control,
                false,
            )?;
            evidence.push(overlay_capture.evidence);
            Some(output)
        } else {
            None
        };

        let toc_output = if toc_pages > 0 {
            let output = TemporaryPath::new("merge-toc-output", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private table-of-contents output: {error}"),
                )
            })?;
            let toc_pdf = TemporaryPath::new("merge-toc-pages", "pdf").map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create private table-of-contents PDF: {error}"),
                )
            })?;
            let entries = toc_entries(
                &prepared,
                &request.inputs,
                &blank_pages,
                request.toc_policy,
                toc_pages,
            );
            write_toc_pdf(toc_pdf.path(), &entries, toc_pages).map_err(|error| {
                EngineError::new(
                    ErrorCode::Internal,
                    format!("cannot create generated table of contents: {error}"),
                )
            })?;
            let source_output = footer_output
                .as_ref()
                .map_or(merge_output, TemporaryPath::path);
            let source_page_count = prepared
                .iter()
                .zip(&blank_pages)
                .map(|(source, blank)| source.pages.len() + usize::from(blank.is_some()))
                .sum::<usize>();
            let source_page_spec = format!("1-{source_page_count}");
            let toc_page_spec = format!("1-{toc_pages}");
            let toc_capture = self.run_qpdf(
                &[
                    OsString::from("--empty"),
                    OsString::from("--pages"),
                    qpdf_path(toc_pdf.path()),
                    OsString::from(&toc_page_spec),
                    qpdf_path(source_output),
                    OsString::from(&source_page_spec),
                    OsString::from("--"),
                    qpdf_path(output.path()),
                ],
                vec![
                    "--empty".to_owned(),
                    "--pages".to_owned(),
                    "<generated-toc-pages>".to_owned(),
                    toc_page_spec,
                    source_output.display().to_string(),
                    source_page_spec,
                    "--".to_owned(),
                    output.path().display().to_string(),
                ],
                control,
                false,
            )?;
            evidence.push(toc_capture.evidence);
            Some(output)
        } else {
            None
        };

        let pre_bookmark_output = toc_output.as_ref().map_or_else(
            || {
                footer_output
                    .as_ref()
                    .map_or(merge_output, TemporaryPath::path)
            },
            TemporaryPath::path,
        );
        if !reconstruct_bookmarks && pre_bookmark_output != request.output.as_path() {
            fs::rename(pre_bookmark_output, &request.output).map_err(|error| {
                EngineError::new(
                    ErrorCode::OutputWriteFailed,
                    format!("cannot finalize generated Merge output: {error}"),
                )
            })?;
        }

        let bookmark_entries = if reconstruct_bookmarks {
            self.add_bookmark_plan(
                pre_bookmark_output,
                &request.output,
                &bookmark_plan,
                &request.inputs,
                control,
                &mut evidence,
            )?
        } else {
            0
        };

        let page_capture = self.run_qpdf(
            &[OsString::from("--show-npages"), qpdf_path(&request.output)],
            vec![
                "--show-npages".to_owned(),
                request.output.display().to_string(),
            ],
            control,
            false,
        )?;
        let page_count = page_capture
            .evidence
            .stdout
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                EngineError::new(
                    ErrorCode::EngineFailure,
                    "qpdf returned a non-integer merged page count",
                )
            })?;
        evidence.push(page_capture.evidence);
        Ok(MergeEngineResult {
            page_count,
            bookmark_entries,
            evidence,
        })
    }
}

fn password_arguments(password_file: Option<&PasswordFile>) -> (Vec<OsString>, Vec<String>) {
    password_file.map_or_else(
        || (Vec::new(), Vec::new()),
        |file| {
            (
                vec![file.argument()],
                vec!["--password-file=<redacted>".to_owned()],
            )
        },
    )
}

fn page_specification(pages: &[PageNumber]) -> String {
    pages
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn toc_page_count(policy: MergeTocPolicy, source_count: usize) -> usize {
    match policy {
        MergeTocPolicy::None => 0,
        MergeTocPolicy::FileNames | MergeTocPolicy::DocumentTitles => {
            source_count.saturating_add(34) / 35
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct PdfPageGeometry {
    media_box: Option<[String; 4]>,
    crop_box: Option<[String; 4]>,
    rotate: Option<i32>,
}

fn parse_page_object_reference(
    document: &str,
    page: PageNumber,
) -> Result<(u64, u64), EngineError> {
    let value = parse_qpdf_json(document, "source page geometry")?;
    let pages = json_array(&value, "pages", "source page geometry")?;
    let entry = pages
        .iter()
        .find(|entry| {
            entry
                .get("pageposfrom1")
                .and_then(Value::as_u64)
                .is_some_and(|position| position == u64::from(page.get()))
        })
        .ok_or_else(|| invalid_qpdf_json("source page geometry omitted the selected page"))?;
    let reference = entry
        .get("object")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_qpdf_json("source page geometry omitted the page object"))?;
    parse_indirect_reference(reference, "source page")
}

fn parse_pdf_number_array(document: &str, key: &str) -> Result<Option<[String; 4]>, EngineError> {
    let Some(key_start) = document.find(key) else {
        return Ok(None);
    };
    let after_key = &document[key_start + key.len()..];
    let Some(open) = after_key.find('[') else {
        return Err(invalid_qpdf_json(format!("{key} was not an array")));
    };
    let after_open = &after_key[open + 1..];
    let Some(close) = after_open.find(']') else {
        return Err(invalid_qpdf_json(format!("{key} array was not closed")));
    };
    let values = after_open[..close]
        .split_whitespace()
        .map(|value| {
            value
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map_or_else(
                    || {
                        Err(invalid_qpdf_json(format!(
                            "{key} contained a non-numeric value"
                        )))
                    },
                    |_| Ok(value.to_owned()),
                )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 4 {
        return Err(invalid_qpdf_json(format!(
            "{key} must contain exactly four numbers"
        )));
    }
    values
        .try_into()
        .map(Some)
        .map_err(|_| invalid_qpdf_json(format!("{key} could not be normalized")))
}

fn parse_pdf_integer(document: &str, key: &str) -> Result<Option<i32>, EngineError> {
    let Some(key_start) = document.find(key) else {
        return Ok(None);
    };
    let value = document[key_start + key.len()..]
        .split_whitespace()
        .next()
        .ok_or_else(|| invalid_qpdf_json(format!("{key} omitted its value")))?;
    value
        .parse::<i32>()
        .map(Some)
        .map_err(|_| invalid_qpdf_json(format!("{key} contained a non-integer value")))
}

fn parse_pdf_reference(document: &str, key: &str) -> Result<Option<(u64, u64)>, EngineError> {
    let Some(key_start) = document.find(key) else {
        return Ok(None);
    };
    let value = document[key_start + key.len()..]
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>();
    if value.len() < 3 || value[2] != "R" {
        return Err(invalid_qpdf_json(format!(
            "{key} was not an indirect reference"
        )));
    }
    parse_indirect_reference(&value[..3].join(" "), key).map(Some)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BookmarkPlanNode {
    title: String,
    page_position: Option<usize>,
    children: Vec<Self>,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct BookmarkPlan {
    roots: Vec<BookmarkPlanNode>,
}

impl BookmarkPlan {
    fn node_count(&self) -> usize {
        count_bookmark_nodes(&self.roots)
    }
}

#[derive(Debug)]
struct AssignedBookmarkNode {
    id: u64,
    title: String,
    page_position: Option<usize>,
    children: Vec<Self>,
}

struct OutlineLayout {
    metadata: Value,
    max_object_id: u64,
    catalog_reference: String,
    catalog_object: u64,
    catalog_generation: u64,
    page_objects: Vec<String>,
}

fn document_bookmark_title(title: &str) -> String {
    Path::new(title)
        .file_stem()
        .filter(|stem| !stem.is_empty())
        .map_or_else(
            || title.to_owned(),
            |stem| stem.to_string_lossy().into_owned(),
        )
}

fn count_bookmark_nodes(nodes: &[BookmarkPlanNode]) -> usize {
    nodes.iter().fold(0_usize, |count, node| {
        count
            .saturating_add(1)
            .saturating_add(count_bookmark_nodes(&node.children))
    })
}

fn parse_source_bookmarks(
    document: &str,
    input: &MergeEngineInput,
    output_offset: usize,
) -> Result<Vec<BookmarkPlanNode>, EngineError> {
    const MAX_OUTLINE_DEPTH: usize = 128;
    const MAX_OUTLINE_NODES: usize = 20_000;
    let value = parse_qpdf_json(document, "source bookmark tree")?;
    let outlines = json_array(&value, "outlines", "source bookmark tree")?;
    let mut observed_nodes = 0_usize;
    outlines
        .iter()
        .map(|entry| {
            parse_source_bookmark_node(
                entry,
                input,
                output_offset,
                0,
                &mut observed_nodes,
                MAX_OUTLINE_DEPTH,
                MAX_OUTLINE_NODES,
            )
        })
        .filter_map(Result::transpose)
        .collect()
}

fn parse_bookmark_boundaries(document: &str) -> Result<Vec<BookmarkBoundary>, EngineError> {
    parse_bookmark_boundaries_at_depth(document, 0)
}

fn parse_bookmark_boundaries_at_depth(
    document: &str,
    selected_depth: u32,
) -> Result<Vec<BookmarkBoundary>, EngineError> {
    const MAX_OUTLINE_DEPTH: u32 = 128;
    const MAX_OUTLINE_NODES: usize = 20_000;
    let value = parse_qpdf_json(document, "split bookmark boundaries")?;
    let outlines = json_array(&value, "outlines", "split bookmark boundaries")?;
    let mut boundaries = Vec::new();
    let mut observed_nodes = 0_usize;
    for entry in outlines {
        collect_bookmark_boundaries(
            entry,
            0,
            selected_depth,
            &mut observed_nodes,
            MAX_OUTLINE_DEPTH,
            MAX_OUTLINE_NODES,
            &mut boundaries,
        )?;
    }
    Ok(boundaries)
}

fn collect_bookmark_boundaries(
    entry: &Value,
    depth: u32,
    selected_depth: u32,
    observed_nodes: &mut usize,
    max_depth: u32,
    max_nodes: usize,
    boundaries: &mut Vec<BookmarkBoundary>,
) -> Result<(), EngineError> {
    if depth > max_depth {
        return Err(invalid_qpdf_json(
            "split bookmark tree exceeded the supported depth",
        ));
    }
    *observed_nodes = observed_nodes.saturating_add(1);
    if *observed_nodes > max_nodes {
        return Err(invalid_qpdf_json(
            "split bookmark tree exceeded the supported node count",
        ));
    }
    let ordinal = *observed_nodes - 1;
    let title = entry
        .get("title")
        .and_then(Value::as_str)
        .filter(|title| !title.trim().is_empty())
        .ok_or_else(|| {
            invalid_qpdf_json(format!(
                "split bookmark {ordinal} omitted a non-empty title"
            ))
        })?;
    if depth == selected_depth {
        let page = entry
            .get("destpageposfrom1")
            .and_then(Value::as_u64)
            .and_then(|page| u32::try_from(page).ok())
            .and_then(|page| PageNumber::new(page).ok())
            .ok_or_else(|| {
                invalid_qpdf_json(format!(
                    "split bookmark {ordinal} at depth {selected_depth} omitted a valid page destination"
                ))
            })?;
        boundaries.push(BookmarkBoundary {
            title: title.to_owned(),
            page,
            depth,
        });
    }
    if let Some(children) = entry.get("kids").and_then(Value::as_array) {
        for child in children {
            collect_bookmark_boundaries(
                child,
                depth.saturating_add(1),
                selected_depth,
                observed_nodes,
                max_depth,
                max_nodes,
                boundaries,
            )?;
        }
    }
    Ok(())
}

fn parse_source_bookmark_node(
    entry: &Value,
    input: &MergeEngineInput,
    output_offset: usize,
    depth: usize,
    observed_nodes: &mut usize,
    max_depth: usize,
    max_nodes: usize,
) -> Result<Option<BookmarkPlanNode>, EngineError> {
    if depth > max_depth {
        return Err(invalid_qpdf_json(
            "source bookmark tree exceeded the supported depth",
        ));
    }
    *observed_nodes = observed_nodes.saturating_add(1);
    if *observed_nodes > max_nodes {
        return Err(invalid_qpdf_json(
            "source bookmark tree exceeded the supported node count",
        ));
    }
    let title = entry
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_qpdf_json("source bookmark item omitted its title"))?
        .to_owned();
    let children = entry
        .get("kids")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_qpdf_json("source bookmark item omitted its children"))?
        .iter()
        .map(|child| {
            parse_source_bookmark_node(
                child,
                input,
                output_offset,
                depth.saturating_add(1),
                observed_nodes,
                max_depth,
                max_nodes,
            )
        })
        .filter_map(Result::transpose)
        .collect::<Result<Vec<_>, _>>()?;
    let page_position = entry
        .get("destpageposfrom1")
        .and_then(Value::as_u64)
        .and_then(|page| u32::try_from(page).ok())
        .and_then(|source_page| {
            input
                .pages
                .iter()
                .position(|page| page.get() == source_page)
        })
        .and_then(|relative| output_offset.checked_add(relative));
    if page_position.is_none() && children.is_empty() {
        return Ok(None);
    }
    Ok(Some(BookmarkPlanNode {
        title,
        page_position,
        children,
    }))
}

fn json_capture_control(
    control: &ExecutionControl,
    source_count: usize,
    inputs: &[MergeEngineInput],
) -> ExecutionControl {
    const MAX_JSON_CAPTURE: usize = 32 * 1024 * 1024;
    let page_count = inputs
        .iter()
        .map(|input| input.pages.len())
        .fold(0_usize, usize::saturating_add);
    let estimate = 64_usize
        .saturating_mul(1024)
        .saturating_add(page_count.saturating_mul(256))
        .saturating_add(source_count.saturating_mul(512))
        .min(MAX_JSON_CAPTURE);
    ExecutionControl::new(
        control.timeout(),
        control.output_limit_bytes().max(estimate),
        control.cancellation().clone(),
    )
}

fn parse_outline_layout(document: &str) -> Result<OutlineLayout, EngineError> {
    let value = parse_qpdf_json(document, "bookmark page layout")?;
    let qpdf = json_array(&value, "qpdf", "bookmark page layout")?;
    let metadata = qpdf.first().cloned().ok_or_else(|| {
        invalid_qpdf_json("bookmark page layout omitted the qpdf metadata record")
    })?;
    let max_object_id = metadata
        .get("maxobjectid")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_qpdf_json("bookmark page layout omitted maxobjectid"))?;
    let objects = qpdf
        .get(1)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_qpdf_json("bookmark page layout omitted the trailer record"))?;
    let catalog_reference = objects
        .get("trailer")
        .and_then(|trailer| trailer.get("value"))
        .and_then(|trailer| trailer.get("/Root"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_qpdf_json("bookmark page layout omitted trailer /Root"))?
        .to_owned();
    let (catalog_object, catalog_generation) =
        parse_indirect_reference(&catalog_reference, "catalog")?;

    let pages = json_array(&value, "pages", "bookmark page layout")?;
    let page_objects = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            page.get("object")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    invalid_qpdf_json(format!(
                        "output page {} was absent from QPDF JSON",
                        index + 1
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(OutlineLayout {
        metadata,
        max_object_id,
        catalog_reference,
        catalog_object,
        catalog_generation,
        page_objects,
    })
}

fn parse_catalog(document: &str, reference: &str) -> Result<Map<String, Value>, EngineError> {
    let value = parse_qpdf_json(document, "catalog object")?;
    let qpdf = json_array(&value, "qpdf", "catalog object")?;
    let objects = qpdf
        .get(1)
        .and_then(Value::as_object)
        .ok_or_else(|| invalid_qpdf_json("catalog JSON omitted its object map"))?;
    objects
        .get(&format!("obj:{reference}"))
        .and_then(|object| object.get("value"))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| invalid_qpdf_json("catalog JSON omitted the requested catalog value"))
}

fn build_bookmark_update(
    layout: &OutlineLayout,
    mut catalog: Map<String, Value>,
    roots: &[BookmarkPlanNode],
) -> Result<Value, EngineError> {
    if roots.is_empty() {
        return Err(invalid_qpdf_json(
            "cannot build an empty reconstructed bookmark tree",
        ));
    }
    let outline_id = layout.max_object_id.checked_add(1).ok_or_else(|| {
        invalid_qpdf_json("cannot allocate the output outline-root object identifier")
    })?;
    let first_item_id = outline_id.checked_add(1).ok_or_else(|| {
        invalid_qpdf_json("cannot allocate the first output bookmark object identifier")
    })?;
    let mut next_item_id = first_item_id;
    let assigned_roots = assign_bookmark_nodes(roots, &mut next_item_id)?;
    let last_item_id = next_item_id
        .checked_sub(1)
        .ok_or_else(|| invalid_qpdf_json("bookmark object identifier plan underflowed"))?;
    let outline_reference = indirect_reference(outline_id);

    catalog.insert(
        "/Outlines".to_owned(),
        Value::String(outline_reference.clone()),
    );
    catalog.insert(
        "/PageMode".to_owned(),
        Value::String("/UseOutlines".to_owned()),
    );

    let mut objects = Map::new();
    objects.insert(
        format!("obj:{}", layout.catalog_reference),
        json!({ "value": Value::Object(catalog) }),
    );
    objects.insert(
        format!("obj:{outline_reference}"),
        json!({
            "value": {
                "/Count": roots.len(),
                "/First": indirect_reference(assigned_roots[0].id),
                "/Last": indirect_reference(assigned_roots[assigned_roots.len() - 1].id),
                "/Type": "/Outlines"
            }
        }),
    );
    render_bookmark_nodes(&assigned_roots, &outline_reference, layout, &mut objects)?;

    let mut metadata = layout.metadata.clone();
    metadata
        .as_object_mut()
        .ok_or_else(|| invalid_qpdf_json("qpdf metadata record was not an object"))?
        .insert("maxobjectid".to_owned(), Value::from(last_item_id));

    Ok(json!({
        "version": 2,
        "parameters": {
            "decodelevel": "generalized"
        },
        "qpdf": [
            metadata,
            Value::Object(objects)
        ]
    }))
}

fn assign_bookmark_nodes(
    nodes: &[BookmarkPlanNode],
    next_item_id: &mut u64,
) -> Result<Vec<AssignedBookmarkNode>, EngineError> {
    nodes
        .iter()
        .map(|node| {
            let id = *next_item_id;
            *next_item_id = next_item_id
                .checked_add(1)
                .ok_or_else(|| invalid_qpdf_json("bookmark object identifier plan overflowed"))?;
            let children = assign_bookmark_nodes(&node.children, next_item_id)?;
            Ok(AssignedBookmarkNode {
                id,
                title: node.title.clone(),
                page_position: node.page_position,
                children,
            })
        })
        .collect()
}

fn render_bookmark_nodes(
    nodes: &[AssignedBookmarkNode],
    parent_reference: &str,
    layout: &OutlineLayout,
    objects: &mut Map<String, Value>,
) -> Result<(), EngineError> {
    for (index, node) in nodes.iter().enumerate() {
        let mut item = Map::new();
        item.insert(
            "/Parent".to_owned(),
            Value::String(parent_reference.to_owned()),
        );
        item.insert(
            "/Title".to_owned(),
            Value::String(format!("u:{}", node.title)),
        );
        if let Some(page_position) = node.page_position {
            let destination = layout
                .page_objects
                .get(page_position.saturating_sub(1))
                .ok_or_else(|| {
                    invalid_qpdf_json(format!(
                        "bookmark destination page {page_position} was absent from QPDF JSON"
                    ))
                })?;
            item.insert("/Dest".to_owned(), json!([destination, "/Fit"]));
        }
        if index > 0 {
            item.insert(
                "/Prev".to_owned(),
                Value::String(indirect_reference(nodes[index - 1].id)),
            );
        }
        if index + 1 < nodes.len() {
            item.insert(
                "/Next".to_owned(),
                Value::String(indirect_reference(nodes[index + 1].id)),
            );
        }
        if let Some(first_child) = node.children.first() {
            item.insert(
                "/First".to_owned(),
                Value::String(indirect_reference(first_child.id)),
            );
            item.insert(
                "/Last".to_owned(),
                Value::String(indirect_reference(
                    node.children.last().map_or(0, |child| child.id),
                )),
            );
            item.insert(
                "/Count".to_owned(),
                Value::from(u64::try_from(node.children.len()).map_err(|_| {
                    invalid_qpdf_json("bookmark child count exceeds the supported range")
                })?),
            );
        }
        let reference = indirect_reference(node.id);
        objects.insert(
            format!("obj:{reference}"),
            json!({ "value": Value::Object(item) }),
        );
        render_bookmark_nodes(&node.children, &reference, layout, objects)?;
    }
    Ok(())
}

fn verify_bookmark_plan(document: &str, expected: &[BookmarkPlanNode]) -> Result<(), EngineError> {
    let value = parse_qpdf_json(document, "generated bookmark tree")?;
    let outlines = json_array(&value, "outlines", "generated bookmark tree")?;
    verify_bookmark_nodes(outlines, expected)
}

fn verify_bookmark_nodes(
    actual: &[Value],
    expected: &[BookmarkPlanNode],
) -> Result<(), EngineError> {
    if actual.len() != expected.len() {
        return Err(invalid_qpdf_json(format!(
            "generated bookmark count mismatch: expected {}, observed {}",
            expected.len(),
            actual.len()
        )));
    }
    for (entry, expectation) in actual.iter().zip(expected) {
        let title = entry.get("title").and_then(Value::as_str);
        let page_position = entry.get("destpageposfrom1").and_then(Value::as_u64);
        let children = entry
            .get("kids")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_qpdf_json("generated bookmark item omitted its children"))?;
        if title != Some(expectation.title.as_str())
            || page_position
                != expectation
                    .page_position
                    .and_then(|page| u64::try_from(page).ok())
        {
            return Err(invalid_qpdf_json(format!(
                "generated bookmark did not match title {:?} at its planned destination",
                expectation.title
            )));
        }
        verify_bookmark_nodes(children, &expectation.children)?;
    }
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<(), EngineError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot create private bookmark JSON: {error}"),
            )
        })?;
    serde_json::to_writer(&mut file, value).map_err(|error| {
        EngineError::new(
            ErrorCode::Internal,
            format!("cannot serialize private bookmark JSON: {error}"),
        )
    })?;
    file.write_all(b"\n")
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot persist private bookmark JSON: {error}"),
            )
        })
}

fn parse_qpdf_json(document: &str, context: &str) -> Result<Value, EngineError> {
    serde_json::from_str(document)
        .map_err(|error| invalid_qpdf_json(format!("cannot parse QPDF {context} JSON: {error}")))
}

fn ensure_complete_json(evidence: &CommandEvidence, context: &str) -> Result<(), EngineError> {
    if evidence.stdout_truncated {
        Err(invalid_qpdf_json(format!(
            "QPDF {context} JSON exceeded the bounded capture limit"
        )))
    } else {
        Ok(())
    }
}

fn ensure_complete_text(evidence: &CommandEvidence, context: &str) -> Result<(), EngineError> {
    if evidence.stdout_truncated {
        Err(invalid_qpdf_json(format!(
            "QPDF {context} output exceeded the bounded capture limit"
        )))
    } else {
        Ok(())
    }
}

fn json_array<'value>(
    value: &'value Value,
    key: &str,
    context: &str,
) -> Result<&'value [Value], EngineError> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| invalid_qpdf_json(format!("QPDF {context} JSON omitted {key}")))
}

fn parse_indirect_reference(reference: &str, context: &str) -> Result<(u64, u64), EngineError> {
    let parts = reference.split_ascii_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 || parts[2] != "R" {
        return Err(invalid_qpdf_json(format!(
            "QPDF {context} reference was malformed"
        )));
    }
    let object = parts[0].parse::<u64>().map_err(|_| {
        invalid_qpdf_json(format!("QPDF {context} object identifier was malformed"))
    })?;
    let generation = parts[1]
        .parse::<u64>()
        .map_err(|_| invalid_qpdf_json(format!("QPDF {context} generation was malformed")))?;
    Ok((object, generation))
}

fn indirect_reference(object: u64) -> String {
    format!("{object} 0 R")
}

fn invalid_qpdf_json(message: impl Into<String>) -> EngineError {
    EngineError::new(ErrorCode::EngineFailure, message)
}

fn qdf_catalog_has_key(qdf: &[u8], key: &str) -> bool {
    String::from_utf8_lossy(qdf)
        .split("endobj")
        .any(|object| object.contains("/Type /Catalog") && object.contains(key))
}

fn qdf_document_title(qdf: &[u8]) -> Option<String> {
    let marker = b"/Title";
    let start = qdf
        .windows(marker.len())
        .position(|window| window == marker)?
        + marker.len();
    let mut cursor = start;
    while qdf.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor = cursor.saturating_add(1);
    }
    let value = match qdf.get(cursor).copied()? {
        b'(' => {
            let (bytes, _) = parse_pdf_literal(&qdf[cursor..])?;
            bytes
        }
        b'<' => parse_pdf_hex_string(&qdf[cursor..])?,
        _ => return None,
    };
    decode_pdf_text(&value)
}

fn parse_pdf_literal(document: &[u8]) -> Option<(Vec<u8>, usize)> {
    if document.first().copied()? != b'(' {
        return None;
    }
    let mut output = Vec::new();
    let mut depth = 1_u32;
    let mut cursor = 1_usize;
    while let Some(&byte) = document.get(cursor) {
        cursor = cursor.saturating_add(1);
        match byte {
            b'(' => {
                depth = depth.saturating_add(1);
                output.push(byte);
            }
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((output, cursor));
                }
                output.push(byte);
            }
            b'\\' => {
                let escaped = document.get(cursor).copied()?;
                cursor = cursor.saturating_add(1);
                match escaped {
                    b'n' => output.push(b'\n'),
                    b'r' => output.push(b'\r'),
                    b't' => output.push(b'\t'),
                    b'b' => output.push(8),
                    b'f' => output.push(12),
                    b'(' | b')' | b'\\' => output.push(escaped),
                    b'\r' => {
                        if document.get(cursor) == Some(&b'\n') {
                            cursor = cursor.saturating_add(1);
                        }
                    }
                    b'\n' => {}
                    digit if (b'0'..=b'7').contains(&digit) => {
                        let mut value = u32::from(digit - b'0');
                        for _ in 0..2 {
                            let Some(next) = document.get(cursor).copied() else {
                                break;
                            };
                            if !(b'0'..=b'7').contains(&next) {
                                break;
                            }
                            value = value
                                .saturating_mul(8)
                                .saturating_add(u32::from(next - b'0'));
                            cursor = cursor.saturating_add(1);
                        }
                        output.push(u8::try_from(value).unwrap_or(b'?'));
                    }
                    other => output.push(other),
                }
            }
            other => output.push(other),
        }
    }
    None
}

fn parse_pdf_hex_string(document: &[u8]) -> Option<Vec<u8>> {
    if document.first().copied()? != b'<' {
        return None;
    }
    let mut nibbles = Vec::new();
    for byte in document.iter().copied().skip(1) {
        if byte == b'>' {
            break;
        }
        if byte.is_ascii_whitespace() {
            continue;
        }
        nibbles.push(hex_nibble(byte)?);
    }
    if nibbles.len() % 2 == 1 {
        nibbles.push(0);
    }
    Some(
        nibbles
            .chunks_exact(2)
            .map(|pair| (pair[0] << 4) | pair[1])
            .collect(),
    )
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_pdf_text(bytes: &[u8]) -> Option<String> {
    let decoded = if bytes.starts_with(&[0xfe, 0xff]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| {
            bytes
                .iter()
                .map(|byte| char::from(*byte))
                .collect::<String>()
        })
    };
    let title = decoded.trim_matches('\0').trim().to_owned();
    (!title.is_empty()).then_some(title)
}

fn read_pdf_version(source: &Path) -> Option<String> {
    let mut file = File::open(source).ok()?;
    let mut header = [0_u8; 16];
    let read = file.read(&mut header).ok()?;
    let header = std::str::from_utf8(&header[..read]).ok()?;
    header
        .strip_prefix("%PDF-")
        .and_then(|rest| rest.lines().next())
        .map(str::trim)
        .map(ToOwned::to_owned)
}

struct PreparedSource {
    path: PathBuf,
    pages: Vec<PageNumber>,
    _password_file: Option<PasswordFile>,
    _decrypted: Option<TemporaryPath>,
}

struct GeneratedBlankPage {
    file: TemporaryPath,
    geometry: PdfPageGeometry,
}

struct FooterOverlayPage {
    geometry: PdfPageGeometry,
    text: Option<String>,
    position: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct TextBounds {
    /// Top-most text coordinate reported by `MuPDF` (top-origin page space).
    top: f64,
    /// Bottom-most text coordinate reported by `MuPDF` (top-origin page space).
    bottom: f64,
}

struct TocEntry {
    title: String,
    page_position: usize,
}

struct PasswordFile {
    temporary: TemporaryPath,
}

impl PasswordFile {
    fn create(secret: &SecretString) -> io::Result<Self> {
        let temporary = TemporaryPath::new("password", "txt")?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(temporary.path())?;
        file.write_all(secret.expose_secret().as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(Self { temporary })
    }

    fn argument(&self) -> OsString {
        let mut argument = OsString::from("--password-file=");
        argument.push(self.temporary.path());
        argument
    }
}

fn write_blank_page(path: &Path, geometry: &PdfPageGeometry) -> io::Result<()> {
    let media_box = geometry
        .media_box
        .as_ref()
        .expect("blank page geometry always has a media box");
    let media_box = media_box.join(" ");
    let crop_box = geometry
        .crop_box
        .as_ref()
        .map(|values| format!(" /CropBox [{}]", values.join(" ")))
        .unwrap_or_default();
    let rotate = geometry
        .rotate
        .map_or_else(String::new, |value| format!(" /Rotate {value}"));
    let objects = [
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_owned(),
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_owned(),
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}]{crop_box}{rotate} /Contents 4 0 R >>\nendobj\n"
        ),
        "4 0 obj\n<< /Length 0 >>\nstream\nendstream\nendobj\n".to_owned(),
    ];
    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in objects {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object.as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice("xref\n0 5\n0000000000 65535 f \n".as_bytes());
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    fs::write(path, bytes)
}

fn write_footer_overlay(path: &Path, pages: &[FooterOverlayPage]) -> io::Result<()> {
    let page_count = pages.len();
    let font_object = 3_usize;
    let first_page_object = 4_usize;
    let first_content_object = first_page_object + page_count;
    let object_count = first_content_object + page_count - 1;
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", first_page_object + index))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_owned(),
        format!(
            "2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\nendobj\n"
        ),
        "3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n".to_owned(),
    ];
    for (index, page) in pages.iter().enumerate() {
        let page_object = first_page_object + index;
        let content_object = first_content_object + index;
        let media_box = page
            .geometry
            .media_box
            .as_ref()
            .expect("footer overlay page geometry always has a media box")
            .join(" ");
        let crop_box = page
            .geometry
            .crop_box
            .as_ref()
            .map(|values| format!(" /CropBox [{}]", values.join(" ")))
            .unwrap_or_default();
        let rotate = page
            .geometry
            .rotate
            .map_or_else(String::new, |value| format!(" /Rotate {value}"));
        objects.push(format!(
            "{page_object} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [{media_box}]{crop_box}{rotate} /Resources << /Font << /F1 {font_object} 0 R >> >> /Contents {content_object} 0 R >>\nendobj\n"
        ));
        let stream = page.text.as_deref().map_or_else(String::new, |text| {
            let (x, y) = page
                .position
                .unwrap_or_else(|| footer_position(&page.geometry));
            format!(
                "BT /F1 8 Tf {x:.2} {y:.2} Td ({}) Tj ET\n",
                pdf_literal(text)
            )
        });
        objects.push(format!(
            "{content_object} 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
            stream.len()
        ));
    }

    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in objects {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object.as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", object_count + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            object_count + 1
        )
        .as_bytes(),
    );
    fs::write(path, bytes)
}

fn footer_position(geometry: &PdfPageGeometry) -> (f64, f64) {
    let box_values = geometry.crop_box.as_ref().or(geometry.media_box.as_ref());
    let Some(box_values) = box_values else {
        return (24.0, 18.0);
    };
    let parse = |value: &str| value.parse::<f64>().unwrap_or(0.0);
    (parse(&box_values[0]) + 24.0, parse(&box_values[1]) + 18.0)
}

const FOOTER_QUIET_BAND: f64 = 36.0;

fn footer_position_with_text(
    geometry: &PdfPageGeometry,
    text_bounds: Option<TextBounds>,
) -> (f64, f64) {
    let box_values = geometry.crop_box.as_ref().or(geometry.media_box.as_ref());
    let Some(box_values) = box_values else {
        return footer_position(geometry);
    };
    let parse = |value: &str| value.parse::<f64>().unwrap_or(0.0);
    let x0 = parse(&box_values[0]);
    let y0 = parse(&box_values[1]);
    let y1 = parse(&box_values[3]);
    let bottom = (x0 + 24.0, y0 + 18.0);
    let Some(bounds) = text_bounds else {
        return bottom;
    };

    // MuPDF reports structured-text boxes in a top-origin coordinate system;
    // PDF overlay coordinates use the opposite origin. Keep a 36pt quiet band
    // around either edge and use whichever edge has more available clearance.
    let content_low = y1 - bounds.bottom;
    let content_high = y1 - bounds.top;
    let bottom_clearance = content_low - y0;
    let top_clearance = y1 - content_high;
    if bottom_clearance >= FOOTER_QUIET_BAND || bottom_clearance >= top_clearance {
        return bottom;
    }
    if top_clearance >= FOOTER_QUIET_BAND {
        return (x0 + 24.0, y1 - 24.0);
    }

    // A page with text touching both edges has no collision-free band. Pick the
    // roomier edge deterministically; semantic text remains visible and the
    // evidence layer records that placement was bounded by page geometry.
    if top_clearance > bottom_clearance {
        (x0 + 24.0, y1 - 24.0)
    } else {
        bottom
    }
}

fn parse_stext_bounds(text: &str) -> Option<TextBounds> {
    let mut bounds = None;
    for line in text.lines() {
        let Some(start) = line.find("bbox=\"") else {
            continue;
        };
        let values = line[start + 6..]
            .split_once('"')
            .map(|(value, _)| value)
            .and_then(|value| {
                let values = value
                    .split_whitespace()
                    .map(str::parse::<f64>)
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                (values.len() == 4).then_some(values)
            });
        let Some(values) = values else {
            continue;
        };
        if !values.iter().all(|value| value.is_finite()) {
            continue;
        }
        let entry = bounds.get_or_insert(TextBounds {
            top: values[1],
            bottom: values[3],
        });
        entry.top = entry.top.min(values[1]);
        entry.bottom = entry.bottom.max(values[3]);
    }
    bounds
}

fn pdf_literal(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\\' => "\\\\".to_owned(),
            '(' => "\\(".to_owned(),
            ')' => "\\)".to_owned(),
            '\n' | '\r' | '\t' => " ".to_owned(),
            character if character.is_ascii() && !character.is_ascii_control() => {
                character.to_string()
            }
            _ => "?".to_owned(),
        })
        .collect()
}

fn toc_entries(
    prepared: &[PreparedSource],
    inputs: &[MergeEngineInput],
    blank_pages: &[Option<GeneratedBlankPage>],
    toc_policy: MergeTocPolicy,
    toc_pages: usize,
) -> Vec<TocEntry> {
    let mut page_position = toc_pages.saturating_add(1);
    let mut entries = Vec::with_capacity(inputs.len());
    for ((source, input), blank_page) in prepared.iter().zip(inputs).zip(blank_pages) {
        entries.push(TocEntry {
            title: toc_title(input, toc_policy),
            page_position,
        });
        page_position = page_position
            .saturating_add(source.pages.len())
            .saturating_add(usize::from(blank_page.is_some()));
    }
    entries
}

fn toc_title(input: &MergeEngineInput, policy: MergeTocPolicy) -> String {
    match policy {
        MergeTocPolicy::DocumentTitles => input
            .metadata_title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map_or_else(
                || document_bookmark_title(&input.document_title),
                ToOwned::to_owned,
            ),
        MergeTocPolicy::None | MergeTocPolicy::FileNames => {
            document_bookmark_title(&input.document_title)
        }
    }
}

fn write_toc_pdf(path: &Path, entries: &[TocEntry], page_count: usize) -> io::Result<()> {
    let font_object = 3_usize;
    let first_page_object = 4_usize;
    let first_content_object = first_page_object + page_count;
    let object_count = first_content_object + page_count - 1;
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", first_page_object + index))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_owned(),
        format!(
            "2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {page_count} >>\nendobj\n"
        ),
        "3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n".to_owned(),
    ];
    for index in 0..page_count {
        let page_object = first_page_object + index;
        let content_object = first_content_object + index;
        objects.push(format!(
            "{page_object} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 {font_object} 0 R >> >> /Contents {content_object} 0 R >>\nendobj\n"
        ));
        let mut stream = String::from("BT /F1 16 Tf 48 800 Td (Table of contents) Tj ET\n");
        stream.push_str("BT /F1 8 Tf\n");
        for (line_index, entry) in entries
            .chunks(35)
            .nth(index)
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let y = 770_i32 - i32::try_from(line_index).unwrap_or(0) * 20;
            let _ = writeln!(
                stream,
                "1 0 0 1 48 {y} Tm ({}) Tj",
                pdf_literal(&format!("{}    page {}", entry.title, entry.page_position))
            );
        }
        stream.push_str("ET\n");
        objects.push(format!(
            "{content_object} 0 obj\n<< /Length {} >>\nstream\n{stream}endstream\nendobj\n",
            stream.len()
        ));
    }

    let mut bytes = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for object in objects {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object.as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", object_count + 1).as_bytes(),
    );
    for offset in offsets {
        bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    bytes.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            object_count + 1
        )
        .as_bytes(),
    );
    fs::write(path, bytes)
}

fn qpdf_path(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
}

fn parse_qpdf_page_count(stdout: &str, context: &str) -> Result<u32, EngineError> {
    stdout.trim().parse::<u32>().map_err(|_| {
        EngineError::new(
            ErrorCode::EngineFailure,
            format!("qpdf returned a non-integer {context} page count"),
        )
    })
}

fn stage_split_output(source: &Path, destination: &Path) -> Result<(), EngineError> {
    fs::copy(source, destination).map_err(|error| {
        EngineError::new(
            ErrorCode::OutputWriteFailed,
            format!("cannot stage split output: {error}"),
        )
    })?;
    fs::remove_file(source).map_err(|error| {
        EngineError::new(
            ErrorCode::OutputWriteFailed,
            format!("cannot clean split staging output: {error}"),
        )
    })
}

struct TemporaryPath {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryPath {
    fn new(purpose: &str, extension: &str) -> io::Result<Self> {
        for _ in 0..32 {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("pincerpdf-{purpose}-{}-{id}", std::process::id()));
            #[cfg(unix)]
            let builder = {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder
            };
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            match builder.create(&directory) {
                Ok(()) => {
                    let path = directory.join(format!("payload.{extension}"));
                    return Ok(Self { directory, path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private PincerPDF temporary directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

struct BoundedText {
    text: String,
    truncated: bool,
}

#[derive(Debug)]
struct ProcessCapture {
    evidence: CommandEvidence,
}

#[derive(Clone, Copy, Debug)]
enum ProcessFailureKind {
    Spawn,
    Cancelled,
    TimedOut,
    Exit,
    Join,
}

#[derive(Debug)]
struct ProcessFailure {
    kind: ProcessFailureKind,
    evidence: Box<CommandEvidence>,
    message: String,
}

fn run_process(
    program: &Path,
    args: &[OsString],
    display_args: Vec<String>,
    control: &ExecutionControl,
) -> Result<ProcessCapture, ProcessFailure> {
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ProcessFailure {
            kind: ProcessFailureKind::Spawn,
            evidence: Box::new(empty_evidence(
                program,
                display_args.clone(),
                started.elapsed(),
            )),
            message: format!("cannot start PDF engine: {error}"),
        })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let limit = control.output_limit_bytes();
    let stdout_reader = thread::spawn(move || read_bounded(stdout, limit));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, limit));

    let mut forced_kind = None;
    let status = loop {
        if control.cancellation().is_cancelled() {
            forced_kind = Some(ProcessFailureKind::Cancelled);
            let _ = child.kill();
            break child.wait();
        }
        if started.elapsed() >= control.timeout() {
            forced_kind = Some(ProcessFailureKind::TimedOut);
            let _ = child.kill();
            break child.wait();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => break Err(error),
        }
    };

    let status = status.map_err(|error| ProcessFailure {
        kind: ProcessFailureKind::Exit,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: format!("cannot wait for PDF engine: {error}"),
    })?;
    let stdout = stdout_reader.join().map_err(|_| ProcessFailure {
        kind: ProcessFailureKind::Join,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: "PDF engine stdout reader panicked".to_owned(),
    })?;
    let stderr = stderr_reader.join().map_err(|_| ProcessFailure {
        kind: ProcessFailureKind::Join,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: "PDF engine stderr reader panicked".to_owned(),
    })?;
    let evidence = evidence(
        program,
        display_args,
        status,
        started.elapsed(),
        stdout,
        stderr,
    );

    if let Some(kind) = forced_kind {
        let message = match kind {
            ProcessFailureKind::Cancelled => "PDF engine operation was cancelled",
            ProcessFailureKind::TimedOut => "PDF engine operation timed out",
            _ => "PDF engine operation was interrupted",
        };
        return Err(ProcessFailure {
            kind,
            evidence: Box::new(evidence),
            message: message.to_owned(),
        });
    }
    if status.success() {
        Ok(ProcessCapture { evidence })
    } else {
        Err(ProcessFailure {
            kind: ProcessFailureKind::Exit,
            message: if evidence.stderr.is_empty() {
                "PDF engine exited unsuccessfully".to_owned()
            } else {
                format!("PDF engine failed: {}", evidence.stderr)
            },
            evidence: Box::new(evidence),
        })
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> BoundedText {
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = limit.saturating_sub(retained.len());
                let keep = remaining.min(read);
                retained.extend_from_slice(&buffer[..keep]);
                truncated |= keep < read;
            }
        }
    }
    BoundedText {
        text: String::from_utf8_lossy(&retained).trim().to_owned(),
        truncated,
    }
}

fn empty_evidence(program: &Path, arguments: Vec<String>, duration: Duration) -> CommandEvidence {
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
