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
    