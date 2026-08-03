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
    /// Stable user-facing title used by generated document-level bookmarks.
    pub document_title: String,
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
    /// Explicit bookmark behavior for the generated output.
    pub bookmark_policy: BookmarkPolicy,
    /// Whether a blank page follows every source contributing an odd page count.
    pub add_blank_page_if_odd: bool,
    /// Whether each output page receives its source filename as a footer.
    pub add_filename_footer: bool,
    /// Whether the adapter should prepend a generated table-of-contents page.
    pub toc_policy: MergeTocPolicy,
}

/// Successful adapter result before application-level semantic verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MergeEngineResult {
    /// Page count observed by the adapter after writing.
    pub page_count: u32,
    /// Exact number of verified top-level bookmarks in the generated output.
    pub bookmark_entries: usize,
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

/// Bookmark behavior applied after page assembly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BookmarkPolicy {
    /// Remove every source outline tree and emit no replacement bookmarks.
    #[default]
    Discard,
    /// Create one top-level bookmark for each ordered source entry.
    OneEntryPerDocument,
    /// Rebuild the relevant source outline trees at the output root.
    Retain,
    /// Group every rebuilt source outline tree below one document-level entry.
    RetainAsOneEntryPerDocument,
}

/// Table-of-contents policy applied to a merged output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MergeTocPolicy {
    /// Do not prepend a generated table of contents.
    #[default]
    None,
    /// Prepend one or more pages listing source filenames and first pages.
    FileNames,
}

/// Application-level execution options.
#[derive(Clone, Debug)]
pub struct MergeExecutionOptions {
    /// Existing destination behavior.
    pub output_policy: ExistingOutputPolicy,
    /// Explicit output bookmark behavior.
    pub bookmark_policy: BookmarkPolicy,
    /// Insert a blank page after each odd-page source, including the final source.
    pub add_blank_page_if_odd: bool,
    /// Add the contributing source filename to the bottom of each output page.
    pub add_filename_footer: bool,
    /// Generated table-of-contents policy.
    pub toc_policy: MergeTocPolicy,
    /// Native-process limits and cancellation.
    pub control: ExecutionControl,
}

impl Default for MergeExecutionOptions {
    fn default() -> Self {
        Self {
            output_policy: ExistingOutputPolicy::Fail,
            bookmark_policy: BookmarkPolicy::Discard,
            add_blank_page_if_odd: false,
            add_filename_footer: false,
            toc_policy: MergeTocPolicy::None,
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
    /// Number of verified top-level bookmark entries generated by policy.
    pub bookmark_entries: usize,
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
        /// Zero-based sourcãou¶‰žËkºwµçQÈ ¤¹¥Í}•µÁÑä ¤¤(€€€€€€€€¹Õ¹ÝÉ…Á}½É}•±Í”¡ñðA…Ñ èé¹•Ü ˆ¸ˆ¤¤ì(€€€±•Ð…¹½¹¥…±}Á…É•¹Ð€ô½ÕÑÁÕÑ}Á…É•¹Ð¹…¹½¹¥…±¥é” ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½ÐÉ•Í½±Ù”µ•É”½ÕÑÁÕÐ‘¥É•Ñ½Éäèí•ÉÉ½Éôˆ¤¤(€€€ô¤üì(€€€±•Ð½ÕÑÁÕÑ}¹…µ”€ôÉ•ÅÕ•ÍÐ(€€€€€€€€¹½ÕÑÁÕÐ ¤(€€€€€€€€¹™¥±•}¹…µ” ¤(€€€€€€€€¹½­}½É}•±Í”¡ñð5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼ ‰µ•É”½ÕÑÁÕÐÁ…Ñ ¡…Ì¹¼™¥±”¹…µ”ˆ¹Ñ½}½Ý¹• ¤¤¤üì(€€€±•Ð½ÕÑÁÕÑ}…¹‘¥‘…Ñ”€ô…¹½¹¥…±}Á…É•¹Ð¹©½¥¸¡½ÕÑÁÕÑ}¹…µ”¤ì((€€€™½È€¡Í½ÕÉ•}¥¹‘•à°Í½ÕÉ”¤¥¸É•ÅÕ•ÍÐ¹Í½ÕÉ•Ì ¤¹¥Ñ•È ¤¹•¹Õµ•É…Ñ” ¤ì(€€€€€€€¥˜±•Ð=¬¡…¹½¹¥…±}Í½ÕÉ”¤€ôÍ½ÕÉ”¹Á…Ñ  ¤¹…¹½¹¥…±¥é” ¤(€€€€€€€€€€€€˜˜…¹½¹¥…±}Í½ÕÉ”€ôô½ÕÑÁÕÑ}…¹‘¥‘…Ñ”(€€€€€€€ì(€€€€€€€€€€€É•ÑÕÉ¸ÉÈ¡5•É•ÉÉ½Èèé=ÕÑÁÕÑ±¥…Í•ÍM½ÕÉ”ìÍ½ÕÉ•}¥¹‘•àô¤ì(€€€€€€€ô(€€€ô(€€€=¬  ¤¤)ô()™¸É•Í½±Ù•}Í½ÕÉ•}Á…•Ì (€€€Í½ÕÉ”è€™5•É•M½ÕÉ”°(€€€µ•Ñ…‘…Ñ„è€™A‘™5•Ñ…‘…Ñ„°(¤€´øI•ÍÕ±ÐñY•ŒñA…•9Õµ‰•Èø°I•Í½±Ù•M•±•Ñ¥½¹ÉÉ½Èøì(€€€¥˜±•ÐM½µ”¡Í•±•Ñ¥½¸¤€ôÍ½ÕÉ”¹Í•±•Ñ¥½¸ ¤ì(€€€€€€€Í•±•Ñ¥½¸¹É•Í½±Ù”¡µ•Ñ…‘…Ñ„¹Á…•}½Õ¹Ð¤(€€€ô•±Í”ì(€€€€€€€=¬  Ä¸¸õµ•Ñ…‘…Ñ„¹Á…•}½Õ¹Ð¤(€€€€€€€€€€€€¹µ…À¡ñÁ…•ðA…•9Õµ‰•Èèé¹•Ü¡Á…”¤¹•áÁ•Ð ‰Á…”É…¹”ÍÑ…ÉÑÌ…Ð½¹”ˆ¤¤(€€€€€€€€€€€€¹½±±•Ð ¤¤(€€€ô)ô()ÍÑÉÕÐ=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸ì(€€€Á±…¸è=ÕÑÁÕÑA…Ñ¡A±…¸°(€€€½µµ¥ÑÑ•è‰½½°°)ô()¥µÁ°=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸ì(€€€™¸‰•¥¸¡Á…Ñ è€™A…Ñ °Á½±¥äèá¥ÍÑ¥¹=ÕÑÁÕÑA½±¥ä¤€´øI•ÍÕ±ÐñM•±˜°5•É•ÉÉ½Èøì(€€€€€€€±•Ð™¥¹…±}•á¥ÍÑÌ€ôÁ…Ñ ¹ÑÉå}•á¥ÍÑÌ ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½Ð¥¹ÍÁ•Ð½ÕÑÁÕÐÁ…Ñ èí•ÉÉ½Éôˆ¤¤(€€€€€€€ô¤üì(€€€€€€€±•ÐÑ½­•¸€ô™½Éµ…Ð„ (€€€€€€€€€€€€‰µ•É•}íõ}íôˆ°(€€€€€€€€€€€ÁÉ½•ÍÌèé¥ ¤°(€€€€€€€€€€€9aQ}=AIQ%=9}%¹™•Ñ¡}…‘ Ä°=É‘•É¥¹œèéI•±…á•¤(€€€€€€€€¤ì(€€€€€€€±•ÐÁ±…¸€ô(€€€€€€€€€€€Á±…¹}½ÕÑÁÕÑ}Á…Ñ ¡Á…Ñ °™¥¹…±}•á¥ÍÑÌ°Á½±¥ä°€™Ñ½­•¸¤¹µ…Á}•ÉÈ¡5•É•ÉÉ½Èèé=ÕÑÁÕÑA±…¸¤üì(€€€€€€€¥˜Á±…¸¹Ñ•µÁ½É…Éå}Á…Ñ ¹ÑÉå}•á¥ÍÑÌ ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½Ð¥¹ÍÁ•ÐÑ•µÁ½É…Éä½ÕÑÁÕÐÁ…Ñ èí•ÉÉ½Éôˆ¤¤(€€€€€€€ô¤üì(€€€€€€€€€€€É•ÑÕÉ¸ÉÈ¡5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼ (€€€€€€€€€€€€€€€€‰Ñ•µÁ½É…Éäµ•É”½ÕÑÁÕÐÕ¹•áÁ•Ñ•‘±ä…±É•…‘ä•á¥ÍÑÌˆ¹Ñ½}½Ý¹• ¤°(€€€€€€€€€€€€¤¤ì(€€€€€€€ô(€€€€€€€=¬¡M•±˜ì(€€€€€€€€€€€Á±…¸°(€€€€€€€€€€€½µµ¥ÑÑ•è™…±Í”°(€€€€€€€ô¤(€€€ô((€€€™¸Ñ•µÁ½É…Éå}Á…Ñ  ™Í•±˜¤€´ø€™A…Ñ ì(€€€€€€€€™Í•±˜¹Á±…¸¹Ñ•µÁ½É…Éå}Á…Ñ (€€€ô((€€€™¸½µµ¥Ð ™µÕÐÍ•±˜¤€´øI•ÍÕ±ÐñA…Ñ¡	Õ˜°5•É•ÉÉ½Èøì(€€€€€€€±•ÐÑ•µÁ½É…Éä€ô€™Í•±˜¹Á±…¸¹Ñ•µÁ½É…Éå}Á…Ñ ì(€€€€€€€±•Ð™¥¹…±}Á…Ñ €ô€™Í•±˜¹Á±…¸¹™¥¹…±}Á…Ñ ì(€€€€€€€¥˜€…Ñ•µÁ½É…Éä¹¥Í}™¥±” ¤ì(€€€€€€€€€€€É•ÑÕÉ¸ÉÈ¡5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼ (€€€€€€€€€€€€€€€€‰•¹¥¹”‘¥¹½ÐÁÉ½‘Õ”„É•Õ±…ÈÑ•µÁ½É…ÉäAˆ¹Ñ½}½Ý¹• ¤°(€€€€€€€€€€€€¤¤ì(€€€€€€€ô(€€€€€€€=Á•¹=ÁÑ¥½¹Ìèé¹•Ü ¤(€€€€€€€€€€€€¹ÝÉ¥Ñ”¡ÑÉÕ”¤(€€€€€€€€€€€€¹½Á•¸¡Ñ•µÁ½É…Éä¤(€€€€€€€€€€€€¹…¹‘}Ñ¡•¸¡ñ™¥±•ð™¥±”¹Íå¹}…±° ¤¤(€€€€€€€€€€€€¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éð5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½ÐÍå¹ŒÑ•µÁ½É…ÉäAèí•ÉÉ½Éôˆ¤¤¤üì(€€€€€€€¥˜€…Í•±˜¹Á±…¸¹É•Á±…•}•á¥ÍÑ¥¹œ(€€€€€€€€€€€€˜˜™¥¹…±}Á…Ñ ¹ÑÉå}•á¥ÍÑÌ ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€€€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½ÐÉ•¡•¬™¥¹…°½ÕÑÁÕÐÁ…Ñ èí•ÉÉ½Éôˆ¤¤(€€€€€€€€€€€ô¤ü(€€€€€€€ì(€€€€€€€€€€€É•ÑÕÉ¸ÉÈ¡5•É•ÉÉ½Èèé=ÕÑÁÕÑA±…¸ (€€€€€€€€€€€€€€€=ÕÑÁÕÑA±…¹ÉÉ½Èèéá¥ÍÑ¥¹=ÕÑÁÕÑ½¹™±¥Ð°(€€€€€€€€€€€€¤¤ì(€€€€€€€ô(€€€€€€€™ÌèéÉ•¹…µ”¡Ñ•µÁ½É…Éä°™¥¹…±}Á…Ñ ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½Ð…Ñ½µ¥…±±ä™¥¹…±¥é”Aèí•ÉÉ½Éôˆ¤¤(€€€€€€€ô¤üì(€€€€€€€¥˜±•ÐM½µ”¡Á…É•¹Ð¤€ô™¥¹…±}Á…Ñ (€€€€€€€€€€€€¹Á…É•¹Ð ¤(€€€€€€€€€€€€¹™¥±Ñ•È¡ñÁ…É•¹Ñð€…Á…É•¹Ð¹…Í}½Í}ÍÑÈ ¤¹¥Í}•µÁÑä ¤¤(€€€€€€€ì(€€€€€€€€€€€€m™œ¡Õ¹¥à¥t(€€€€€€€€€€€Íå¹}½ÕÑÁÕÑ}‘¥É•Ñ½Éä¡Á…É•¹Ð¤üì(€€€€€€€€€€€€m™œ¡¹½Ð¡Õ¹¥à¤¥t(€€€€€€€€€€€ì(€€€€€€€€€€€€€€€€¼¼ÍÑèé™Ì…¹¹½Ð½Á•¸„]¥¹‘½ÝÌ‘¥É•Ñ½Éä™½È±ÕÍ¡¥±•	Õ™™•ÉÌ¸Q¡”(€€€€€€€€€€€€€€€€¼¼Ñ•µÁ½É…Éä™¥±”¥ÑÍ•±˜Ý…Ì‘ÕÉ…‰±ä™±ÕÍ¡•‰•™½É”Ñ¡”Í…µ”µÙ½±Õµ”É•¹…µ”¸(€€€€€€€€€€€€€€€±•Ð|€ôÁ…É•¹Ðì(€€€€€€€€€€€ô(€€€€€€€ô(€€€€€€€Í•±˜¹½µµ¥ÑÑ•€ôÑÉÕ”ì(€€€€€€€=¬¡™¥¹…±}Á…Ñ ¹±½¹” ¤¤(€€€ô)ô((m™œ¡Õ¹¥à¥t)™¸Íå¹}½ÕÑÁÕÑ}‘¥É•Ñ½Éä¡Á…É•¹Ðè€™A…Ñ ¤€´øI•ÍÕ±Ðð ¤°5•É•ÉÉ½Èøì(€€€¥±”èé½Á•¸¡Á…É•¹Ð¤(€€€€€€€€¹…¹‘}Ñ¡•¸¡ñ‘¥É•Ñ½Éåð‘¥É•Ñ½Éä¹Íå¹}…±° ¤¤(€€€€€€€€¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éð5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡™½Éµ…Ð„ ‰…¹¹½ÐÍå¹Œ½ÕÑÁÕÐ‘¥É•Ñ½Éäèí•ÉÉ½Éôˆ¤¤¤)ô()¥µÁ°É½À™½È=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸ì(€€€™¸‘É½À ™µÕÐÍ•±˜¤ì(€€€€€€€¥˜€…Í•±˜¹½µµ¥ÑÑ•ì(€€€€€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±” ™Í•±˜¹Á±…¸¹Ñ•µÁ½É…Éå}Á…Ñ ¤ì(€€€€€€€ô(€€€ô)ô((¼¼¼I•ÑÕÉ¹ÌÑ¡”…Á…‰¥±¥ÑäÍ•ÐÉ•ÅÕ¥É•‰ä@Ð¸Ä5•É”¸(mµÕÍÑ}ÕÍ•t)ÁÕˆ™¸µ•É•}½É•}…Á…‰¥±¥Ñ¥•Ì ¤€´ø…Á…‰¥±¥ÑåM•Ðì(€€€Á¥¹•ÉÁ‘™}…ÁÁ±¥…Ñ¥½¸èéÉ•ÅÕ¥É•‘}…Á…‰¥±¥Ñ¥•Ì¡Q½½±-¥¹èé5•É”¤)ô((m™œ¡Ñ•ÍÐ¥t)µ½Ñ•ÍÑÌì(€€€ÕÍ”ÍÕÁ•Èèè¨ì(€€€ÕÍ”Á¥¹•ÉÁ‘™}•¹¥¹•}…Á¤èéíA‘™…Á…‰¥±¥Ñä°A‘™5•Ñ…‘…Ñ…ôì(€€€ÕÍ”ÍÑèéÍå¹Œèé…Ñ½µ¥ŒèéíÑ½µ¥TÌÈ°=É‘•É¥¹œ…ÌÑ½µ¥=É‘•É¥¹ôì((€€€€m™œ¡Ý¥¹‘½ÝÌ¥t(€€€ÕÍ”ÍÑèé½ÌèéÝ¥¹‘½ÝÌèé™Ìèé=Á•¹=ÁÑ¥½¹ÍáÐì((€€€ÍÑÉÕÐ…­•¹¥¹”ì(€€€€€€€½ÕÑÁÕÑ}Á…•ÌèÑ½µ¥TÌÈ°(€€€€€€€½ÕÑÁÕÑ}¡…Í}‰½½­µ…É­ÌèÑ½µ¥	½½°°(€€€ô((€€€¥µÁ°…­•¹¥¹”ì(€€€€€€€™¸¹•Ü ¤€´øM•±˜ì(€€€€€€€€€€€M•±˜ì(€€€€€€€€€€€€€€€½ÕÑÁÕÑ}Á…•ÌèÑ½µ¥TÌÈèé¹•Ü À¤°(€€€€€€€€€€€€€€€½ÕÑÁÕÑ}¡…Í}‰½½­µ…É­ÌèÑ½µ¥	½½°èé¹•Ü¡™…±Í”¤°(€€€€€€€€€€€ô(€€€€€€€ô(€€€ô((€€€¥µÁ°A‘™¹¥¹•A½ÉÐ™½È…­•¹¥¹”ì(€€€€€€€™¸¥‘•¹Ñ¥Ñä ™Í•±˜¤€´ø¹¥¹•%‘•¹Ñ¥Ñäì(€€€€€€€€€€€¹¥¹•%‘•¹Ñ¥Ñäì(€€€€€€€€€€€€€€€¥è€‰™…­”ˆ¹Ñ½}½Ý¹• ¤°(€€€€€€€€€€€€€€€Ù•ÉÍ¥½¸è€ˆÄˆ¹Ñ½}½Ý¹• ¤°(€€€€€€€€€€€ô(€€€€€€€ô((€€€€€€€™¸…Á…‰¥±¥Ñ¥•Ì ™Í•±˜¤€´ø…Á…‰¥±¥ÑåM•Ðì(€€€€€€€€€€€…Á…‰¥±¥ÑåM•Ðèé™É½µ}…Á…‰¥±¥Ñ¥•Ì¡mA‘™…Á…‰¥±¥Ñäèé%¹ÍÁ•Ð°A‘™…Á…‰¥±¥Ñäèé5•É•t¤(€€€€€€€ô((€€€€€€€™¸¥¹ÍÁ•Ð (€€€€€€€€€€€€™Í•±˜°(€€€€€€€€€€€Í½ÕÉ”è€™A…Ñ °(€€€€€€€€€€€}½ÁÑ¥½¹Ìè%¹ÍÁ•Ñ=ÁÑ¥½¹Ìð|ø°(€€€€€€€€¤€´øI•ÍÕ±ÐñA‘™5•Ñ…‘…Ñ„°¹¥¹•ÉÉ½Èøì(€€€€€€€€€€€±•Ð¥Í}½ÕÑÁÕÐ€ôÍ½ÕÉ”(€€€€€€€€€€€€€€€€¹™¥±•}¹…µ” ¤(€€€€€€€€€€€€€€€€¹¥Í}Í½µ•}…¹¡ñ¹…µ•ð¹…µ”¹Ñ½}ÍÑÉ¥¹}±½ÍÍä ¤¹½¹Ñ…¥¹Ì ‰Á¥¹•ÉÁ‘˜µµ•É•|ˆ¤¤ì(€€€€€€€€€€€=¬¡A‘™5•Ñ…‘…Ñ„ì(€€€€€€€€€€€€€€€Á…•}½Õ¹Ðè¥˜¥Í}½ÕÑÁÕÐì(€€€€€€€€€€€€€€€€€€€Í•±˜¹½ÕÑÁÕÑ}Á…•Ì¹±½…¡Ñ½µ¥=É‘•É¥¹œèéÅÕ¥É”¤(€€€€€€€€€€€€€€€ô•±Í”ì(€€€€€€€€€€€€€€€€€€€€Ì(€€€€€€€€€€€€€€€ô°(€€€€€€€€€€€€€€€•¹ÉåÁÑ•è™…±Í”°(€€€€€€€€€€€€€€€Á‘™}Ù•ÉÍ¥½¸èM½µ” ˆÄ¸Üˆ¹Ñ½}½Ý¹• ¤¤°(€€€€€€€€€€€€€€€¡…Í}‰½½­µ…É­Ìè¥˜¥Í}½ÕÑÁÕÐì(€€€€€€€€€€€€€€€€€€€Í•±˜¹½ÕÑÁÕÑ}¡…Í}‰½½­µ…É­Ì¹±½…¡Ñ½µ¥=É‘•É¥¹œèéÅÕ¥É”¤(€€€€€€€€€€€€€€€ô•±Í”ì(€€€€€€€€€€€€€€€€€€€Í½ÕÉ”¹Ñ½}ÍÑÉ¥¹}±½ÍÍä ¤¹½¹Ñ…¥¹Ì ‰‰½½­µ…É­Ìˆ¤(€€€€€€€€€€€€€€€ô°(€€€€€€€€€€€€€€€¡…Í}™½ÉµÌèÍ½ÕÉ”¹Ñ½}ÍÑÉ¥¹}±½ÍÍä ¤¹½¹Ñ…¥¹Ì ‰™½É´ˆ¤°(€€€€€€€€€€€ô¤(€€€€€€€ô(€€€ô((€€€¥µÁ°5•É•¹¥¹•A½ÉÐ™½È…­•¹¥¹”ì(€€€€€€€™¸µ•É” (€€€€€€€€€€€€™Í•±˜°(€€€€€€€€€€€É•ÅÕ•ÍÐè€™5•É•¹¥¹•I•ÅÕ•ÍÐ°(€€€€€€€€€€€}½¹ÑÉ½°è€™á•ÕÑ¥½¹½¹ÑÉ½°°(€€€€€€€€¤€´øI•ÍÕ±Ðñ5•É•¹¥¹•I•ÍÕ±Ð°¹¥¹•ÉÉ½Èøì(€€€€€€€€€€€±•ÐÁ…•Ì€ôÉ•ÅÕ•ÍÐ(€€€€€€€€€€€€€€€€¹¥¹ÁÕÑÌ(€€€€€€€€€€€€€€€€¹¥Ñ•È ¤(€€€€€€€€€€€€€€€€¹µ…À¡ñ¥¹ÁÕÑð¥¹ÁÕÐ¹Á…•Ì¹±•¸ ¤¤(€€€€€€€€€€€€€€€€¹ÍÕ´èèñÕÍ¥é”ø ¤ì(€€€€€€€€€€€±•ÐÁ…•Ì€ôÁ…•Ì(€€€€€€€€€€€€€€€€¬ÕÍ¥é”èé™É½´¡É•ÅÕ•ÍÐ¹…‘‘}‰±…¹­}Á…•}¥™}½‘¤(€€€€€€€€€€€€€€€€€€€€¨É•ÅÕ•ÍÐ(€€€€€€€€€€€€€€€€€€€€€€€€¹¥¹ÁÕÑÌ(€€€€€€€€€€€€€€€€€€€€€€€€¹¥Ñ•È ¤(€€€€€€€€€€€€€€€€€€€€€€€€¹™¥±Ñ•È¡ñ¥¹ÁÕÑð¥¹ÁÕÐ¹Á…•Ì¹±•¸ ¤€”€È€ôô€Ä¤(€€€€€€€€€€€€€€€€€€€€€€€€¹½Õ¹Ð ¤ì(€€€€€€€€€€€±•ÐÁ…•Ì€ôÔÌÈèéÑÉå}™É½´¡Á…•Ì¤¹•áÁ•Ð ‰Ñ•ÍÐÁ…”½Õ¹Ð™¥ÑÌÔÌÈˆ¤ì(€€€€€€€€€€€Í•±˜¹½ÕÑÁÕÑ}Á…•Ì¹ÍÑ½É”¡Á…•Ì°Ñ½µ¥=É‘•É¥¹œèéI•±•…Í”¤ì(€€€€€€€€€€€±•Ð‰½½­µ…É­}•¹ÑÉ¥•Ì€ôµ…Ñ É•ÅÕ•ÍÐ¹‰½½­µ…É­}Á½±¥äì(€€€€€€€€€€€€€€€	½½­µ…É­A½±¥äèé¥Í…É€ôø€À°(€€€€€€€€€€€€€€€	½½­µ…É­A½±¥äèé=¹•¹ÑÉåA•É½Õµ•¹Ð(€€€€€€€€€€€€€€€ð	½½­µ…É­A½±¥äèéI•Ñ…¥¹Í=¹•¹ÑÉåA•É½Õµ•¹Ð€ôøÉ•ÅÕ•ÍÐ¹¥¹ÁÕÑÌ¹±•¸ ¤°(€€€€€€€€€€€€€€€	½½­µ…É­A½±¥äèéI•Ñ…¥¸€ôø€Ä°(€€€€€€€€€€€ôì(€€€€€€€€€€€Í•±˜¹½ÕÑÁÕÑ}¡…Í}‰½½­µ…É­Ì(€€€€€€€€€€€€€€€€¹ÍÑ½É”¡‰½½­µ…É­}•¹ÑÉ¥•Ì€ø€À°Ñ½µ¥=É‘•É¥¹œèéI•±•…Í”¤ì(€€€€€€€€€€€™ÌèéÝÉ¥Ñ” ™É•ÅÕ•ÍÐ¹½ÕÑÁÕÐ°ˆˆ•A´Ä¸Ýq¸”•=q¸ˆ¤¹µ…Á}•ÉÈ¡ñ•ÉÉ½Éðì(€€€€€€€€€€€€€€€¹¥¹•ÉÉ½Èèé¹•Ü¡ÉÉ½É½‘”èé=ÕÑÁÕÑ]É¥Ñ•…¥±•°•ÉÉ½È¹Ñ½}ÍÑÉ¥¹œ ¤¤(€€€€€€€€€€€ô¤üì(€€€€€€€€€€€=¬¡5•É•¹¥¹•I•ÍÕ±Ðì(€€€€€€€€€€€€€€€Á…•}½Õ¹ÐèÁ…•Ì°(€€€€€€€€€€€€€€€‰½½­µ…É­}•¹ÑÉ¥•Ì°(€€€€€€€€€€€€€€€•Ù¥‘•¹”èY•Œèé¹•Ü ¤°(€€€€€€€€€€€ô¤(€€€€€€€ô(€€€ô((€€€™¸Õ¹¥ÅÕ•}Á…Ñ ¡¹…µ”è€™ÍÑÈ¤€´øA…Ñ¡	Õ˜ì(€€€€€€€ÍÑèé•¹ØèéÑ•µÁ}‘¥È ¤¹©½¥¸¡™½Éµ…Ð„ (€€€€€€€€€€€€‰Á¥¹•ÉÁ‘˜µµ•É”µÑ•ÍÐµíôµíôµí¹…µ•ôˆ°(€€€€€€€€€€€ÁÉ½•ÍÌèé¥ ¤°(€€€€€€€€€€€9aQ}=AIQ%=9}%¹™•Ñ¡}…‘ Ä°=É‘•É¥¹œèéI•±…á•¤(€€€€€€€€¤¤(€€€ô((€€€€mÑ•ÍÑt(€€€™¸Í•É•Ñ}‘•‰Õ}¥Í}É•‘…Ñ•‘}…¹‘}µÕ±Ñ¥±¥¹•}Ù…±Õ•Í}…É•}É•©•Ñ• ¤ì(€€€€€€€±•ÐÍ•É•Ð€ôM•É•ÑMÑÉ¥¹œèé¹•Ü ‰½ÉÉ•Ð¡½ÉÍ”‰…ÑÑ•ÉäÍÑ…Á±”ˆ¤¹•áÁ•Ð ‰Ù…±¥Í•É•Ðˆ¤ì(€€€€€€€±•Ð‘•‰Õœ€ô™½Éµ…Ð„ ‰íÍ•É•Ðèýôˆ¤ì(€€€€€€€…ÍÍ•ÉÐ„¡‘•‰Õœ¹½¹Ñ…¥¹Ì ‰IQˆ¤¤ì(€€€€€€€…ÍÍ•ÉÐ„ …‘•‰Õœ¹½¹Ñ…¥¹Ì ‰½ÉÉ•Ð¡½ÉÍ”ˆ¤¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡M•É•ÑMÑÉ¥¹œèé¹•Ü ‰±¥¹”Åq¹±¥¹”Èˆ¤°ÉÈ¡M•É•ÑMÑÉ¥¹ÉÉ½È¤¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸É•ÅÕ•ÍÑ}Ù…±¥‘…Ñ¥½¹}ÁÉ•Í•ÉÙ•Í}‘ÕÁ±¥…Ñ•}Í½ÕÉ•Í}‰ÕÑ}É•©•ÑÍ}Õ¹Í…™•}½ÕÑÁÕÐ ¤ì(€€€€€€€±•ÐÍ½ÕÉ”€ô5•É•M½ÕÉ”èé¹•Ü ‰„¹Á‘˜ˆ¤ì(€€€€€€€±•ÐÉ•ÅÕ•ÍÐ€ô(€€€€€€€€€€€5•É•I•ÅÕ•ÍÐèé¹•Ü¡mÍ½ÕÉ”¹±½¹” ¤°Í½ÕÉ•t°€‰½ÕÐ¹Aˆ¤¹•áÁ•Ð ‰Ù…±¥É•ÅÕ•ÍÐˆ¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•ÅÕ•ÍÐ¹Í½ÕÉ•Ì ¤¹±•¸ ¤°€È¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€5•É•I•ÅÕ•ÍÐèé¹•Ü¡m5•É•M½ÕÉ”èé¹•Ü ‰„¹Á‘˜ˆ¥t°€‰½ÕÐ¹Á‘˜ˆ¤°(€€€€€€€€€€€ÉÈ¡5•É•I•ÅÕ•ÍÑÉÉ½Èèé9½Ñ¹½Õ¡M½ÕÉ•Ì¤(€€€€€€€€¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€5•É•I•ÅÕ•ÍÐèé¹•Ü (€€€€€€€€€€€€€€€m5•É•M½ÕÉ”èé¹•Ü ‰„¹Á‘˜ˆ¤°5•É•M½ÕÉ”èé¹•Ü ‰ˆ¹Á‘˜ˆ¥t°(€€€€€€€€€€€€€€€€‰„¹Á‘˜ˆ(€€€€€€€€€€€€¤°(€€€€€€€€€€€ÉÈ¡5•É•I•ÅÕ•ÍÑÉÉ½Èèé=ÕÑÁÕÑÅÕ…±ÍM½ÕÉ”ìÍ½ÕÉ•}¥¹‘•àè€Àô¤(€€€€€€€€¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸Í•ÉÙ¥•}ÁÉ•Í•ÉÙ•Í}Á…•}½É‘•É}…¹‘}‘ÕÁ±¥…Ñ•Í}Ñ¡•¹}™¥¹…±¥é•Í}…Ñ½µ¥…±±ä ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰½É‘•É•¹Á‘˜ˆ¤ì(€€€€€€€±•ÐÍ•±•Ñ¥½¸èA…•M•±•Ñ¥½¸€ô€ˆÌ°Ä°Ìˆ¹Á…ÉÍ” ¤¹•áÁ•Ð ‰Ù…±¥Í•±•Ñ¥½¸ˆ¤ì(€€€€€€€±•ÐÉ•ÅÕ•ÍÐ€ô5•É•I•ÅÕ•ÍÐèé¹•Ü (€€€€€€€€€€€l(€€€€€€€€€€€€€€€5•É•M½ÕÉ”èé¹•Ü ‰™¥ÉÍÐ¹Á‘˜ˆ¤¹Ý¥Ñ¡}Í•±•Ñ¥½¸¡Í•±•Ñ¥½¸¤°(€€€€€€€€€€€€€€€5•É•M½ÕÉ”èé¹•Ü ‰Í•½¹¹Á‘˜ˆ¤°(€€€€€€€€€€€t°(€€€€€€€€€€€€™½ÕÑÁÕÐ°(€€€€€€€€¤(€€€€€€€€¹•áÁ•Ð ‰Ù…±¥É•ÅÕ•ÍÐˆ¤ì(€€€€€€€±•Ð•¹¥¹”€ô…­•¹¥¹”èé¹•Ü ¤ì(€€€€€€€±•ÐÉ•Á½ÉÐ€ô5•É•M•ÉÙ¥”èé¹•Ü ™•¹¥¹”¤(€€€€€€€€€€€€¹•á•ÕÑ” ™É•ÅÕ•ÍÐ°€™5•É•á•ÕÑ¥½¹=ÁÑ¥½¹Ìèé‘•™…Õ±Ð ¤¤(€€€€€€€€€€€€¹•áÁ•Ð ‰µ•É”ÍÕ••‘Ìˆ¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•Á½ÉÐ¹Á…•}½Õ¹Ð°€Ø¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•Á½ÉÐ¹Í½ÕÉ•}½Õ¹Ð°€È¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•Á½ÉÐ¹‰½½­µ…É­}•¹ÑÉ¥•Ì°€À¤ì(€€€€€€€…ÍÍ•ÉÐ„¡½ÕÑÁÕÐ¹¥Í}™¥±” ¤¤ì(€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±”¡½ÕÑÁÕÐ¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸½¹•}•¹ÑÉå}Á½±¥å}É•ÅÕ¥É•Í}…¹‘}É•Á½ÉÑÍ}½¹•}‰½½­µ…É­}Á•É}½É‘•É•‘}Í½ÕÉ” ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰½¹”µ•¹ÑÉäµÁ•Èµ‘½Õµ•¹Ð¹Á‘˜ˆ¤ì(€€€€€€€±•ÐÉ•ÅÕ•ÍÐ€ô5•É•I•ÅÕ•ÍÐèé¹•Ü (€€€€€€€€€€€l(€€€€€€€€€€€€€€€5•É•M½ÕÉ”èé¹•Ü ‰™½±‘•È½™¥ÉÍÐ¹Á‘˜ˆ¤°(€€€€€€€€€€€€€€€5•É•M½ÕÉ”èé¹•Ü ‰™½±‘•È½Í•½¹¹Á‘˜ˆ¤°(€€€€€€€€€€€t°(€€€€€€€€€€€€™½ÕÑÁÕÐ°(€€€€€€€€¤(€€€€€€€€¹•áÁ•Ð ‰Ù…±¥É•ÅÕ•ÍÐˆ¤ì(€€€€€€€±•Ð½ÁÑ¥½¹Ì€ô5•É•á•ÕÑ¥½¹=ÁÑ¥½¹Ìì(€€€€€€€€€€€‰½½­µ…É­}Á½±¥äè	½½­µ…É­A½±¥äèé=¹•¹ÑÉåA•É½Õµ•¹Ð°(€€€€€€€€€€€€¸¹5•É•á•ÕÑ¥½¹=ÁÑ¥½¹Ìèé‘•™…Õ±Ð ¤(€€€€€€€ôì((€€€€€€€±•ÐÉ•Á½ÉÐ€ô5•É•M•ÉÙ¥”èé¹•Ü ™…­•¹¥¹”èé¹•Ü ¤¤(€€€€€€€€€€€€¹•á•ÕÑ” ™É•ÅÕ•ÍÐ°€™½ÁÑ¥½¹Ì¤(€€€€€€€€€€€€¹•áÁ•Ð ‰‰½½­µ…É¬Á½±¥äÍÕ••‘Ìˆ¤ì((€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•Á½ÉÐ¹‰½½­µ…É­}•¹ÑÉ¥•Ì°€È¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„¡É•Á½ÉÐ¹‰½½­µ…É­}Í½ÕÉ•Í}‘¥Í…É‘•°€À¤ì(€€€€€€€…ÍÍ•ÉÐ„¡½ÕÑÁÕÐ¹¥Í}™¥±” ¤¤ì(€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±”¡½ÕÑÁÕÐ¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸É•Á±…•}Á½±¥å}…Ñ½µ¥…±±å}É•Á±…•Í}…¹}•á¥ÍÑ¥¹}½ÕÑÁÕÐ ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰É•Á±…”µ•á¥ÍÑ¥¹œ¹Á‘˜ˆ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™½ÕÑÁÕÐ°ˆ‰Ù•É¥™¥•µ½±µ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰ÝÉ¥Ñ”½É¥¥¹…°½ÕÑÁÕÐˆ¤ì(€€€€€€€±•ÐµÕÐÑÉ…¹Í…Ñ¥½¸€ô=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸èé‰•¥¸ ™½ÕÑÁÕÐ°á¥ÍÑ¥¹=ÕÑÁÕÑA½±¥äèéI•Á±…”¤(€€€€€€€€€€€€¹•áÁ•Ð ‰‰•¥¸É•Á±…•µ•¹Ðˆ¤ì(€€€€€€€±•ÐÑ•µÁ½É…Éä€ôÑÉ…¹Í…Ñ¥½¸¹Ñ•µÁ½É…Éå}Á…Ñ  ¤¹Ñ½}Á…Ñ¡}‰Õ˜ ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™Ñ•µÁ½É…Éä°ˆ‰Ù•É¥™¥•µ¹•Üµ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰ÝÉ¥Ñ”É•Á±…•µ•¹Ð½ÕÑÁÕÐˆ¤ì((€€€€€€€ÑÉ…¹Í…Ñ¥½¸¹½µµ¥Ð ¤¹•áÁ•Ð ‰É•Á±…”•á¥ÍÑ¥¹œ½ÕÑÁÕÐˆ¤ì((€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€™ÌèéÉ•… ™½ÕÑÁÕÐ¤¹•áÁ•Ð ‰É•…É•Á±…•½ÕÑÁÕÐˆ¤°(€€€€€€€€€€€ˆ‰Ù•É¥™¥•µ¹•Üµ½ÕÑÁÕÐˆ(€€€€€€€€¤ì(€€€€€€€…ÍÍ•ÉÐ„ …Ñ•µÁ½É…Éä¹•á¥ÍÑÌ ¤¤ì(€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±”¡½ÕÑÁÕÐ¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸™…¥±}Á½±¥å}ÁÉ•Í•ÉÙ•Í}…}‘•ÍÑ¥¹…Ñ¥½¹}É•…Ñ•‘}…™Ñ•É}Á±…¹¹¥¹œ ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰±…Ñ”µ½¹™±¥Ð¹Á‘˜ˆ¤ì(€€€€€€€±•ÐµÕÐÑÉ…¹Í…Ñ¥½¸€ô=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸èé‰•¥¸ ™½ÕÑÁÕÐ°á¥ÍÑ¥¹=ÕÑÁÕÑA½±¥äèé…¥°¤(€€€€€€€€€€€€¹•áÁ•Ð ‰‰•¥¸½¹™±¥Ðµ™É•”½ÕÑÁÕÐˆ¤ì(€€€€€€€±•ÐÑ•µÁ½É…Éä€ôÑÉ…¹Í…Ñ¥½¸¹Ñ•µÁ½É…Éå}Á…Ñ  ¤¹Ñ½}Á…Ñ¡}‰Õ˜ ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™Ñ•µÁ½É…Éä°ˆ‰Õ¹½µµ¥ÑÑ•µ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰ÝÉ¥Ñ”Ñ•µÁ½É…Éä½ÕÑÁÕÐˆ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™½ÕÑÁÕÐ°ˆ‰±…Ñ”µ•á¥ÍÑ¥¹œµ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰É•…Ñ”É…¥¹œ‘•ÍÑ¥¹…Ñ¥½¸ˆ¤ì((€€€€€€€±•Ð•ÉÉ½È€ôÑÉ…¹Í…Ñ¥½¸¹½µµ¥Ð ¤¹•áÁ•Ñ}•ÉÈ ‰±…Ñ”‘•ÍÑ¥¹…Ñ¥½¸µÕÍÐÝ¥¸ˆ¤ì((€€€€€€€…ÍÍ•ÉÐ„¡µ…Ñ¡•Ì„ (€€€€€€€€€€€•ÉÉ½È°(€€€€€€€€€€€5•É•ÉÉ½Èèé=ÕÑÁÕÑA±…¸¡=ÕÑÁÕÑA±…¹ÉÉ½Èèéá¥ÍÑ¥¹=ÕÑÁÕÑ½¹™±¥Ð¤(€€€€€€€€¤¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€™ÌèéÉ•… ™½ÕÑÁÕÐ¤¹•áÁ•Ð ‰É•…ÁÉ•Í•ÉÙ•‘•ÍÑ¥¹…Ñ¥½¸ˆ¤°(€€€€€€€€€€€ˆ‰±…Ñ”µ•á¥ÍÑ¥¹œµ½ÕÑÁÕÐˆ(€€€€€€€€¤ì(€€€€€€€‘É½À¡ÑÉ…¹Í…Ñ¥½¸¤ì(€€€€€€€…ÍÍ•ÉÐ„ …Ñ•µÁ½É…Éä¹•á¥ÍÑÌ ¤¤ì(€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±”¡½ÕÑÁÕÐ¤ì(€€€ô((€€€€m™œ¡Ý¥¹‘½ÝÌ¥t(€€€€mÑ•ÍÑt(€€€™¸™…¥±•‘}Ý¥¹‘½ÝÍ}É•Á±…•}ÁÉ•Í•ÉÙ•Í}Ñ¡•}½±‘}½ÕÑÁÕÑ}…¹‘}±•…¹Í}Ñ¡•}Ñ•µÁ½É…Éå}™¥±” ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰±½­•µÉ•Á±…•µ•¹Ð¹Á‘˜ˆ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™½ÕÑÁÕÐ°ˆ‰±½­•µ½±µ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰ÝÉ¥Ñ”½É¥¥¹…°½ÕÑÁÕÐˆ¤ì(€€€€€€€±•ÐµÕÐÑÉ…¹Í…Ñ¥½¸€ô=ÕÑÁÕÑQÉ…¹Í…Ñ¥½¸èé‰•¥¸ ™½ÕÑÁÕÐ°á¥ÍÑ¥¹=ÕÑÁÕÑA½±¥äèéI•Á±…”¤(€€€€€€€€€€€€¹•áÁ•Ð ‰‰•¥¸É•Á±…•µ•¹Ðˆ¤ì(€€€€€€€±•ÐÑ•µÁ½É…Éä€ôÑÉ…¹Í…Ñ¥½¸¹Ñ•µÁ½É…Éå}Á…Ñ  ¤¹Ñ½}Á…Ñ¡}‰Õ˜ ¤ì(€€€€€€€™ÌèéÝÉ¥Ñ” ™Ñ•µÁ½É…Éä°ˆ‰Õ¹½µµ¥ÑÑ•µ¹•Üµ½ÕÑÁÕÐˆ¤¹•áÁ•Ð ‰ÝÉ¥Ñ”Ñ•µÁ½É…Éä½ÕÑÁÕÐˆ¤ì(€€€€€€€±•Ð±½­•‘}½ÕÑÁÕÐ€ô=Á•¹=ÁÑ¥½¹Ìèé¹•Ü ¤(€€€€€€€€€€€€¹É•…¡ÑÉÕ”¤(€€€€€€€€€€€€¹ÝÉ¥Ñ”¡ÑÉÕ”¤(€€€€€€€€€€€€¹Í¡…É•}µ½‘” À¤(€€€€€€€€€€€€¹½Á•¸ ™½ÕÑÁÕÐ¤(€€€€€€€€€€€€¹•áÁ•Ð ‰±½¬•á¥ÍÑ¥¹œ½ÕÑÁÕÐÝ¥Ñ¡½ÕÐ‘•±•Ñ”Í¡…É¥¹œˆ¤ì((€€€€€€€±•Ð•ÉÉ½È€ôÑÉ…¹Í…Ñ¥½¸(€€€€€€€€€€€€¹½µµ¥Ð ¤(€€€€€€€€€€€€¹•áÁ•Ñ}•ÉÈ ‰]¥¹‘½ÝÌµÕÍÐÉ•©•ÐÉ•Á±…•µ•¹ÐÝ¡¥±”‘•ÍÑ¥¹…Ñ¥½¸¥Ì±½­•ˆ¤ì((€€€€€€€…ÍÍ•ÉÐ„¡µ…Ñ¡•Ì„¡•ÉÉ½È°5•É•ÉÉ½Èèé=ÕÑÁÕÑ%¼¡|¤¤¤ì(€€€€€€€…ÍÍ•ÉÐ„¡½ÕÑÁÕÐ¹•á¥ÍÑÌ ¤¤ì(€€€€€€€…ÍÍ•ÉÐ„¡Ñ•µÁ½É…Éä¹•á¥ÍÑÌ ¤¤ì(€€€€€€€‘É½À¡±½­•‘}½ÕÑÁÕÐ¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€™ÌèéÉ•… ™½ÕÑÁÕÐ¤¹•áÁ•Ð ‰É•…ÁÉ•Í•ÉÙ•½ÕÑÁÕÐ…™Ñ•ÈÉ•±•…Í¥¹œ±½¬ˆ¤°(€€€€€€€€€€€ˆ‰±½­•µ½±µ½ÕÑÁÕÐˆ(€€€€€€€€¤ì(€€€€€€€‘É½À¡ÑÉ…¹Í…Ñ¥½¸¤ì(€€€€€€€…ÍÍ•ÉÐ„ …Ñ•µÁ½É…Éä¹•á¥ÍÑÌ ¤¤ì(€€€€€€€…ÍÍ•ÉÑ}•Ä„ (€€€€€€€€€€€™ÌèéÉ•… ™½ÕÑÁÕÐ¤¹•áÁ•Ð ‰É•…½ÕÑÁÕÐ…™Ñ•È±•…¹ÕÀˆ¤°(€€€€€€€€€€€ˆ‰±½­•µ½±µ½ÕÑÁÕÐˆ(€€€€€€€€¤ì(€€€€€€€±•Ð|€ô™ÌèéÉ•µ½Ù•}™¥±”¡½ÕÑÁÕÐ¤ì(€€€ô((€€€€mÑ•ÍÑt(€€€™¸Í•ÉÙ¥•}É•©•ÑÍ}™½ÉµÍ}‰•™½É•}•¹¥¹•}µ•É” ¤ì(€€€€€€€±•Ð½ÕÑÁÕÐ€ôÕ¹¥ÅÕ•}Á…Ñ  ‰™½ÉµÌ¹Á‘˜ˆ¤ì(€€€€€€€±•ÐÉ•ÅÕ•ÍÐ€ô5•É•I•ÅÕ•ÍÐèé¹•Ü (€€€€€€€€€€€m5•É•M½ÕÉ”èé¹•Ü ‰™½É´¹Á‘˜ˆ¤°5•É•M½ÕÉ”èé¹•Ü ‰Á±…¥¸¹Á‘˜ˆ¥t°(€€€€€€€€€€€€™½ÕÑÁÕÐ°(€€€€€€€€¤(€€€€€€€€¹•áÁ•Ð ‰Ù…±¥É•ÅÕ•ÍÐˆ¤ì(€€€€€€€±•Ð•ÉÉ½È€ô5•É•M•ÉÙ¥”èé¹•Ü ™…­•¹¥¹”èé¹•Ü ¤¤(€€€€€€€€€€€€¹•á•ÕÑ” ™É•ÅÕ•ÍÐ°€™5•É•á•ÕÑ¥½¹=ÁÑ¥½¹Ìèé‘•™…Õ±Ð ¤¤(€€€€€€€€€€€€¹•áÁ•Ñ}•ÉÈ ‰™½ÉµÌ…É”¹½ÐÙ•É¥™¥•ˆ¤ì(€€€€€€€…ÍÍ•ÉÐ„¡µ…Ñ¡•Ì„ (€€€€€€€€€€€•ÉÉ½È°(€€€€€€€€€€€5•É•ÉÉ½Èèé½ÉµÍU¹ÍÕÁÁ½ÉÑ•ìÍ½ÕÉ•}¥¹‘•àè€Àô(€€€€€€€€¤¤ì(€€€€€€€…ÍÍ•ÉÐ„ …½ÕÑÁÕÐ¹•á¥ÍÑÌ ¤¤ì(€€€ô)ô