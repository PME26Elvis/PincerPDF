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
        for (part, output_plan) in plan.parts.iter().zip(&planned) {
            let page_spec = page_specification(&part.pages);
            let capture = self.run_qpdf(
                &[
                    OsString::from("--empty"),
                    OsString::from("--pages"),
                    qpdf_path(&plan.source),
                    OsString::from(&page_spec),
                    OsString::from("--"),
                    qpdf_path(&output_plan.temporary_path),
                ],
                vec![
                    "--empty".to_owned(),
                    "--pages".to_owned(),
                    plan.source.display().to_string(),
                    page_spec,
                    "--".to_owned(),
                    output_plan.temporary_path.display().to_string(),
                ],
                control,
                false,
            )?;
            evidence.push(capture.evidence);
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
            let temporary = TemporaryPath::new("split-size-estimÛ®töÚ$z{-®éÜj×Óâ&÷VæFVEFW‡B°¢ÆWB×WB&WF–æVBÒfV3£§v—F…ö66—G’†Æ–Ö—BæÖ–âƒƒ“"’“°¢ÆWB×WBG'Væ6FVBÒfÇ6S°¢ÆWB×WB'VffW"Ò³÷Sƒ²ƒ“%Ó°¢Æö÷°¢ÖF6‚&VFW"ç&VB‚f×WB'VffW"’°¢ö²ƒ’ÂW'"…ò’Óâ'&V²À¢ö²‡&VB’Óâ°¢ÆWB&VÖ–æ–ærÒÆ–Ö—Bç6GW&F–æu÷7V"‡&WF–æVBæÆVâ‚’“°¢ÆWB¶VWÒ&VÖ–æ–æræÖ–â‡&VB“°¢&WF–æVBæW‡FVæEög&öÕ÷6Æ–6R‚f'VffW%²âæ¶VWÒ“°¢G'Væ6FVBÃÒ¶VWÂ&VC°¢Ð¢Ð¢Ð¢&÷VæFVEFW‡B°¢FW‡C¢7G&–æs£¦g&öÕ÷WFc…öÆ÷77’‚g&WF–æVB’çG&–Ò‚’çFõö÷væVB‚’À¢G'Væ6FVBÀ¢Ð§Ð ¦fâV×G•öWf–FVæ6R‡&öw&Ó¢eF‚Â&wVÖVçG3¢fV3Å7G&–æsâÂGW&F–öã¢GW&F–öâ’Óâ6öÖÖæDWf–FVæ6R°¢6öÖÖæDWf–FVæ6R°¢&öw&Ó¢&öw&ÒæF—7Æ’‚’çFõ÷7G&–ær‚’À¢&wVÖVçG2À¢W†—Eö6öFS¢æöæRÀ¢GW&F–öåö×3¢GW&F–öâæ5öÖ–ÆÆ—2‚’À¢7FF÷WC¢7G&–æs£¦æWr‚’À¢7FFW'#¢7G&–æs£¦æWr‚’À¢7FF÷WE÷G'Væ6FVC¢fÇ6RÀ¢7FFW'%÷G'Væ6FVC¢fÇ6RÀ¢Ð§Ð ¦fâWf–FVæ6R€¢&öw&Ó¢eF‚À¢&wVÖVçG3¢fV3Å7G&–æsâÀ¢7FGW3¢W†—E7FGW2À¢GW&F–öã¢GW&F–öâÀ¢7FF÷WC¢&÷VæFVEFW‡BÀ¢7FFW'#¢&÷VæFVEFW‡BÀ¢’Óâ6öÖÖæDWf–FVæ6R°¢6öÖÖæDWf–FVæ6R°¢&öw&Ó¢&öw&ÒæF—7Æ’‚’çFõ÷7G&–ær‚’À¢&wVÖVçG2À¢W†—Eö6öFS¢7FGW2æ6öFR‚’À¢GW&F–öåö×3¢GW&F–öâæ5öÖ–ÆÆ—2‚’À¢7FF÷WC¢7FF÷WBçFW‡BÀ¢7FFW'#¢7FFW'"çFW‡BÀ¢7FF÷WE÷G'Væ6FVC¢7FF÷WBçG'Væ6FVBÀ¢7FFW'%÷G'Væ6FVC¢7FFW'"çG'Væ6FVBÀ¢Ð§Ð ¦fâÖ÷&ö6W75öf–ÇW&R†f–ÇW&S¢e&ö6W74f–ÇW&RÂ77v÷&E÷7WÆ–VC¢&ööÂ’ÓâVæv–æTW'&÷"°¢ÆWB6öÖ&–æVBÐ¢f÷&ÖB‚'·Ò·Ò"Âf–ÇW&RæWf–FVæ6Rç7FF÷WBÂf–ÇW&RæWf–FVæ6Rç7FFW'"’çFõö66–•öÆ÷vW&66R‚“°¢ÆWB6öFRÒÖF6‚f–ÇW&Ræ¶–æB°¢&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVBÓâW'&÷$6öFS£¤6æ6VÆÆVBÀ¢&ö6W74f–ÇW&T¶–æC£¥7vâÂ&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WBÂ&ö6W74f–ÇW&T¶–æC£¤¦ö–âÓâ°¢W'&÷$6öFS£¤Væv–æTf–ÇW&P¢Ð¢&ö6W74f–ÇW&T¶–æC£¤W†—@¢–b6öÖ&–æVBæ6öçF–ç2‚&–çfÆ–B77v÷&B"¢ÇÂ6öÖ&–æVBæ6öçF–ç2‚&–æ6÷'&V7B77v÷&B"¢ÇÂ6öÖ&–æVBæ6öçF–ç2‚'77v÷&B—2–æ6÷'&V7B"’Óà¢°¢W'&÷$6öFS£¤–æ6÷'&V7E77v÷&@¢Ð¢&ö6W74f–ÇW&T¶–æC£¤W†—B–b6öÖ&–æVBæ6öçF–ç2‚'77v÷&B"’bb77v÷&E÷7WÆ–VBÓâ°¢W'&÷$6öFS£¥77v÷&E&WV—&V@¢Ð¢&ö6W74f–ÇW&T¶–æC£¤W†—BÓâW'&÷$6öFS£¤Væv–æTf–ÇW&RÀ¢Ó°¢ÆWB6öÖÖæBÒf÷&ÖB€¢'·Ò·Ò"À¢f–ÇW&RæWf–FVæ6Rç&öw&ÒÀ¢f–ÇW&RæWf–FVæ6Ræ&wVÖVçG2æ¦ö–â‚""¢“°¢Væv–æTW'&÷#£¦æWr†6öFRÂf÷&ÖB‚'·Ó²6öÖÖæC¢¶6öÖÖæGÒ"Âf–ÇW&RæÖW76vR’§Ð ¢5¶6fr‡FW7B•Ð¦ÖöBFW7G2°¢W6R7WW#£¢£°¢5¶6fr‡Væ—‚•Ð¢W6R–æ6W'FeöÖW&vS£¤6æ6VÆÆF–öåFö¶Vã° ¢5¶6fr‡Væ—‚•Ð¢5·FW7EÐ¢fâ&÷VæFVEö6GW&UöG&–ç5ö'WE÷&WF–ç5ööæÇ•÷F†Uö6öæf–wW&VEöÆ–Ö—B‚’°¢ÆWB6öçG&öÂÐ¢W†V7WF–öä6öçG&öÃ£¦æWr„GW&F–öã£¦g&öÕ÷6V72ƒ"’ÂRÂ6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚’“°¢ÆWB6GW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'&–çFb#3CScsƒ“"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'&–çFbÇFW7BÖFFâ"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7B‚'&ö6W727V66VVG2"“°¢76W'EöW†6GW&RæWf–FVæ6Rç7FF÷WBÂ##3CR"“°¢76W'B†6GW&RæWf–FVæ6Rç7FF÷WE÷G'Væ6FVB“°¢Ð ¢5¶6fr‡Væ—‚•Ð¢5·FW7EÐ¢fâF–ÖV÷WEö¶–ÆÇ5÷F†Uö6†–ÆE÷&ö6W72‚’°¢ÆWB6öçG&öÂÒW†V7WF–öä6öçG&öÃ£¦æWr€¢GW&F–öã£¦g&öÕöÖ–ÆÆ—2ƒC’À¢#BÀ¢6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚’À¢“°¢ÆWBf–ÇW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'6ÆVW"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'6ÆVW"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7EöW'"‚'&ö6W72×W7BF–ÖR÷WB"“°¢76W'B†ÖF6†W2†f–ÇW&Ræ¶–æBÂ&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WB’“°¢Ð ¢5¶6fr‡Væ—‚•Ð¢5·FW7EÐ¢fâ6æ6VÆÆF–öåö¶–ÆÇ5÷F†Uö6†–ÆE÷&ö6W72‚’°¢ÆWBFö¶VâÒ6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚“°¢ÆWB6æ6VÆÆF–öâÒFö¶Vâæ6ÆöæR‚“°¢ÆWB†æFÆRÒF‡&VC£§7vâ†Ö÷fRÇÂ°¢F‡&VC£§6ÆVW„GW&F–öã£¦g&öÕöÖ–ÆÆ—2ƒC’“°¢6æ6VÆÆF–öâæ6æ6VÂ‚“°¢Ò“°¢ÆWB6öçG&öÂÒW†V7WF–öä6öçG&öÃ£¦æWr„GW&F–öã£¦g&öÕ÷6V72ƒ"’Â#BÂFö¶Vâ“°¢ÆWBf–ÇW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'6ÆVW"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'6ÆVW"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7EöW'"‚'&ö6W72×W7B&R6æ6VÆÆVB"“°¢†æFÆRæ¦ö–â‚’æW‡V7B‚&6æ6VÆÆF–öâF‡&VB¦ö–ç2"“°¢76W'B†ÖF6†W2†f–ÇW&Ræ¶–æBÂ&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVB’“°¢Ð ¢5·FW7EÐ¢fâvU÷7V6–f–6F–öå÷&W6W'fW5ö÷&FW%öæEöGWÆ–6FW2‚’°¢ÆWBvW2Ò°¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢Ó°¢76W'EöW‡vU÷7V6–f–6F–öâ‚gvW2’Â#2ÃÃ2"“°¢Ð ¢5·FW7EÐ¢fâvUövVöÖWG'•÷'6W%ö66WG5ö&÷†W5÷&÷FF–öåöæE÷&VçE÷&VfW&Væ6R‚’°¢ÆWBvRÒ#ÃÂõG—RõvRõ&VçB""ôÖVF–&÷‚²ÓÓ#c"s“"Òô7&÷&÷‚²csÒõ&÷FFR#sãâ#°¢76W'EöW€¢'6U÷FeöçVÖ&W%ö'&’‡vRÂ"ôÖVF–&÷‚"’æW‡V7B‚&ÖVF–&÷‚"’À¢6öÖR…°¢"Ó"çFõö÷væVB‚’À¢"Ó#"çFõö÷væVB‚’À¢#c""çFõö÷væVB‚’À¢#s“""çFõö÷væVB‚’À¢Ò¢“°¢76W'EöW€¢'6U÷FeöçVÖ&W%ö'&’‡vRÂ"ô7&÷&÷‚"’æW‡V7B‚&7&÷&÷‚"’À¢6öÖR…°¢#"çFõö÷væVB‚’À¢#"çFõö÷væVB‚’À¢#c"çFõö÷væVB‚’À¢#s"çFõö÷væVB‚’À¢Ò¢“°¢76W'EöW€¢'6U÷Feö–çFVvW"‡vRÂ"õ&÷FFR"’æW‡V7B‚'&÷FF–öâ"’À¢6öÖRƒ#s¢“°¢76W'EöW€¢'6U÷Fe÷&VfW&Væ6R‡vRÂ"õ&VçB"’æW‡V7B‚'&VçB"’À¢6öÖR‚ƒ"Â’¢“°¢Ð ¢5·FW7EÐ¢fâfö÷FW%÷÷6—F–öå÷&VfW'5÷F†U÷f—6–&ÆUö7&÷ö&÷‚‚’°¢ÆWBvVöÖWG'’ÒFevTvVöÖWG'’°¢ÖVF–ö&÷ƒ¢6öÖR…°¢"Ó"çFõö÷væVB‚’À¢"Ó#"çFõö÷væVB‚’À¢#c""çFõö÷væVB‚’À¢#s“""çFõö÷væVB‚’À¢Ò’À¢7&÷ö&÷ƒ¢6öÖR…°¢##"çFõö÷væVB‚’À¢#3"çFõö÷væVB‚’À¢#Sƒ"çFõö÷væVB‚’À¢#sc"çFõö÷væVB‚’À¢Ò’À¢&÷FFS¢æöæRÀ¢Ó°¢76W'EöW†fö÷FW%÷÷6—F–öâ‚fvVöÖWG'’’ÂƒCBãÂC‚ã’“°¢Ð ¢5·FW7EÐ¢fâfö÷FW%÷÷6—F–öåöÖ÷fW5÷Fõ÷F÷÷v†Våö&÷GFöÕö&æEö—5öö67W–VB‚’°¢ÆWBvVöÖWG'’ÒFevTvVöÖWG'’°¢ÖVF–ö&÷ƒ¢6öÖR…°¢#"çFõö÷væVB‚’À¢#"çFõö÷væVB‚’À¢#c"çFõö÷væVB‚’À¢#ƒ"çFõö÷væVB‚’À¢Ò’À¢7&÷ö&÷ƒ¢æöæRÀ¢&÷FFS¢æöæRÀ¢Ó°¢ÆWB&÷VæG2ÒFW‡D&÷VæG2°¢F÷¢cãÀ¢&÷GFöÓ¢s“ãÀ¢Ó°¢76W'EöW€¢fö÷FW%÷÷6—F–öå÷v—F…÷FW‡B‚fvVöÖWG'’Â6öÖR†&÷VæG2’’À¢ƒ#BãÂssbã¢“°¢Ð ¢5·FW7EÐ¢fâ7G'V7GW&VE÷FW‡Eö&÷VæG5ö&Uö&÷VæFVE÷Fõö&Æö6µö&÷†W2‚’°¢ÆWBFW‡BÒ"2#ÇvR–CÒ'vS#à£Æ&Æö6²&&÷ƒÒ#s"S"ãcRc‚ãsrã3‚#à£ÆÆ–æR&&÷ƒÒ#s"s"#s##à£ÂöÆ–æSà£Âö&Æö6³à£Â÷vSâ"3°¢76W'EöW€¢'6U÷7FW‡Eö&÷VæG2‡FW‡B’À¢6öÖR…FW‡D&÷VæG2°¢F÷¢S"ãcRÀ¢&÷GFöÓ¢s#ãÀ¢Ò¢“°¢Ð ¢5·FW7EÐ¢fâFeöFö7VÖVçE÷F—FÆUöFV6öFW5öÆ—FW&ÅöæE÷WFceö†W…÷fÇVW2‚’°¢76W'EöW€¢FeöFö7VÖVçE÷F—FÆR†"#Bö&¥ÆãÃÂõF—FÆR…V'FW&Ç’ÅÂ†G&gEÅÂ’’ãåÆæVæFö&¢"’À¢6öÖR‚%V'FW&Ç’†G&gB’"çFõö÷væVB‚’¢“°¢76W'EöW€¢FeöFö7VÖVçE÷F—FÆR†"#Bö&¥ÆãÃÂõF—FÆRÄdTdcSc“dSc3âãåÆæVæFö&¢"’À¢6öÖR‚%–æ2"çFõö÷væVB‚’¢“°¢Ð ¢5·FW7EÐ¢fâFeöFö7VÖVçE÷F—FÆUö–væ÷&W5öV×G•öæEöÖÆf÷&ÖVE÷fÇVW2‚’°¢76W'EöW‡FeöFö7VÖVçE÷F—FÆR†"#ÃÂõF—FÆR‚’ãâ"’ÂæöæR“°¢76W'EöW‡FeöFö7VÖVçE÷F—FÆR†"#ÃÂõF—FÆR‡VçFW&Ö–æFVBãâ"’ÂæöæR“°¢76W'EöW‡FeöFö7VÖVçE÷F—FÆR†"#ÃÂõF—FÆRÄtsâãâ"’ÂæöæR“°¢Ð ¢5·FW7EÐ¢fâFö7VÖVçE÷F—FÆUö6öçFVçG5öfÆÇ5ö&6µ÷Fõöf–ÆVæÖR‚’°¢ÆWB–çWBÒÖW&vTVæv–æT–çWB°¢6÷W&6S¢F„'Vc£¦g&öÒ‚'&W÷'BçFb"’À¢Fö7VÖVçE÷F—FÆS¢'&W÷'BçFb"çFõö÷væVB‚’À¢ÖWFFF÷F—FÆS¢6öÖR‚""çFõö÷væVB‚’’À¢vW3¢fV2µvTçVÖ&W#£¦æWrƒ’æW‡V7B‚'fÆ–B"•ÒÀ¢77v÷&C¢æöæRÀ¢Ó°¢76W'EöW‡Fö5÷F—FÆR‚f–çWBÂÖW&vUFö5öÆ–7“£¤Fö7VÖVçEF—FÆW2’Â'&W÷'B"“°¢76W'EöW‡Fö5÷F—FÆR‚f–çWBÂÖW&vUFö5öÆ–7“£¤f–ÆTæÖW2’Â'&W÷'B"“°¢Ð ¢5·FW7EÐ¢fâ6÷W&6Uö&öö¶Ö&µ÷Æå÷'VæW5öW†6ÇVFVEöÆVfW5öæEöÖ5÷6VÆV7FVE÷vW2‚’°¢ÆWB–çWBÒÖW&vTVæv–æT–çWB°¢6÷W&6S¢F„'Vc£¦g&öÒ‚&&öö¶Ö&·2çFb"’À¢Fö7VÖVçE÷F—FÆS¢&&öö¶Ö&·2çFb"çFõö÷væVB‚’À¢ÖWFFF÷F—FÆS¢æöæRÀ¢vW3¢fV2°¢vTçVÖ&W#£¦æWrƒ"’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢ÒÀ¢77v÷&C¢æöæRÀ¢Ó°¢ÆWB6÷W&6RÒ"2'°¢&÷WFÆ–æW2#¢°¢²'F—FÆR#¢$6†FW""Â&FW7GvW÷6g&öÓ#£Â&¶–G2#¥µ×ÒÀ¢²'F—FÆR#¢$6†FW"""Â&FW7GvW÷6g&öÓ#£"Â&¶–G2#¥°¢²'F—FÆR#¢$VæF—‚"Â&FW7GvW÷6g&öÓ#£2Â&¶–G2#¥µ×Ð¢×Ð¢Ð¢Ò"3° ¢76W'EöW€¢'6U÷6÷W&6Uö&öö¶Ö&·2‡6÷W&6RÂf–çWBÂB’æW‡V7B‚'6÷W&6RÆâ"’À¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢$6†FW"""çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒB’À¢6†–ÆG&Vã¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢$VæF—‚"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒR’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÕÒÀ¢ÕÐ¢“°¢76W'EöW†Fö7VÖVçEö&öö¶Ö&µ÷F—FÆR‚&&öö¶Ö&·2çFb"’Â&&öö¶Ö&·2"“°¢Ð ¢5·FW7EÐ¢fâ&öö¶Ö&µö&÷VæF'•÷'6W%ö¶VW5ööæÇ•ö÷&FW&VE÷F÷öÆWfVÅ÷vU÷F&vWG2‚’°¢ÆWB§6öâÒ"2'°¢&÷WFÆ–æW2#¢°¢²'F—FÆR#¢$–çG&ò"Â&FW7GvW÷6g&öÓ#£Â&¶–G2#¥µ×ÒÀ¢²'F—FÆR#¢$6†FW"""Â&FW7GvW÷6g&öÓ#£2Â&¶–G2#¥°¢²'F—FÆR#¢$VæF—‚"Â&FW7GvW÷6g&öÓ#£BÂ&¶–G2#¥µ×Ð¢×Ð¢Ð¢Ò"3°¢76W'EöW€¢'6Uö&öö¶Ö&µö&÷VæF&–W2†§6öâ’æW‡V7B‚'fÆ–B&÷VæF&–W2"’À¢fV2°¢&öö¶Ö&´&÷VæF'’°¢F—FÆS¢$–çG&ò"çFõö÷væVB‚’À¢vS¢vTçVÖ&W#£¦æWrƒ’æW‡V7B‚'vR"’À¢FWFƒ¢À¢ÒÀ¢&öö¶Ö&´&÷VæF'’°¢F—FÆS¢$6†FW"""çFõö÷væVB‚’À¢vS¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'vR"’À¢FWFƒ¢À¢ÒÀ¢Ð¢“°¢Ð ¢5·FW7EÐ¢fâ&öö¶Ö&µö&÷VæF'•÷'6W%÷6VÆV7G5öæW7FVEöFWF…öæE÷&W6W'fW5ö÷&FW"‚’°¢ÆWB§6öâÒ"2'°¢&÷WFÆ–æW2#¢°¢²'F—FÆR#¢$6†FW""Â&FW7GvW÷6g&öÓ#£Â&¶–G2#¥°¢²'F—FÆR#¢%6V7F–öâã"Â&FW7GvW÷6g&öÓ#£Â&¶–G2#¥µ×ÒÀ¢²'F—FÆR#¢%6V7F–öâã""Â&FW7GvW÷6g&öÓ#£"Â&¶–G2#¥µ×Ð¢×ÒÀ¢²'F—FÆR#¢$6†FW"""Â&FW7GvW÷6g&öÓ#£2Â&¶–G2#¥°¢²'F—FÆR#¢%6V7F–öâ"ã"Â&FW7GvW÷6g&öÓ#£2Â&¶–G2#¥µ×Ð¢×Ð¢Ð¢Ò"3°¢76W'EöW€¢'6Uö&öö¶Ö&µö&÷VæF&–W5öEöFWF‚†§6öâÂ’æW‡V7B‚&æW7FVB&÷VæF&–W2"’À¢fV2°¢&öö¶Ö&´&÷VæF'’°¢F—FÆS¢%6V7F–öâã"çFõö÷væVB‚’À¢vS¢vTçVÖ&W#£¦æWrƒ’æW‡V7B‚'vR"’À¢FWFƒ¢À¢ÒÀ¢&öö¶Ö&´&÷VæF'’°¢F—FÆS¢%6V7F–öâã""çFõö÷væVB‚’À¢vS¢vTçVÖ&W#£¦æWrƒ"’æW‡V7B‚'vR"’À¢FWFƒ¢À¢ÒÀ¢&öö¶Ö&´&÷VæF'’°¢F—FÆS¢%6V7F–öâ"ã"çFõö÷væVB‚’À¢vS¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'vR"’À¢FWFƒ¢À¢ÒÀ¢Ð¢“°¢Ð ¢5·FW7EÐ¢fâ&öö¶Ö&µö&÷VæF'•÷'6W%öÆÆ÷w5÷Vç&W6öÇfVEö–çFW&ÖVF–FUöFW7F–æF–öç2‚’°¢ÆWB§6öâÒ"2'°¢&÷WFÆ–æW2#¢·²'F—FÆR#¢$6†FW""Â&¶–G2#¥°¢²'F—FÆR#¢%6V7F–öâ"Â&FW7GvW÷6g&öÓ#£"Â&¶–G2#¥µ×Ð¢×ÕÐ¢Ò"3°¢ÆWB&÷VæF&–W2Ò'6Uö&öö¶Ö&µö&÷VæF&–W5öEöFWF‚†§6öâÂ’æW‡V7B‚&6†–ÆB&÷VæF'’"“°¢76W'EöW†&÷VæF&–W5³ÒçF—FÆRÂ%6V7F–öâ"“°¢76W'EöW†&÷VæF&–W5³ÒçvRævWB‚’Â"“°¢Ð ¢5·FW7EÐ¢fâ&öö¶Ö&µ÷WFFU÷&W6W'fW5ö6FÆöuöæEöFöW5öæ÷E÷&WÆ6U÷F†U÷G&–ÆW"‚’°¢ÆWBÆ–÷WBÒ÷WFÆ–æTÆ–÷WB°¢ÖWFFF¢§6öâ‡°¢&§6öçfW'6–öâ#¢"À¢'FgfW'6–öâ#¢#ãr"À¢&Ö†ö&¦V7F–B#¢p¢Ò’À¢Ö…öö&¦V7Eö–C¢rÀ¢6FÆöu÷&VfW&Væ6S¢#""çFõö÷væVB‚’À¢6FÆöuöö&¦V7C¢À¢6FÆöuövVæW&F–öã¢À¢vUöö&¦V7G3¢fV2²#2""çFõö÷væVB‚’Â#R""çFõö÷væVB‚’Â#r""çFõö÷væVB‚•ÒÀ¢Ó°¢ÆWB6FÆörÒ§6öâ‡°¢"õvW2#¢#"""À¢"õG—R#¢"ô6FÆör"À¢"ôÆær#¢'S¦Vâ ¢Ò¢æ5öö&¦V7B‚¢æW‡V7B‚&6FÆörö&¦V7B"¢æ6ÆöæR‚“°¢ÆWBW‡V7FVBÒfV2°¢&öö¶Ö&µÆäæöFR°¢F—FÆS¢&öæR"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ’À¢6†–ÆG&Vã¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢&6†–ÆB"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ2’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÕÒÀ¢ÒÀ¢&öö¶Ö&µÆäæöFR°¢F—FÆS¢'Gvò"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ"’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÒÀ¢Ó° ¢ÆWBWFFRÒ'V–ÆEö&öö¶Ö&µ÷WFFR‚fÆ–÷WBÂ6FÆörÂfW‡V7FVB’æW‡V7B‚&&öö¶Ö&²WFFR"“°¢ÆWBö&¦V7G2ÒWFFU²'Fb%Õ³Òæ5öö&¦V7B‚’æW‡V7B‚'WFFRö&¦V7G2"“° ¢76W'B‚ö&¦V7G2æ6öçF–ç5ö¶W’‚'G&–ÆW""’“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"õvW2%ÒÀ¢fÇVS£¥7G&–ær‚#"""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôÆær%ÒÀ¢fÇVS£¥7G&–ær‚'S¦Vâ"çFõö÷væVB‚’¢“°¢76W'EöW†ö&¦V7G5²&ö&££‚"%Õ²'fÇVR%Õ²"ô6÷VçB%ÒÂfÇVS£¦g&öÒƒ"’“°¢76W'EöW€¢ö&¦V7G5²&ö&££’"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#2""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#r""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#R""çFõö÷væVB‚’¢“°¢Ð§Ð 