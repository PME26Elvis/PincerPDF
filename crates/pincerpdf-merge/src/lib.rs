≠rá^—f•ñÿ¶{^¨y 'v√Æ∂õ≠#![forbid(unsafe_code)]
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
        /// Resolutﬂæ<∂âûÀk∫wµÁ`§π•Õ}ïµ¡—‰†§§(ÄÄÄÄÄÄÄÄπ’π›…Ö¡}Ω…}ï±Õî°ÒÅAÖ—†ËÈπï‹†à∏à§§Ï(ÄÄÄÅ±ï–ÅçÖπΩπ•çÖ±}¡Ö…ïπ–ÄÙÅΩ’—¡’—}¡Ö…ïπ–πçÖπΩπ•çÖ±•Èî†§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–Å…ïÕΩ±ŸîÅµï…ùîÅΩ’—¡’–Åë•…ïç—Ω…‰ËÅÌï……Ω…Ùà§§(ÄÄÄÅÙ§¸Ï(ÄÄÄÅ±ï–ÅΩ’—¡’—}πÖµîÄÙÅ…ï≈’ïÕ–(ÄÄÄÄÄÄÄÄπΩ’—¡’–†§(ÄÄÄÄÄÄÄÄπô•±ï}πÖµî†§(ÄÄÄÄÄÄÄÄπΩ≠}Ω…}ï±Õî°ÒÅ5ï…ùï……Ω»ËÈ=’—¡’—%º†âµï…ùîÅΩ’—¡’–Å¡Ö—†Å°ÖÃÅπºÅô•±îÅπÖµîàπ—Ω}Ω›πïê†§§§¸Ï(ÄÄÄÅ±ï–ÅΩ’—¡’—}çÖπë•ëÖ—îÄÙÅçÖπΩπ•çÖ±}¡Ö…ïπ–π©Ω•∏°Ω’—¡’—}πÖµî§Ï((ÄÄÄÅôΩ»Ä°ÕΩ’…çï}•πëï‡∞ÅÕΩ’…çî§Å•∏Å…ï≈’ïÕ–πÕΩ’…çïÃ†§π•—ï»†§πïπ’µï…Ö—î†§ÅÏ(ÄÄÄÄÄÄÄÅ•òÅ±ï–Å=¨°çÖπΩπ•çÖ±}ÕΩ’…çî§ÄÙÅÕΩ’…çîπ¡Ö—††§πçÖπΩπ•çÖ±•Èî†§(ÄÄÄÄÄÄÄÄÄÄÄÄòòÅçÖπΩπ•çÖ±}ÕΩ’…çîÄÙÙÅΩ’—¡’—}çÖπë•ëÖ—î(ÄÄÄÄÄÄÄÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ…ï—’…∏Å…»°5ï…ùï……Ω»ËÈ=’—¡’—±•ÖÕïÕMΩ’…çîÅÏÅÕΩ’…çï}•πëï‡ÅÙ§Ï(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÅÙ(ÄÄÄÅ=¨††§§)Ù()ô∏Å…ïÕΩ±Ÿï}ÕΩ’…çï}¡ÖùïÃ†(ÄÄÄÅÕΩ’…çîËÄô5ï…ùïMΩ’…çî∞(ÄÄÄÅµï—ÖëÖ—ÑËÄôAëô5ï—ÖëÖ—Ñ∞(§Ä¥¯ÅIïÕ’±–ÒYïåÒAÖùï9’µâï»¯∞ÅIïÕΩ±ŸïMï±ïç—•Ωπ……Ω»¯ÅÏ(ÄÄÄÅ•òÅ±ï–ÅMΩµî°Õï±ïç—•Ω∏§ÄÙÅÕΩ’…çîπÕï±ïç—•Ω∏†§ÅÏ(ÄÄÄÄÄÄÄÅÕï±ïç—•Ω∏π…ïÕΩ±Ÿî°µï—ÖëÖ—Ñπ¡Öùï}çΩ’π–§(ÄÄÄÅÙÅï±ÕîÅÏ(ÄÄÄÄÄÄÄÅ=¨††ƒ∏∏ıµï—ÖëÖ—Ñπ¡Öùï}çΩ’π–§(ÄÄÄÄÄÄÄÄÄÄÄÄπµÖ¿°Ò¡ÖùïÅAÖùï9’µâï»ËÈπï‹°¡Öùî§πï·¡ïç–†â¡ÖùîÅ…ÖπùîÅÕ—Ö…—ÃÅÖ–ÅΩπîà§§(ÄÄÄÄÄÄÄÄÄÄÄÄπçΩ±±ïç–†§§(ÄÄÄÅÙ)Ù()Õ—…’ç–Å=’—¡’—Q…ÖπÕÖç—•Ω∏ÅÏ(ÄÄÄÅ¡±Ö∏ËÅ=’—¡’—AÖ—°A±Ö∏∞(ÄÄÄÅçΩµµ•——ïêËÅâΩΩ∞∞)Ù()•µ¡∞Å=’—¡’—Q…ÖπÕÖç—•Ω∏ÅÏ(ÄÄÄÅô∏Åâïù•∏°¡Ö—†ËÄôAÖ—†∞Å¡Ω±•ç‰ËÅ·•Õ—•πù=’—¡’—AΩ±•ç‰§Ä¥¯ÅIïÕ’±–ÒMï±ò∞Å5ï…ùï……Ω»¯ÅÏ(ÄÄÄÄÄÄÄÅ±ï–Åô•πÖ±}ï·•Õ—ÃÄÙÅ¡Ö—†π—…Â}ï·•Õ—Ã†§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–Å•πÕ¡ïç–ÅΩ’—¡’–Å¡Ö—†ËÅÌï……Ω…Ùà§§(ÄÄÄÄÄÄÄÅÙ§¸Ï(ÄÄÄÄÄÄÄÅ±ï–Å—Ω≠ï∏ÄÙÅôΩ…µÖ–Ñ†(ÄÄÄÄÄÄÄÄÄÄÄÄâµï…ùï}Ìı}ÌÙà∞(ÄÄÄÄÄÄÄÄÄÄÄÅ¡…ΩçïÕÃËÈ•ê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÅ9aQ}=AIQ%=9}%πôï—ç°}Öëê†ƒ∞Å=…ëï…•πúËÈIï±Ö·ïê§(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅ±ï–Å¡±Ö∏ÄÙ(ÄÄÄÄÄÄÄÄÄÄÄÅ¡±Öπ}Ω’—¡’—}¡Ö—†°¡Ö—†∞Åô•πÖ±}ï·•Õ—Ã∞Å¡Ω±•ç‰∞Äô—Ω≠ï∏§πµÖ¡}ï…»°5ï…ùï……Ω»ËÈ=’—¡’—A±Ö∏§¸Ï(ÄÄÄÄÄÄÄÅ•òÅ¡±Ö∏π—ïµ¡Ω…Ö…Â}¡Ö—†π—…Â}ï·•Õ—Ã†§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–Å•πÕ¡ïç–Å—ïµ¡Ω…Ö…‰ÅΩ’—¡’–Å¡Ö—†ËÅÌï……Ω…Ùà§§(ÄÄÄÄÄÄÄÅÙ§¸ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ…ï—’…∏Å…»°5ï…ùï……Ω»ËÈ=’—¡’—%º†(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄâ—ïµ¡Ω…Ö…‰Åµï…ùîÅΩ’—¡’–Å’πï·¡ïç—ïë±‰ÅÖ±…ïÖë‰Åï·•Õ—Ãàπ—Ω}Ω›πïê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÄ§§Ï(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅ=¨°Mï±òÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ¡±Ö∏∞(ÄÄÄÄÄÄÄÄÄÄÄÅçΩµµ•——ïêËÅôÖ±Õî∞(ÄÄÄÄÄÄÄÅÙ§(ÄÄÄÅÙ((ÄÄÄÅô∏Å—ïµ¡Ω…Ö…Â}¡Ö—††ôÕï±ò§Ä¥¯ÄôAÖ—†ÅÏ(ÄÄÄÄÄÄÄÄôÕï±òπ¡±Ö∏π—ïµ¡Ω…Ö…Â}¡Ö—†(ÄÄÄÅÙ((ÄÄÄÅô∏ÅçΩµµ•–†ôµ’–ÅÕï±ò§Ä¥¯ÅIïÕ’±–ÒAÖ—°	’ò∞Å5ï…ùï……Ω»¯ÅÏ(ÄÄÄÄÄÄÄÅ±ï–Å—ïµ¡Ω…Ö…‰ÄÙÄôÕï±òπ¡±Ö∏π—ïµ¡Ω…Ö…Â}¡Ö—†Ï(ÄÄÄÄÄÄÄÅ±ï–Åô•πÖ±}¡Ö—†ÄÙÄôÕï±òπ¡±Ö∏πô•πÖ±}¡Ö—†Ï(ÄÄÄÄÄÄÄÅ•òÄÖ—ïµ¡Ω…Ö…‰π•Õ}ô•±î†§ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ…ï—’…∏Å…»°5ï…ùï……Ω»ËÈ=’—¡’—%º†(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄâïπù•πîÅë•êÅπΩ–Å¡…Ωë’çîÅÑÅ…ïù’±Ö»Å—ïµ¡Ω…Ö…‰ÅAàπ—Ω}Ω›πïê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÄ§§Ï(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅ=¡ïπ=¡—•ΩπÃËÈπï‹†§(ÄÄÄÄÄÄÄÄÄÄÄÄπ›…•—î°—…’î§(ÄÄÄÄÄÄÄÄÄÄÄÄπΩ¡ï∏°—ïµ¡Ω…Ö…‰§(ÄÄÄÄÄÄÄÄÄÄÄÄπÖπë}—°ï∏°Òô•±ïÅô•±îπÕÂπç}Ö±∞†§§(ÄÄÄÄÄÄÄÄÄÄÄÄπµÖ¡}ï…»°Òï……Ω…Å5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–ÅÕÂπåÅ—ïµ¡Ω…Ö…‰ÅAËÅÌï……Ω…Ùà§§§¸Ï(ÄÄÄÄÄÄÄÅ•òÄÖÕï±òπ¡±Ö∏π…ï¡±Öçï}ï·•Õ—•πú(ÄÄÄÄÄÄÄÄÄÄÄÄòòÅô•πÖ±}¡Ö—†π—…Â}ï·•Õ—Ã†§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–Å…ïç°ïç¨Åô•πÖ∞ÅΩ’—¡’–Å¡Ö—†ËÅÌï……Ω…Ùà§§(ÄÄÄÄÄÄÄÄÄÄÄÅÙ§¸(ÄÄÄÄÄÄÄÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ…ï—’…∏Å…»°5ï…ùï……Ω»ËÈ=’—¡’—A±Ö∏†(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ=’—¡’—A±Öπ……Ω»ËÈ·•Õ—•πù=’—¡’—Ωπô±•ç–∞(ÄÄÄÄÄÄÄÄÄÄÄÄ§§Ï(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅôÃËÈ…ïπÖµî°—ïµ¡Ω…Ö…‰∞Åô•πÖ±}¡Ö—†§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–ÅÖ—Ωµ•çÖ±±‰Åô•πÖ±•ÈîÅAËÅÌï……Ω…Ùà§§(ÄÄÄÄÄÄÄÅÙ§¸Ï(ÄÄÄÄÄÄÄÅ•òÅ±ï–ÅMΩµî°¡Ö…ïπ–§ÄÙÅô•πÖ±}¡Ö—†(ÄÄÄÄÄÄÄÄÄÄÄÄπ¡Ö…ïπ–†§(ÄÄÄÄÄÄÄÄÄÄÄÄπô•±—ï»°Ò¡Ö…ïπ—ÄÖ¡Ö…ïπ–πÖÕ}ΩÕ}Õ—»†§π•Õ}ïµ¡—‰†§§(ÄÄÄÄÄÄÄÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄçmçôú°’π•‡•t(ÄÄÄÄÄÄÄÄÄÄÄÅÕÂπç}Ω’—¡’—}ë•…ïç—Ω…‰°¡Ö…ïπ–§¸Ï(ÄÄÄÄÄÄÄÄÄÄÄÄçmçôú°πΩ–°’π•‡§•t(ÄÄÄÄÄÄÄÄÄÄÄÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄººÅÕ—êËÈôÃÅçÖππΩ–ÅΩ¡ï∏ÅÑÅ]•πëΩ›ÃÅë•…ïç—Ω…‰ÅôΩ»Å±’Õ°•±ï	’ôôï…Ã∏ÅQ°î(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄººÅ—ïµ¡Ω…Ö…‰Åô•±îÅ•—Õï±òÅ›ÖÃÅë’…Öâ±‰Åô±’Õ°ïêÅâïôΩ…îÅ—°îÅÕÖµîµŸΩ±’µîÅ…ïπÖµî∏(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅ¡Ö…ïπ–Ï(ÄÄÄÄÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅÕï±òπçΩµµ•——ïêÄÙÅ—…’îÏ(ÄÄÄÄÄÄÄÅ=¨°ô•πÖ±}¡Ö—†πç±Ωπî†§§(ÄÄÄÅÙ)Ù((çmçôú°’π•‡•t)ô∏ÅÕÂπç}Ω’—¡’—}ë•…ïç—Ω…‰°¡Ö…ïπ–ËÄôAÖ—†§Ä¥¯ÅIïÕ’±–†§∞Å5ï…ùï……Ω»¯ÅÏ(ÄÄÄÅ•±îËÈΩ¡ï∏°¡Ö…ïπ–§(ÄÄÄÄÄÄÄÄπÖπë}—°ï∏°Òë•…ïç—Ω…ÂÅë•…ïç—Ω…‰πÕÂπç}Ö±∞†§§(ÄÄÄÄÄÄÄÄπµÖ¡}ï…»°Òï……Ω…Å5ï…ùï……Ω»ËÈ=’—¡’—%º°ôΩ…µÖ–Ñ†âçÖππΩ–ÅÕÂπåÅΩ’—¡’–Åë•…ïç—Ω…‰ËÅÌï……Ω…Ùà§§§)Ù()•µ¡∞Å…Ω¿ÅôΩ»Å=’—¡’—Q…ÖπÕÖç—•Ω∏ÅÏ(ÄÄÄÅô∏Åë…Ω¿†ôµ’–ÅÕï±ò§ÅÏ(ÄÄÄÄÄÄÄÅ•òÄÖÕï±òπçΩµµ•——ïêÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î†ôÕï±òπ¡±Ö∏π—ïµ¡Ω…Ö…Â}¡Ö—†§Ï(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÅÙ)Ù((ºººÅIï—’…πÃÅ—°îÅçÖ¡Öâ•±•—‰ÅÕï–Å…ï≈’•…ïêÅâ‰Å@–∏ƒÅ5ï…ùî∏(çmµ’Õ—}’Õït)¡’àÅô∏Åµï…ùï}çΩ…ï}çÖ¡Öâ•±•—•ïÃ†§Ä¥¯ÅÖ¡Öâ•±•—ÂMï–ÅÏ(ÄÄÄÅ¡•πçï…¡ëô}Ö¡¡±•çÖ—•Ω∏ËÈ…ï≈’•…ïë}çÖ¡Öâ•±•—•ïÃ°QΩΩ±-•πêËÈ5ï…ùî§)Ù((çmçôú°—ïÕ–•t)µΩêÅ—ïÕ—ÃÅÏ(ÄÄÄÅ’ÕîÅÕ’¡ï»ËË®Ï(ÄÄÄÅ’ÕîÅ¡•πçï…¡ëô}ïπù•πï}Ö¡§ËÈÌAëôÖ¡Öâ•±•—‰∞ÅAëô5ï—ÖëÖ—ÖÙÏ(ÄÄÄÅ’ÕîÅÕ—êËÈÕÂπåËÈÖ—Ωµ•åËÈÌ—Ωµ•çTÃ»∞Å=…ëï…•πúÅÖÃÅ—Ωµ•ç=…ëï…•πùÙÏ((ÄÄÄÄçmçôú°›•πëΩ›Ã•t(ÄÄÄÅ’ÕîÅÕ—êËÈΩÃËÈ›•πëΩ›ÃËÈôÃËÈ=¡ïπ=¡—•ΩπÕ·–Ï((ÄÄÄÅÕ—…’ç–ÅÖ≠ïπù•πîÅÏ(ÄÄÄÄÄÄÄÅΩ’—¡’—}¡ÖùïÃËÅ—Ωµ•çTÃ»∞(ÄÄÄÄÄÄÄÅΩ’—¡’—}°ÖÕ}âΩΩ≠µÖ…≠ÃËÅ—Ωµ•ç	ΩΩ∞∞(ÄÄÄÅÙ((ÄÄÄÅ•µ¡∞ÅÖ≠ïπù•πîÅÏ(ÄÄÄÄÄÄÄÅô∏Åπï‹†§Ä¥¯ÅMï±òÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅMï±òÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅΩ’—¡’—}¡ÖùïÃËÅ—Ωµ•çTÃ»ËÈπï‹†¿§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅΩ’—¡’—}°ÖÕ}âΩΩ≠µÖ…≠ÃËÅ—Ωµ•ç	ΩΩ∞ËÈπï‹°ôÖ±Õî§∞(ÄÄÄÄÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÅÙ((ÄÄÄÅ•µ¡∞ÅAëôπù•πïAΩ…–ÅôΩ»ÅÖ≠ïπù•πîÅÏ(ÄÄÄÄÄÄÄÅô∏Å•ëïπ—•—‰†ôÕï±ò§Ä¥¯Åπù•πï%ëïπ—•—‰ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅπù•πï%ëïπ—•—‰ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ•êËÄâôÖ≠îàπ—Ω}Ω›πïê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅŸï…Õ•Ω∏ËÄàƒàπ—Ω}Ω›πïê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÅÙ(ÄÄÄÄÄÄÄÅÙ((ÄÄÄÄÄÄÄÅô∏ÅçÖ¡Öâ•±•—•ïÃ†ôÕï±ò§Ä¥¯ÅÖ¡Öâ•±•—ÂMï–ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅÖ¡Öâ•±•—ÂMï–ËÈô…Ωµ}çÖ¡Öâ•±•—•ïÃ°mAëôÖ¡Öâ•±•—‰ËÈ%πÕ¡ïç–∞ÅAëôÖ¡Öâ•±•—‰ËÈ5ï…ùït§(ÄÄÄÄÄÄÄÅÙ((ÄÄÄÄÄÄÄÅô∏Å•πÕ¡ïç–†(ÄÄÄÄÄÄÄÄÄÄÄÄôÕï±ò∞(ÄÄÄÄÄÄÄÄÄÄÄÅÕΩ’…çîËÄôAÖ—†∞(ÄÄÄÄÄÄÄÄÄÄÄÅ}Ω¡—•ΩπÃËÅ%πÕ¡ïç—=¡—•ΩπÃù|¯∞(ÄÄÄÄÄÄÄÄ§Ä¥¯ÅIïÕ’±–ÒAëô5ï—ÖëÖ—Ñ∞Åπù•πï……Ω»¯ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å•Õ}Ω’—¡’–ÄÙÅÕΩ’…çî(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπô•±ï}πÖµî†§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπ•Õ}ÕΩµï}Öπê°ÒπÖµïÅπÖµîπ—Ω}Õ—…•πù}±ΩÕÕ‰†§πçΩπ—Ö•πÃ†â¡•πçï…¡ëòµµï…ùï|à§§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅ=¨°Aëô5ï—ÖëÖ—ÑÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ¡Öùï}çΩ’π–ËÅ•òÅ•Õ}Ω’—¡’–ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÕï±òπΩ’—¡’—}¡ÖùïÃπ±ΩÖê°—Ωµ•ç=…ëï…•πúËÈç≈’•…î§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÙÅï±ÕîÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÃ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÙ∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅïπç…Â¡—ïêËÅôÖ±Õî∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ¡ëô}Ÿï…Õ•Ω∏ËÅMΩµî†àƒ∏‹àπ—Ω}Ω›πïê†§§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ°ÖÕ}âΩΩ≠µÖ…≠ÃËÅ•òÅ•Õ}Ω’—¡’–ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÕï±òπΩ’—¡’—}°ÖÕ}âΩΩ≠µÖ…≠Ãπ±ΩÖê°—Ωµ•ç=…ëï…•πúËÈç≈’•…î§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÙÅï±ÕîÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÕΩ’…çîπ—Ω}Õ—…•πù}±ΩÕÕ‰†§πçΩπ—Ö•πÃ†ââΩΩ≠µÖ…≠Ãà§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÙ∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ°ÖÕ}ôΩ…µÃËÅÕΩ’…çîπ—Ω}Õ—…•πù}±ΩÕÕ‰†§πçΩπ—Ö•πÃ†âôΩ…¥à§∞(ÄÄÄÄÄÄÄÄÄÄÄÅÙ§(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÅÙ((ÄÄÄÅ•µ¡∞Å5ï…ùïπù•πïAΩ…–ÅôΩ»ÅÖ≠ïπù•πîÅÏ(ÄÄÄÄÄÄÄÅô∏Åµï…ùî†(ÄÄÄÄÄÄÄÄÄÄÄÄôÕï±ò∞(ÄÄÄÄÄÄÄÄÄÄÄÅ…ï≈’ïÕ–ËÄô5ï…ùïπù•πïIï≈’ïÕ–∞(ÄÄÄÄÄÄÄÄÄÄÄÅ}çΩπ—…Ω∞ËÄô·ïç’—•ΩπΩπ—…Ω∞∞(ÄÄÄÄÄÄÄÄ§Ä¥¯ÅIïÕ’±–Ò5ï…ùïπù•πïIïÕ’±–∞Åπù•πï……Ω»¯ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å¡ÖùïÃÄÙÅ…ï≈’ïÕ–(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπ•π¡’—Ã(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπ•—ï»†§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπµÖ¿°Ò•π¡’—Å•π¡’–π¡ÖùïÃπ±ï∏†§§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπÕ’¥ËËÒ’Õ•Èî¯†§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å¡ÖùïÃÄÙÅ¡ÖùïÃ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄ¨Å’Õ•ÈîËÈô…Ω¥°…ï≈’ïÕ–πÖëë}â±Öπ≠}¡Öùï}•ô}Ωëê§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄ®Å…ï≈’ïÕ–(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπ•π¡’—Ã(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπ•—ï»†§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπô•±—ï»°Ò•π¡’—Å•π¡’–π¡ÖùïÃπ±ï∏†§ÄîÄ»ÄÙÙÄƒ§(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπçΩ’π–†§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–Å¡ÖùïÃÄÙÅ‘Ã»ËÈ—…Â}ô…Ω¥°¡ÖùïÃ§πï·¡ïç–†â—ïÕ–Å¡ÖùîÅçΩ’π–Åô•—ÃÅ‘Ã»à§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅÕï±òπΩ’—¡’—}¡ÖùïÃπÕ—Ω…î°¡ÖùïÃ∞Å—Ωµ•ç=…ëï…•πúËÈIï±ïÖÕî§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅ±ï–ÅâΩΩ≠µÖ…≠}ïπ—…•ïÃÄÙÅµÖ—ç†Å…ï≈’ïÕ–πâΩΩ≠µÖ…≠}¡Ω±•ç‰ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ	ΩΩ≠µÖ…≠AΩ±•ç‰ËÈ•ÕçÖ…êÄÙ¯Ä¿∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ	ΩΩ≠µÖ…≠AΩ±•ç‰ËÈ=πïπ—…ÂAï…Ωç’µïπ–(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅÅ	ΩΩ≠µÖ…≠AΩ±•ç‰ËÈIï—Ö•πÕ=πïπ—…ÂAï…Ωç’µïπ–ÄÙ¯Å…ï≈’ïÕ–π•π¡’—Ãπ±ï∏†§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ	ΩΩ≠µÖ…≠AΩ±•ç‰ËÈIï—Ö•∏ÄÙ¯Äƒ∞(ÄÄÄÄÄÄÄÄÄÄÄÅÙÏ(ÄÄÄÄÄÄÄÄÄÄÄÅÕï±òπΩ’—¡’—}°ÖÕ}âΩΩ≠µÖ…≠Ã(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄπÕ—Ω…î°âΩΩ≠µÖ…≠}ïπ—…•ïÃÄ¯Ä¿∞Å—Ωµ•ç=…ëï…•πúËÈIï±ïÖÕî§Ï(ÄÄÄÄÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ô…ï≈’ïÕ–πΩ’—¡’–∞ÅààïA¥ƒ∏›q∏îï=q∏à§πµÖ¡}ï…»°Òï……Ω…ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅπù•πï……Ω»ËÈπï‹°……Ω…ΩëîËÈ=’—¡’—]…•—ïÖ•±ïê∞Åï……Ω»π—Ω}Õ—…•πú†§§(ÄÄÄÄÄÄÄÄÄÄÄÅÙ§¸Ï(ÄÄÄÄÄÄÄÄÄÄÄÅ=¨°5ï…ùïπù•πïIïÕ’±–ÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ¡Öùï}çΩ’π–ËÅ¡ÖùïÃ∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅâΩΩ≠µÖ…≠}ïπ—…•ïÃ∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅïŸ•ëïπçîËÅYïåËÈπï‹†§∞(ÄÄÄÄÄÄÄÄÄÄÄÅÙ§(ÄÄÄÄÄÄÄÅÙ(ÄÄÄÅÙ((ÄÄÄÅô∏Å’π•≈’ï}¡Ö—†°πÖµîËÄôÕ—»§Ä¥¯ÅAÖ—°	’òÅÏ(ÄÄÄÄÄÄÄÅÕ—êËÈïπÿËÈ—ïµ¡}ë•»†§π©Ω•∏°ôΩ…µÖ–Ñ†(ÄÄÄÄÄÄÄÄÄÄÄÄâ¡•πçï…¡ëòµµï…ùîµ—ïÕ–µÌÙµÌÙµÌπÖµïÙà∞(ÄÄÄÄÄÄÄÄÄÄÄÅ¡…ΩçïÕÃËÈ•ê†§∞(ÄÄÄÄÄÄÄÄÄÄÄÅ9aQ}=AIQ%=9}%πôï—ç°}Öëê†ƒ∞Å=…ëï…•πúËÈIï±Ö·ïê§(ÄÄÄÄÄÄÄÄ§§(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅÕïç…ï—}ëïâ’ù}•Õ}…ïëÖç—ïë}Öπë}µ’±—•±•πï}ŸÖ±’ïÕ}Ö…ï}…ï©ïç—ïê†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅÕïç…ï–ÄÙÅMïç…ï—M—…•πúËÈπï‹†âçΩ……ïç–Å°Ω…ÕîÅâÖ——ï…‰ÅÕ—Ö¡±îà§πï·¡ïç–†âŸÖ±•êÅÕïç…ï–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Åëïâ’úÄÙÅôΩ…µÖ–Ñ†âÌÕïç…ï–Ë˝Ùà§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°ëïâ’úπçΩπ—Ö•πÃ†âIQà§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ†Öëïâ’úπçΩπ—Ö•πÃ†âçΩ……ïç–Å°Ω…Õîà§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°Mïç…ï—M—…•πúËÈπï‹†â±•πî≈qπ±•πî»à§∞Å…»°Mïç…ï—M—…•πù……Ω»§§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏Å…ï≈’ïÕ—}ŸÖ±•ëÖ—•Ωπ}¡…ïÕï…ŸïÕ}ë’¡±•çÖ—ï}ÕΩ’…çïÕ}â’—}…ï©ïç—Õ}’πÕÖôï}Ω’—¡’–†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅÕΩ’…çîÄÙÅ5ï…ùïMΩ’…çîËÈπï‹†âÑπ¡ëòà§Ï(ÄÄÄÄÄÄÄÅ±ï–Å…ï≈’ïÕ–ÄÙ(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïIï≈’ïÕ–ËÈπï‹°mÕΩ’…çîπç±Ωπî†§∞ÅÕΩ’…çït∞ÄâΩ’–πAà§πï·¡ïç–†âŸÖ±•êÅ…ï≈’ïÕ–à§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï≈’ïÕ–πÕΩ’…çïÃ†§π±ï∏†§∞Ä»§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïIï≈’ïÕ–ËÈπï‹°m5ï…ùïMΩ’…çîËÈπï‹†âÑπ¡ëòà•t∞ÄâΩ’–π¡ëòà§∞(ÄÄÄÄÄÄÄÄÄÄÄÅ…»°5ï…ùïIï≈’ïÕ—……Ω»ËÈ9Ω—πΩ’ù°MΩ’…çïÃ§(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïIï≈’ïÕ–ËÈπï‹†(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅm5ï…ùïMΩ’…çîËÈπï‹†âÑπ¡ëòà§∞Å5ï…ùïMΩ’…çîËÈπï‹†âàπ¡ëòà•t∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄâÑπ¡ëòà(ÄÄÄÄÄÄÄÄÄÄÄÄ§∞(ÄÄÄÄÄÄÄÄÄÄÄÅ…»°5ï…ùïIï≈’ïÕ—……Ω»ËÈ=’—¡’—≈’Ö±ÕMΩ’…çîÅÏÅÕΩ’…çï}•πëï‡ËÄ¿ÅÙ§(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅÕï…Ÿ•çï}¡…ïÕï…ŸïÕ}¡Öùï}Ω…ëï…}Öπë}ë’¡±•çÖ—ïÕ}—°ïπ}ô•πÖ±•ÈïÕ}Ö—Ωµ•çÖ±±‰†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††âΩ…ëï…ïêπ¡ëòà§Ï(ÄÄÄÄÄÄÄÅ±ï–ÅÕï±ïç—•Ω∏ËÅAÖùïMï±ïç—•Ω∏ÄÙÄàÃ∞ƒ∞Ãàπ¡Ö…Õî†§πï·¡ïç–†âŸÖ±•êÅÕï±ïç—•Ω∏à§Ï(ÄÄÄÄÄÄÄÅ±ï–Å…ï≈’ïÕ–ÄÙÅ5ï…ùïIï≈’ïÕ–ËÈπï‹†(ÄÄÄÄÄÄÄÄÄÄÄÅl(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïMΩ’…çîËÈπï‹†âô•…Õ–π¡ëòà§π›•—°}Õï±ïç—•Ω∏°Õï±ïç—•Ω∏§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïMΩ’…çîËÈπï‹†âÕïçΩπêπ¡ëòà§∞(ÄÄÄÄÄÄÄÄÄÄÄÅt∞(ÄÄÄÄÄÄÄÄÄÄÄÄôΩ’—¡’–∞(ÄÄÄÄÄÄÄÄ§(ÄÄÄÄÄÄÄÄπï·¡ïç–†âŸÖ±•êÅ…ï≈’ïÕ–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Åïπù•πîÄÙÅÖ≠ïπù•πîËÈπï‹†§Ï(ÄÄÄÄÄÄÄÅ±ï–Å…ï¡Ω…–ÄÙÅ5ï…ùïMï…Ÿ•çîËÈπï‹†ôïπù•πî§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·ïç’—î†ô…ï≈’ïÕ–∞Äô5ï…ùï·ïç’—•Ωπ=¡—•ΩπÃËÈëïôÖ’±–†§§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†âµï…ùîÅÕ’ççïïëÃà§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï¡Ω…–π¡Öùï}çΩ’π–∞Äÿ§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï¡Ω…–πÕΩ’…çï}çΩ’π–∞Ä»§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï¡Ω…–πâΩΩ≠µÖ…≠}ïπ—…•ïÃ∞Ä¿§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°Ω’—¡’–π•Õ}ô•±î†§§Ï(ÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î°Ω’—¡’–§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅΩπï}ïπ—…Â}¡Ω±•çÂ}…ï≈’•…ïÕ}Öπë}…ï¡Ω…—Õ}Ωπï}âΩΩ≠µÖ…≠}¡ï…}Ω…ëï…ïë}ÕΩ’…çî†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††âΩπîµïπ—…‰µ¡ï»µëΩç’µïπ–π¡ëòà§Ï(ÄÄÄÄÄÄÄÅ±ï–Å…ï≈’ïÕ–ÄÙÅ5ï…ùïIï≈’ïÕ–ËÈπï‹†(ÄÄÄÄÄÄÄÄÄÄÄÅl(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïMΩ’…çîËÈπï‹†âôΩ±ëï»Ωô•…Õ–π¡ëòà§∞(ÄÄÄÄÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùïMΩ’…çîËÈπï‹†âôΩ±ëï»ΩÕïçΩπêπ¡ëòà§∞(ÄÄÄÄÄÄÄÄÄÄÄÅt∞(ÄÄÄÄÄÄÄÄÄÄÄÄôΩ’—¡’–∞(ÄÄÄÄÄÄÄÄ§(ÄÄÄÄÄÄÄÄπï·¡ïç–†âŸÖ±•êÅ…ï≈’ïÕ–à§Ï(ÄÄÄÄÄÄÄÅ±ï–ÅΩ¡—•ΩπÃÄÙÅ5ï…ùï·ïç’—•Ωπ=¡—•ΩπÃÅÏ(ÄÄÄÄÄÄÄÄÄÄÄÅâΩΩ≠µÖ…≠}¡Ω±•ç‰ËÅ	ΩΩ≠µÖ…≠AΩ±•ç‰ËÈ=πïπ—…ÂAï…Ωç’µïπ–∞(ÄÄÄÄÄÄÄÄÄÄÄÄ∏π5ï…ùï·ïç’—•Ωπ=¡—•ΩπÃËÈëïôÖ’±–†§(ÄÄÄÄÄÄÄÅÙÏ((ÄÄÄÄÄÄÄÅ±ï–Å…ï¡Ω…–ÄÙÅ5ï…ùïMï…Ÿ•çîËÈπï‹†ôÖ≠ïπù•πîËÈπï‹†§§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·ïç’—î†ô…ï≈’ïÕ–∞ÄôΩ¡—•ΩπÃ§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†ââΩΩ≠µÖ…¨Å¡Ω±•ç‰ÅÕ’ççïïëÃà§Ï((ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï¡Ω…–πâΩΩ≠µÖ…≠}ïπ—…•ïÃ∞Ä»§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ°…ï¡Ω…–πâΩΩ≠µÖ…≠}ÕΩ’…çïÕ}ë•ÕçÖ…ëïê∞Ä¿§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°Ω’—¡’–π•Õ}ô•±î†§§Ï(ÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î°Ω’—¡’–§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏Å…ï¡±Öçï}¡Ω±•çÂ}Ö—Ωµ•çÖ±±Â}…ï¡±ÖçïÕ}Öπ}ï·•Õ—•πù}Ω’—¡’–†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††â…ï¡±Öçîµï·•Õ—•πúπ¡ëòà§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ôΩ’—¡’–∞ÅàâŸï…•ô•ïêµΩ±êµΩ’—¡’–à§πï·¡ïç–†â›…•—îÅΩ…•ù•πÖ∞ÅΩ’—¡’–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Åµ’–Å—…ÖπÕÖç—•Ω∏ÄÙÅ=’—¡’—Q…ÖπÕÖç—•Ω∏ËÈâïù•∏†ôΩ’—¡’–∞Å·•Õ—•πù=’—¡’—AΩ±•ç‰ËÈIï¡±Öçî§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†ââïù•∏Å…ï¡±Öçïµïπ–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Å—ïµ¡Ω…Ö…‰ÄÙÅ—…ÖπÕÖç—•Ω∏π—ïµ¡Ω…Ö…Â}¡Ö—††§π—Ω}¡Ö—°}â’ò†§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ô—ïµ¡Ω…Ö…‰∞ÅàâŸï…•ô•ïêµπï‹µΩ’—¡’–à§πï·¡ïç–†â›…•—îÅ…ï¡±Öçïµïπ–ÅΩ’—¡’–à§Ï((ÄÄÄÄÄÄÄÅ—…ÖπÕÖç—•Ω∏πçΩµµ•–†§πï·¡ïç–†â…ï¡±ÖçîÅï·•Õ—•πúÅΩ’—¡’–à§Ï((ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅôÃËÈ…ïÖê†ôΩ’—¡’–§πï·¡ïç–†â…ïÖêÅ…ï¡±ÖçïêÅΩ’—¡’–à§∞(ÄÄÄÄÄÄÄÄÄÄÄÅàâŸï…•ô•ïêµπï‹µΩ’—¡’–à(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ†Ö—ïµ¡Ω…Ö…‰πï·•Õ—Ã†§§Ï(ÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î°Ω’—¡’–§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅôÖ•±}¡Ω±•çÂ}¡…ïÕï…ŸïÕ}Ö}ëïÕ—•πÖ—•Ωπ}ç…ïÖ—ïë}Öô—ï…}¡±Öππ•πú†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††â±Ö—îµçΩπô±•ç–π¡ëòà§Ï(ÄÄÄÄÄÄÄÅ±ï–Åµ’–Å—…ÖπÕÖç—•Ω∏ÄÙÅ=’—¡’—Q…ÖπÕÖç—•Ω∏ËÈâïù•∏†ôΩ’—¡’–∞Å·•Õ—•πù=’—¡’—AΩ±•ç‰ËÈÖ•∞§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†ââïù•∏ÅçΩπô±•ç–µô…ïîÅΩ’—¡’–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Å—ïµ¡Ω…Ö…‰ÄÙÅ—…ÖπÕÖç—•Ω∏π—ïµ¡Ω…Ö…Â}¡Ö—††§π—Ω}¡Ö—°}â’ò†§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ô—ïµ¡Ω…Ö…‰∞Åàâ’πçΩµµ•——ïêµΩ’—¡’–à§πï·¡ïç–†â›…•—îÅ—ïµ¡Ω…Ö…‰ÅΩ’—¡’–à§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ôΩ’—¡’–∞Åàâ±Ö—îµï·•Õ—•πúµΩ’—¡’–à§πï·¡ïç–†âç…ïÖ—îÅ…Öç•πúÅëïÕ—•πÖ—•Ω∏à§Ï((ÄÄÄÄÄÄÄÅ±ï–Åï……Ω»ÄÙÅ—…ÖπÕÖç—•Ω∏πçΩµµ•–†§πï·¡ïç—}ï…»†â±Ö—îÅëïÕ—•πÖ—•Ω∏Åµ’Õ–Å›•∏à§Ï((ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°µÖ—ç°ïÃÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅï……Ω»∞(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈ=’—¡’—A±Ö∏°=’—¡’—A±Öπ……Ω»ËÈ·•Õ—•πù=’—¡’—Ωπô±•ç–§(ÄÄÄÄÄÄÄÄ§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅôÃËÈ…ïÖê†ôΩ’—¡’–§πï·¡ïç–†â…ïÖêÅ¡…ïÕï…ŸïêÅëïÕ—•πÖ—•Ω∏à§∞(ÄÄÄÄÄÄÄÄÄÄÄÅàâ±Ö—îµï·•Õ—•πúµΩ’—¡’–à(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅë…Ω¿°—…ÖπÕÖç—•Ω∏§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ†Ö—ïµ¡Ω…Ö…‰πï·•Õ—Ã†§§Ï(ÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î°Ω’—¡’–§Ï(ÄÄÄÅÙ((ÄÄÄÄçmçôú°›•πëΩ›Ã•t(ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅôÖ•±ïë}›•πëΩ›Õ}…ï¡±Öçï}¡…ïÕï…ŸïÕ}—°ï}Ω±ë}Ω’—¡’—}Öπë}ç±ïÖπÕ}—°ï}—ïµ¡Ω…Ö…Â}ô•±î†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††â±Ωç≠ïêµ…ï¡±Öçïµïπ–π¡ëòà§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ôΩ’—¡’–∞Åàâ±Ωç≠ïêµΩ±êµΩ’—¡’–à§πï·¡ïç–†â›…•—îÅΩ…•ù•πÖ∞ÅΩ’—¡’–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Åµ’–Å—…ÖπÕÖç—•Ω∏ÄÙÅ=’—¡’—Q…ÖπÕÖç—•Ω∏ËÈâïù•∏†ôΩ’—¡’–∞Å·•Õ—•πù=’—¡’—AΩ±•ç‰ËÈIï¡±Öçî§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†ââïù•∏Å…ï¡±Öçïµïπ–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Å—ïµ¡Ω…Ö…‰ÄÙÅ—…ÖπÕÖç—•Ω∏π—ïµ¡Ω…Ö…Â}¡Ö—††§π—Ω}¡Ö—°}â’ò†§Ï(ÄÄÄÄÄÄÄÅôÃËÈ›…•—î†ô—ïµ¡Ω…Ö…‰∞Åàâ’πçΩµµ•——ïêµπï‹µΩ’—¡’–à§πï·¡ïç–†â›…•—îÅ—ïµ¡Ω…Ö…‰ÅΩ’—¡’–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Å±Ωç≠ïë}Ω’—¡’–ÄÙÅ=¡ïπ=¡—•ΩπÃËÈπï‹†§(ÄÄÄÄÄÄÄÄÄÄÄÄπ…ïÖê°—…’î§(ÄÄÄÄÄÄÄÄÄÄÄÄπ›…•—î°—…’î§(ÄÄÄÄÄÄÄÄÄÄÄÄπÕ°Ö…ï}µΩëî†¿§(ÄÄÄÄÄÄÄÄÄÄÄÄπΩ¡ï∏†ôΩ’—¡’–§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç–†â±Ωç¨Åï·•Õ—•πúÅΩ’—¡’–Å›•—°Ω’–Åëï±ï—îÅÕ°Ö…•πúà§Ï((ÄÄÄÄÄÄÄÅ±ï–Åï……Ω»ÄÙÅ—…ÖπÕÖç—•Ω∏(ÄÄÄÄÄÄÄÄÄÄÄÄπçΩµµ•–†§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç—}ï…»†â]•πëΩ›ÃÅµ’Õ–Å…ï©ïç–Å…ï¡±Öçïµïπ–Å›°•±îÅëïÕ—•πÖ—•Ω∏Å•ÃÅ±Ωç≠ïêà§Ï((ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°µÖ—ç°ïÃÑ°ï……Ω»∞Å5ï…ùï……Ω»ËÈ=’—¡’—%º°|§§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°Ω’—¡’–πï·•Õ—Ã†§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°—ïµ¡Ω…Ö…‰πï·•Õ—Ã†§§Ï(ÄÄÄÄÄÄÄÅë…Ω¿°±Ωç≠ïë}Ω’—¡’–§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅôÃËÈ…ïÖê†ôΩ’—¡’–§πï·¡ïç–†â…ïÖêÅ¡…ïÕï…ŸïêÅΩ’—¡’–ÅÖô—ï»Å…ï±ïÖÕ•πúÅ±Ωç¨à§∞(ÄÄÄÄÄÄÄÄÄÄÄÅàâ±Ωç≠ïêµΩ±êµΩ’—¡’–à(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅë…Ω¿°—…ÖπÕÖç—•Ω∏§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ†Ö—ïµ¡Ω…Ö…‰πï·•Õ—Ã†§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…—}ïƒÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅôÃËÈ…ïÖê†ôΩ’—¡’–§πï·¡ïç–†â…ïÖêÅΩ’—¡’–ÅÖô—ï»Åç±ïÖπ’¿à§∞(ÄÄÄÄÄÄÄÄÄÄÄÅàâ±Ωç≠ïêµΩ±êµΩ’—¡’–à(ÄÄÄÄÄÄÄÄ§Ï(ÄÄÄÄÄÄÄÅ±ï–Å|ÄÙÅôÃËÈ…ïµΩŸï}ô•±î°Ω’—¡’–§Ï(ÄÄÄÅÙ((ÄÄÄÄçm—ïÕ—t(ÄÄÄÅô∏ÅÕï…Ÿ•çï}…ï©ïç—Õ}ôΩ…µÕ}âïôΩ…ï}ïπù•πï}µï…ùî†§ÅÏ(ÄÄÄÄÄÄÄÅ±ï–ÅΩ’—¡’–ÄÙÅ’π•≈’ï}¡Ö—††âôΩ…µÃπ¡ëòà§Ï(ÄÄÄÄÄÄÄÅ±ï–Å…ï≈’ïÕ–ÄÙÅ5ï…ùïIï≈’ïÕ–ËÈπï‹†(ÄÄÄÄÄÄÄÄÄÄÄÅm5ï…ùïMΩ’…çîËÈπï‹†âôΩ…¥π¡ëòà§∞Å5ï…ùïMΩ’…çîËÈπï‹†â¡±Ö•∏π¡ëòà•t∞(ÄÄÄÄÄÄÄÄÄÄÄÄôΩ’—¡’–∞(ÄÄÄÄÄÄÄÄ§(ÄÄÄÄÄÄÄÄπï·¡ïç–†âŸÖ±•êÅ…ï≈’ïÕ–à§Ï(ÄÄÄÄÄÄÄÅ±ï–Åï……Ω»ÄÙÅ5ï…ùïMï…Ÿ•çîËÈπï‹†ôÖ≠ïπù•πîËÈπï‹†§§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·ïç’—î†ô…ï≈’ïÕ–∞Äô5ï…ùï·ïç’—•Ωπ=¡—•ΩπÃËÈëïôÖ’±–†§§(ÄÄÄÄÄÄÄÄÄÄÄÄπï·¡ïç—}ï…»†âôΩ…µÃÅÖ…îÅπΩ–ÅŸï…•ô•ïêà§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ°µÖ—ç°ïÃÑ†(ÄÄÄÄÄÄÄÄÄÄÄÅï……Ω»∞(ÄÄÄÄÄÄÄÄÄÄÄÅ5ï…ùï……Ω»ËÈΩ…µÕUπÕ’¡¡Ω…—ïêÅÏÅÕΩ’…çï}•πëï‡ËÄ¿ÅÙ(ÄÄÄÄÄÄÄÄ§§Ï(ÄÄÄÄÄÄÄÅÖÕÕï…–Ñ†ÖΩ’—¡’–πï·•Õ—Ã†§§Ï(ÄÄÄÅÙ)Ù(