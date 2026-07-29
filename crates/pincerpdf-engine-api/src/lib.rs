#![forbid(unsafe_code)]
//! Ports implemented by concrete PDF engines.

use pincerpdf_domain::ErrorCode;
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::Path;

/// Engine behaviors used by the application layer.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PdfCapability {
    /// Read document metadata and page count.
    Inspect,
    /// Merge source documents.
    Merge,
    /// Split documents by page rules.
    Split,
    /// Read and preserve bookmark trees.
    Bookmarks,
    /// Estimate and split by output size.
    SplitBySize,
    /// Interleave page sequences.
    AlternateMix,
    /// Insert page sequences multiple times.
    InsertPages,
    /// Extract selected pages.
    Extract,
    /// Rotate selected pages.
    Rotate,
    /// Read and write encrypted documents.
    Encryption,
    /// Preserve or flatten interactive forms.
    Forms,
    /// Render pages for visual comparison and thumbnails.
    Render,
}

/// Deterministic set of capabilities advertised by an engine adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet(BTreeSet<PdfCapability>);

impl CapabilitySet {
    /// Creates an empty capability set.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Creates a set from an iterator, removing duplicates.
    #[must_use]
    pub fn from_capabilities(capabilities: impl IntoIterator<Item = PdfCapability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// Returns whether a capability is present.
    #[must_use]
    pub fn contains(&self, capability: PdfCapability) -> bool {
        self.0.contains(&capability)
    }

    /// Iterates capabilities in stable order.
    pub fn iter(&self) -> impl Iterator<Item = PdfCapability> + '_ {
        self.0.iter().copied()
    }

    /// Returns capabilities from `required` that are not present.
    #[must_use]
    pub fn missing(&self, required: &Self) -> Vec<PdfCapability> {
        required
            .iter()
            .filter(|capability| !self.contains(*capability))
            .collect()
    }
}

/// Stable identity and version of an engine implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineIdentity {
    /// Stable adapter identifier such as `qpdf`.
    pub id: String,
    /// Human-readable engine version.
    pub version: String,
}

/// Semantic metadata needed before planning an operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PdfMetadata {
    /// Number of pages reported by the engine.
    pub page_count: u32,
    /// Whether the source is encrypted.
    pub encrypted: bool,
    /// Declared PDF version, if available.
    pub pdf_version: Option<String>,
    /// Whether a bookmark outline is present.
    pub has_bookmarks: bool,
    /// Whether an `AcroForm` is present.
    pub has_forms: bool,
}

/// Password handling inputs are borrowed and must never be logged by adapters.
#[derive(Clone, Copy, Debug, Default)]
pub struct InspectOptions<'password> {
    /// Optional password supplied explicitly for this inspection.
    pub password: Option<&'password str>,
}

/// Error returned by a PDF engine adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineError {
    code: ErrorCode,
    message: String,
}

impl EngineError {
    /// Constructs an adapter error without embedding secret input.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable application error code.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for EngineError {}

/// Replaceable boundary implemented by QPDF/MuPDF or future adapters.
pub trait PdfEnginePort: Send + Sync {
    /// Returns stable engine identity.
    fn identity(&self) -> EngineIdentity;

    /// Returns all behaviors proven by the adapter's capability suite.
    fn capabilities(&self) -> CapabilitySet;

    /// Inspects semantic metadata without mutating the source.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the source cannot be read, unlocked, or inspected.
    fn inspect(
        &self,
        source: &Path,
        options: InspectOptions<'_>,
    ) -> Result<PdfMetadata, EngineError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_set_deduplicates_and_reports_missing_items() {
        let available = CapabilitySet::from_capabilities([
            PdfCapability::Inspect,
            PdfCapability::Merge,
            PdfCapability::Merge,
        ]);
        let required = CapabilitySet::from_capabilities([
            PdfCapability::Inspect,
            PdfCapability::Merge,
            PdfCapability::Encryption,
        ]);

        assert_eq!(
            available.missing(&required),
            vec![PdfCapability::Encryption]
        );
        assert_eq!(available.iter().count(), 2);
    }

    #[test]
    fn engine_error_exposes_code_without_exposing_internal_fields() {
        let error = EngineError::new(ErrorCode::PasswordRequired, "password required");
        assert_eq!(error.code(), ErrorCode::PasswordRequired);
        assert_eq!(error.to_string(), "password required");
    }
}
