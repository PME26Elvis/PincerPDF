#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
//! Engine-independent Merge vertical slice for `PincerPDF`.

use pincerpdf_application::{MissingCapabilities, validate_tool_capabilities};
use pincerpdf_domain::{ErrorCode, PageNumber, PageSelection, ResolveSelectionError, ToolKind};
use pincerpdf_engine_api::{
    CapabilitySet, EngineError, EngineIdentity, InspectOptions, PdfEnginePort, PdfMetadata,
};
use pincerpdf_filesystem::{
    ExistingOutputPolicy, OutputPathPlan, OutputPlanError, plan_output_path,
};
use std::error::Error;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

#[cfg(unix)]
use std::fs::File;

static NEXT_OPERATION_ID: AtomicU64 = AtomicU64::new(1);

/// Secret text whose debug representation never exposes its value.
#[derive(Clone, Eq, PartialEq)]
pub struct SecretString(String);

impl SecretString {
    /// Creates a secret suitable for the line-oriented QPDF password-file contract.
    ///
    /// # Errors
    ///
    /// Returns [`SecretStringError`] when the value contains a line break or NUL byte.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretStringError> {
        let value = value.into();
        if value
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0'))
        {
            Err(SecretStringError)
        } else {
            Ok(Self(value))
        }
    }

    /// Exposes the secret only to an adapter that must authenticate an input.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.clear();
    }
}

/// A password cannot be represented safely by the line-oriented adapter boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecretStringError;

impl fmt::Display for SecretStringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("passwords must not contain line breaks or NUL bytes")
    }
}

impl Error for SecretStringError {}

/// One source document in user-defined merge order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeSource {
    path: PathBuf,
    selection: Option<PageSelection>,
    password: Option<SecretString>,
}

impl MergeSource {
    /// Creates a source that contributes all pages.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            selection: None,
            password: None,
        }
    }

    /// Applies an ordered page selection. Deliberate duplicates remain significant.
    #[must_use]
    pub fn with_selection(mut self, selection: PageSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Supplies an input password without changing debug redaction.
    #[must_use]
    pub fn with_password(mut self, password: SecretString) -> Self {
        self.password = Some(password);
        self
    }

    /// Returns the source path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the optional ordered page selection.
    #[must_use]
    pub fn selection(&self) -> Option<&PageSelection> {
        self.selection.as_ref()
    }

    /// Returns the optional source password.
    #[must_use]
    pub fn password(&self) -> Option<&SecretString> {
        self.password.as_ref()
    }
}

/// Validated user intent for the first Merge vertical slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeRequest {
    sources: Vec<MergeSource>,
    output: PathBuf,
}

impl MergeRequest {
    /// Validates a merge request while preserving source order and duplicates.
    ///
    /// # Errors
    ///
    /// Returns [`MergeRequestError`] for fewer than two sources, blank paths, a non-PDF
    /// destination, or a destination that is exactly one of the source paths.
    pub fn new(
        sources: impl IntoIterator<Item = MergeSource>,
        output: impl Into<PathBuf>,
    ) -> Result<Self, MergeRequestError> {
        let sources = sources.into_iter().collect::<Vec<_>>();
        let output = output.into();
        if sources.len() < 2 {
            return Err(MergeRequestError::NotEnoughSources);
        }
        for (index, source) in sources.iter().enumerate() {
            if source.path.as_os_str().is_empty() {
                return Err(MergeRequestError::EmptySourcePath {
                    source_index: index,
                });
            }
            if source.path == output {
                return Err(MergeRequestError::OutputEqualsSource {
                    source_index: index,
                });
            }
        }
        let extension_is_pdf = output
            .extension()
            .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("pdf"));
        if !extension_is_pdf {
            return Err(MergeRequestError::OutputMustBePdf);
        }
        Ok(Self { sources, output })
    }

    /// Returns sources in exact user-defined order.
    #[must_use]
    pub fn sources(&self) -> &[MergeSource] {
        &self.sources
    }

    /// Returns the requested final destination.
    #[must_use]
    pub fn output(&self) -> &Path {
        &self.output
    }
}

/// Merge request validation failure before filesystem or engine access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MergeRequestError {
    /// At least two source entries are required.
    NotEnoughSources,
    /// A source path was empty.
    EmptySourcePath {
        /// Zero-based position of the invalid source.
        source_index: usize,
    },
    /// The destination does not use a PDF extension.
    OutputMustBePdf,
    /// The destination exactly matched a source path.
    OutputEqualsSource {
        /// Zero-based position of the conflicting source.
        source_index: usize,
    },
}

impl fmt::Display for MergeRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnoughSources => {
                formatter.write_str("merge requires at least two source entries")
            }
            Self::EmptySourcePath { source_index } => {
                write!(
                    formatter,
                    "merge source {} has an empty path",
                    source_index + 1
                )
            }
            Self::OutputMustBePdf => formatter.write_str("merge output must use a .pdf extension"),
            Self::OutputEqualsSource { source_index } => write!(
                formatter,
                "merge output must not overwrite source entry {}",
                source_index + 1
            ),
        }
    }
}

impl Error for MergeRequestError {}

/// Cooperative cancellation shared by application and native-process adapters.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Returns whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Bounded process-execution policy for one engine operation.
#[derive(Clone, Debug)]
pub struct ExecutionControl {
    timeout: Duration,
    output_limit_bytes: usize,
    cancellation: CancellationToken,
}

impl ExecutionControl {
    /// Creates explicit timeout, capture, and cancellation limits.
    #[must_use]
    pub fn new(
        timeout: Duration,
        output_limit_bytes: usize,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            timeout,
            output_limit_bytes,
            cancellation,
        }
    }

    /// Returns the maximum elapsed process time.
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Returns the maximum retained bytes for each output stream.
    #[must_use]
    pub const fn output_limit_bytes(&self) -> usize {
        self.output_limit_bytes
    }

    /// Returns the cooperative cancellation token.
    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

impl Default for ExecutionControl {
    fn default() -> Self {
        Self::new(
            Duration::from_mins(2),
            64 * 1024,
            CancellationToken::default(),
        )
    }
}

/// Secret-safe, bounded evidence for one external command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandEvidence {
    /// Executable name or path.
    pub program: String,
    /// Display arguments with every secret replaced by a redaction marker.
    pub arguments: Vec<String>,
    /// Exit code when the operating system supplied one.
    pub exit_code: Option<i32>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u128,
    /// Retained standard output.
    pub stdout: String,
    /// Retained standard error.
    pub stderr: String,
    /// Whether standard output exceeded its retention limit.
    pub stdout_truncated: bool,
    /// Whether standard error exceeded its retention limit.
    pub stderr_truncated: bool,
}

/// Fully resolved source passed to an engine adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeEngineInput {
    /// Source path, possibly an adapter-owned decrypted temporary copy.
    pub source: PathBuf,
    /// Exact one-based page order, including deliberate duplicates.
    pub pages: Vec<PageNumber>,
    /// Optional source password.
    pub password: Option<SecretString>,
}

/// Engine request that writes only to an application-owned temporary path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeEngineRequest {
    /// Ordered source entries.
    pub inputs: Vec<MergeEngineInput>,
    /// Temporary output path; never the final user destination.
    pub output: PathBuf,
}

/// Successful adapter result before application-level semantic verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeEngineResult {
    /// Page count observed by the adapter after writing.
    pub page_count: u32,
    /// Redacted external-command evidence.
    pub evidence: Vec<CommandEvidence>,
}

/// Operation-specific port implemented by a concrete merge adapter.
pub trait MergeEnginePort: Send + Sync {
    /// Writes a merged PDF to `request.output`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] for cancellation, authentication, process, or output failures.
    fn merge(
        &self,
        request: &MergeEngineRequest,
        control: &ExecutionControl,
    ) -> Result<MergeEngineResult, EngineError>;
}

/// Complete engine boundary required by [`MergeService`].
pub trait MergeEngine: PdfEnginePort + MergeEnginePort {}

impl<T> MergeEngine for T where T: PdfEnginePort + MergeEnginePort {}

/// Application-level execution options.
#[derive(Clone, Debug)]
pub struct MergeExecutionOptions {
    /// Existing destination behavior.
    pub output_policy: ExistingOutputPolicy,
    /// Native-process limits and cancellation.
    pub control: ExecutionControl,
}

impl Default for MergeExecutionOptions {
    fn default() -> Self {
        Self {
            output_policy: ExistingOutputPolicy::Fail,
            control: ExecutionControl::default(),
        }
    }
}

/// Verified Merge result after atomic finalization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeReport {
    /// Final output path.
    pub output: PathBuf,
    /// Number of source entries, including deliberate duplicates.
    pub source_count: usize,
    /// Verified output page count.
    pub page_count: u32,
    /// Number of source documents whose bookmarks were intentionally discarded.
    pub bookmark_sources_discarded: usize,
    /// Concrete engine identity used for the operation.
    pub engine: EngineIdentity,
    /// Redacted adapter evidence.
    pub evidence: Vec<CommandEvidence>,
}

/// Merge execution failure with a stable application-facing code.
#[derive(Debug)]
pub enum MergeError {
    /// The configured engine lacks proven Merge behavior.
    MissingCapabilities(MissingCapabilities),
    /// A source could not be inspected.
    InspectSource {
        /// Zero-based source position.
        source_index: usize,
        /// Underlying engine error.
        error: EngineError,
    },
    /// The output resolves to the same existing path as a source.
    OutputAliasesSource {
        /// Zero-based source position.
        source_index: usize,
    },
    /// An inspected source contains no pages.
    EmptySourceDocument {
        /// Zero-based source position.
        source_index: usize,
    },
    /// An interactive form was detected and cannot yet be preserved safely.
    FormsUnsupported {
        /// Zero-based source position.
        source_index: usize,
    },
    /// A page selection exceeded the inspected document boundary.
    InvalidSelection {
        /// Zero-based source position.
        source_index: usize,
        /// Resolution error.
        error: ResolveSelectionError,
    },
    /// Output planning failed before execution.
    OutputPlan(OutputPlanError),
    /// Filesystem access or atomic finalization failed.
    OutputIo(String),
    /// The merge adapter failed.
    Engine(EngineError),
    /// The temporary output could not be inspected.
    VerifyOutput(EngineError),
    /// The engine output page count differed from the application plan.
    PageCountMismatch {
        /// Planned number of pages.
        expected: u32,
        /// Observed number of pages.
        actual: u32,
    },
    /// Output unexpectedly retained a bookmark tree.
    UnexpectedBookmarks,
    /// Output unexpectedly retained an interactive form.
    UnexpectedForms,
}

impl MergeError {
    /// Returns the stable error category used by CLI and future IPC.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::MissingCapabilities(_) => ErrorCode::CapabilityUnavailable,
            Self::InspectSource { error, .. } | Self::Engine(error) | Self::VerifyOutput(error) => {
                error.code()
            }
            Self::OutputAliasesSource { .. }
            | Self::EmptySourceDocument { .. }
            | Self::FormsUnsupported { .. }
            | Self::InvalidSelection { .. } => ErrorCode::InvalidInput,
            Self::OutputPlan(OutputPlanError::ExistingOutputConflict) => ErrorCode::OutputConflict,
            Self::OutputPlan(_) | Self::OutputIo(_) => ErrorCode::OutputWriteFailed,
            Self::PageCountMismatch { .. } | Self::UnexpectedBookmarks | Self::UnexpectedForms => {
                ErrorCode::EngineFailure
            }
        }
    }
}

impl fmt::Display for MergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCapabilities(error) => error.fmt(formatter),
            Self::InspectSource {
                source_index,
                error,
            } => {
                write!(
                    formatter,
                    "failed to inspect merge source {}: {error}",
                    source_index + 1
                )
            }
            Self::OutputAliasesSource { source_index } => write!(
                formatter,
                "merge output resolves to the same file as source {}",
                source_index + 1
            ),
            Self::EmptySourceDocument { source_index } => write!(
                formatter,
                "merge source {} contains no pages",
                source_index + 1
            ),
            Self::FormsUnsupported { source_index } => write!(
                formatter,
                "merge source {} contains an interactive form; form merge is not yet verified",
                source_index + 1
            ),
            Self::InvalidSelection {
                source_index,
                error,
            } => write!(
                formatter,
                "invalid page selection for merge source {}: {error}",
                source_index + 1
            ),
            Self::OutputPlan(error) => error.fmt(formatter),
            Self::OutputIo(message) => formatter.write_str(message),
            Self::Engine(error) => error.fmt(formatter),
            Self::VerifyOutput(error) => {
                write!(formatter, "failed to verify merge output: {error}")
            }
            Self::PageCountMismatch { expected, actual } => write!(
                formatter,
                "merge output page count mismatch: expected {expected}, observed {actual}"
            ),
            Self::UnexpectedBookmarks => {
                formatter.write_str("merge output unexpectedly retained a bookmark tree")
            }
            Self::UnexpectedForms => {
                formatter.write_str("merge output unexpectedly retained an interactive form")
            }
        }
    }
}

impl Error for MergeError {}

/// Engine-agnostic orchestration for the first Merge vertical slice.
pub struct MergeService<'engine, E: MergeEngine + ?Sized> {
    engine: &'engine E,
}

impl<'engine, E: MergeEngine + ?Sized> MergeService<'engine, E> {
    /// Creates a Merge service over one concrete engine composition.
    #[must_use]
    pub const fn new(engine: &'engine E) -> Self {
        Self { engine }
    }

    /// Inspects, plans, executes, verifies, and atomically finalizes one merge request.
    ///
    /// Bookmarks are intentionally discarded in P4.1. Inputs containing forms are rejected.
    ///
    /// # Errors
    ///
    /// Returns [`MergeError`] for capability, input, engine, verification, or filesystem failures.
    pub fn execute(
        &self,
        request: &MergeRequest,
        options: &MergeExecutionOptions,
    ) -> Result<MergeReport, MergeError> {
        validate_tool_capabilities(ToolKind::Merge, &self.engine.capabilities())
            .map_err(MergeError::MissingCapabilities)?;
        ensure_output_does_not_alias_source(request)?;

        let mut inputs = Vec::with_capacity(request.sources.len());
        let mut expected_pages = 0_u32;
        let mut bookmark_sources_discarded = 0_usize;

        for (source_index, source) in request.sources.iter().enumerate() {
            let metadata = self
                .engine
                .inspect(
                    source.path(),
                    InspectOptions {
                        password: source.password().map(SecretString::expose_secret),
                    },
                )
                .map_err(|error| MergeError::InspectSource {
                    source_index,
                    error,
                })?;
            if metadata.page_count == 0 {
                return Err(MergeError::EmptySourceDocument { source_index });
            }
            if metadata.has_forms {
                return Err(MergeError::FormsUnsupported { source_index });
            }
            bookmark_sources_discarded += usize::from(metadata.has_bookmarks);
            let pages = resolve_source_pages(source, &metadata).map_err(|error| {
                MergeError::InvalidSelection {
                    source_index,
                    error,
                }
            })?;
            expected_pages = expected_pages
                .checked_add(u32::try_from(pages.len()).map_err(|_| {
                    MergeError::OutputIo("merge page plan exceeds supported size".to_owned())
                })?)
                .ok_or_else(|| MergeError::OutputIo("merge page plan overflowed".to_owned()))?;
            inputs.push(MergeEngineInput {
                source: source.path.clone(),
                pages,
                password: source.password.clone(),
            });
        }

        let mut transaction = OutputTransaction::begin(request.output(), options.output_policy)?;
        let engine_result = self
            .engine
            .merge(
                &MergeEngineRequest {
                    inputs,
                    output: transaction.temporary_path().to_path_buf(),
                },
                &options.control,
            )
            .map_err(MergeError::Engine)?;

        let verified = self
            .engine
            .inspect(transaction.temporary_path(), InspectOptions::default())
            .map_err(MergeError::VerifyOutput)?;
        if verified.page_count != expected_pages || engine_result.page_count != expected_pages {
            return Err(MergeError::PageCountMismatch {
                expected: expected_pages,
                actual: verified.page_count,
            });
        }
        if verified.has_bookmarks {
            return Err(MergeError::UnexpectedBookmarks);
        }
        if verified.has_forms {
            return Err(MergeError::UnexpectedForms);
        }

        let output = transaction.commit()?;
        Ok(MergeReport {
            output,
            source_count: request.sources.len(),
            page_count: verified.page_count,
            bookmark_sources_discarded,
            engine: self.engine.identity(),
            evidence: engine_result.evidence,
        })
    }
}

fn ensure_output_does_not_alias_source(request: &MergeRequest) -> Result<(), MergeError> {
    let output_parent = request
        .output()
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = output_parent.canonicalize().map_err(|error| {
        MergeError::OutputIo(format!("cannot resolve merge output directory: {error}"))
    })?;
    let output_name = request
        .output()
        .file_name()
        .ok_or_else(|| MergeError::OutputIo("merge output path has no file name".to_owned()))?;
    let output_candidate = canonical_parent.join(output_name);

    for (source_index, source) in request.sources().iter().enumerate() {
        if let Ok(canonical_source) = source.path().canonicalize()
            && canonical_source == output_candidate
        {
            return Err(MergeError::OutputAliasesSource { source_index });
        }
    }
    Ok(())
}

fn resolve_source_pages(
    source: &MergeSource,
    metadata: &PdfMetadata,
) -> Result<Vec<PageNumber>, ResolveSelectionError> {
    if let Some(selection) = source.selection() {
        selection.resolve(metadata.page_count)
    } else {
        Ok((1..=metadata.page_count)
            .map(|page| PageNumber::new(page).expect("page range starts at one"))
            .collect())
    }
}

struct OutputTransaction {
    plan: OutputPathPlan,
    committed: bool,
}

impl OutputTransaction {
    fn begin(path: &Path, policy: ExistingOutputPolicy) -> Result<Self, MergeError> {
        let final_exists = path.try_exists().map_err(|error| {
            MergeError::OutputIo(format!("cannot inspect output path: {error}"))
        })?;
        let token = format!(
            "merge_{}_{}",
            process::id(),
            NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed)
        );
        let plan =
            plan_output_path(path, final_exists, policy, &token).map_err(MergeError::OutputPlan)?;
        if plan.temporary_path.try_exists().map_err(|error| {
            MergeError::OutputIo(format!("cannot inspect temporary output path: {error}"))
        })? {
            return Err(MergeError::OutputIo(
                "temporary merge output unexpectedly already exists".to_owned(),
            ));
        }
        Ok(Self {
            plan,
            committed: false,
        })
    }

    fn temporary_path(&self) -> &Path {
        &self.plan.temporary_path
    }

    fn commit(&mut self) -> Result<PathBuf, MergeError> {
        let temporary = &self.plan.temporary_path;
        let final_path = &self.plan.final_path;
        if !temporary.is_file() {
            return Err(MergeError::OutputIo(
                "engine did not produce a regular temporary PDF".to_owned(),
            ));
        }
        OpenOptions::new()
            .write(true)
            .open(temporary)
            .and_then(|file| file.sync_all())
            .map_err(|error| MergeError::OutputIo(format!("cannot sync temporary PDF: {error}")))?;
        if !self.plan.replace_existing
            && final_path.try_exists().map_err(|error| {
                MergeError::OutputIo(format!("cannot recheck final output path: {error}"))
            })?
        {
            return Err(MergeError::OutputPlan(
                OutputPlanError::ExistingOutputConflict,
            ));
        }
        fs::rename(temporary, final_path).map_err(|error| {
            MergeError::OutputIo(format!("cannot atomically finalize PDF: {error}"))
        })?;
        if let Some(parent) = final_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            #[cfg(unix)]
            sync_output_directory(parent)?;
            #[cfg(not(unix))]
            {
                // std::fs cannot open a Windows directory for FlushFileBuffers. The
                // temporary file itself was durably flushed before the same-volume rename.
                let _ = parent;
            }
        }
        self.committed = true;
        Ok(final_path.clone())
    }
}

#[cfg(unix)]
fn sync_output_directory(parent: &Path) -> Result<(), MergeError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| MergeError::OutputIo(format!("cannot sync output directory: {error}")))
}

impl Drop for OutputTransaction {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.plan.temporary_path);
        }
    }
}

/// Returns the capability set required by P4.1 Merge.
#[must_use]
pub fn merge_core_capabilities() -> CapabilitySet {
    pincerpdf_application::required_capabilities(ToolKind::Merge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pincerpdf_engine_api::{PdfCapability, PdfMetadata};
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;

    struct FakeEngine {
        output_pages: AtomicU32,
    }

    impl FakeEngine {
        fn new() -> Self {
            Self {
                output_pages: AtomicU32::new(0),
            }
        }
    }

    impl PdfEnginePort for FakeEngine {
        fn identity(&self) -> EngineIdentity {
            EngineIdentity {
                id: "fake".to_owned(),
                version: "1".to_owned(),
            }
        }

        fn capabilities(&self) -> CapabilitySet {
            CapabilitySet::from_capabilities([PdfCapability::Inspect, PdfCapability::Merge])
        }

        fn inspect(
            &self,
            source: &Path,
            _options: InspectOptions<'_>,
        ) -> Result<PdfMetadata, EngineError> {
            let is_output = source
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("pincerpdf-merge_"));
            Ok(PdfMetadata {
                page_count: if is_output {
                    self.output_pages.load(AtomicOrdering::Acquire)
                } else {
                    3
                },
                encrypted: false,
                pdf_version: Some("1.7".to_owned()),
                has_bookmarks: source.to_string_lossy().contains("bookmarks"),
                has_forms: source.to_string_lossy().contains("form"),
            })
        }
    }

    impl MergeEnginePort for FakeEngine {
        fn merge(
            &self,
            request: &MergeEngineRequest,
            _control: &ExecutionControl,
        ) -> Result<MergeEngineResult, EngineError> {
            let pages = request
                .inputs
                .iter()
                .map(|input| input.pages.len())
                .sum::<usize>();
            let pages = u32::try_from(pages).expect("test page count fits u32");
            self.output_pages.store(pages, AtomicOrdering::Release);
            fs::write(&request.output, b"%PDF-1.7\n%%EOF\n").map_err(|error| {
                EngineError::new(ErrorCode::OutputWriteFailed, error.to_string())
            })?;
            Ok(MergeEngineResult {
                page_count: pages,
                evidence: Vec::new(),
            })
        }
    }

    fn unique_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pincerpdf-merge-test-{}-{}-{name}",
            process::id(),
            NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn secret_debug_is_redacted_and_multiline_values_are_rejected() {
        let secret = SecretString::new("correct horse battery staple").expect("valid secret");
        let debug = format!("{secret:?}");
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("correct horse"));
        assert_eq!(SecretString::new("line1\nline2"), Err(SecretStringError));
    }

    #[test]
    fn request_validation_preserves_duplicate_sources_but_rejects_unsafe_output() {
        let source = MergeSource::new("a.pdf");
        let request =
            MergeRequest::new([source.clone(), source], "out.PDF").expect("valid request");
        assert_eq!(request.sources().len(), 2);
        assert_eq!(
            MergeRequest::new([MergeSource::new("a.pdf")], "out.pdf"),
            Err(MergeRequestError::NotEnoughSources)
        );
        assert_eq!(
            MergeRequest::new(
                [MergeSource::new("a.pdf"), MergeSource::new("b.pdf")],
                "a.pdf"
            ),
            Err(MergeRequestError::OutputEqualsSource { source_index: 0 })
        );
    }

    #[test]
    fn service_preserves_page_order_and_duplicates_then_finalizes_atomically() {
        let output = unique_path("ordered.pdf");
        let selection: PageSelection = "3,1,3".parse().expect("valid selection");
        let request = MergeRequest::new(
            [
                MergeSource::new("first.pdf").with_selection(selection),
                MergeSource::new("second.pdf"),
            ],
            &output,
        )
        .expect("valid request");
        let engine = FakeEngine::new();
        let report = MergeService::new(&engine)
            .execute(&request, &MergeExecutionOptions::default())
            .expect("merge succeeds");
        assert_eq!(report.page_count, 6);
        assert_eq!(report.source_count, 2);
        assert!(output.is_file());
        let _ = fs::remove_file(output);
    }

    #[test]
    fn replace_policy_atomically_replaces_an_existing_output() {
        let output = unique_path("replace-existing.pdf");
        fs::write(&output, b"verified-old-output").expect("write original output");
        let mut transaction = OutputTransaction::begin(&output, ExistingOutputPolicy::Replace)
            .expect("begin replacement");
        let temporary = transaction.temporary_path().to_path_buf();
        fs::write(&temporary, b"verified-new-output").expect("write replacement output");

        transaction.commit().expect("replace existing output");

        assert_eq!(
            fs::read(&output).expect("read replaced output"),
            b"verified-new-output"
        );
        assert!(!temporary.exists());
        let _ = fs::remove_file(output);
    }

    #[test]
    fn fail_policy_preserves_a_destination_created_after_planning() {
        let output = unique_path("late-conflict.pdf");
        let mut transaction = OutputTransaction::begin(&output, ExistingOutputPolicy::Fail)
            .expect("begin conflict-free output");
        let temporary = transaction.temporary_path().to_path_buf();
        fs::write(&temporary, b"uncommitted-output").expect("write temporary output");
        fs::write(&output, b"late-existing-output").expect("create racing destination");

        let error = transaction.commit().expect_err("late destination must win");

        assert!(matches!(
            error,
            MergeError::OutputPlan(OutputPlanError::ExistingOutputConflict)
        ));
        assert_eq!(
            fs::read(&output).expect("read preserved destination"),
            b"late-existing-output"
        );
        drop(transaction);
        assert!(!temporary.exists());
        let _ = fs::remove_file(output);
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_replace_preserves_the_old_output_and_cleans_the_temporary_file() {
        let output = unique_path("locked-replacement.pdf");
        fs::write(&output, b"locked-old-output").expect("write original output");
        let mut transaction = OutputTransaction::begin(&output, ExistingOutputPolicy::Replace)
            .expect("begin replacement");
        let temporary = transaction.temporary_path().to_path_buf();
        fs::write(&temporary, b"uncommitted-new-output").expect("write temporary output");
        let locked_output = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&output)
            .expect("lock existing output without delete sharing");

        let error = transaction
            .commit()
            .expect_err("Windows must reject replacement while destination is locked");

        assert!(matches!(error, MergeError::OutputIo(_)));
        assert!(output.exists());
        assert!(temporary.exists());
        drop(locked_output);
        assert_eq!(
            fs::read(&output).expect("read preserved output after releasing lock"),
            b"locked-old-output"
        );
        drop(transaction);
        assert!(!temporary.exists());
        assert_eq!(
            fs::read(&output).expect("read output after cleanup"),
            b"locked-old-output"
        );
        let _ = fs::remove_file(output);
    }

    #[test]
    fn service_rejects_forms_before_engine_merge() {
        let output = unique_path("forms.pdf");
        let request = MergeRequest::new(
            [MergeSource::new("form.pdf"), MergeSource::new("plain.pdf")],
            &output,
        )
        .expect("valid request");
        let error = MergeService::new(&FakeEngine::new())
            .execute(&request, &MergeExecutionOptions::default())
            .expect_err("forms are not verified");
        assert!(matches!(
            error,
            MergeError::FormsUnsupported { source_index: 0 }
        ));
        assert!(!output.exists());
    }
}
