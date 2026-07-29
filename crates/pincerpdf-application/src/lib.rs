#![forbid(unsafe_code)]
//! Application policies that orchestrate domain values and engine ports.

use pincerpdf_domain::ToolKind;
use pincerpdf_engine_api::{CapabilitySet, PdfCapability};
use std::error::Error;
use std::fmt;

/// Returns the minimum engine capabilities needed to expose a tool.
#[must_use]
pub fn required_capabilities(tool: ToolKind) -> CapabilitySet {
    use PdfCapability as Capability;

    let capabilities: &[Capability] = match tool {
        ToolKind::Merge => &[Capability::Inspect, Capability::Merge],
        ToolKind::Split => &[Capability::Inspect, Capability::Split],
        ToolKind::SplitByBookmarks => &[
            Capability::Inspect,
            Capability::Split,
            Capability::Bookmarks,
        ],
        ToolKind::SplitBySize => &[
            Capability::Inspect,
            Capability::Split,
            Capability::SplitBySize,
        ],
        ToolKind::AlternateMix => &[Capability::Inspect, Capability::AlternateMix],
        ToolKind::InsertPages => &[Capability::Inspect, Capability::InsertPages],
        ToolKind::Extract => &[Capability::Inspect, Capability::Extract],
        ToolKind::Rotate => &[Capability::Inspect, Capability::Rotate],
    };

    CapabilitySet::from_capabilities(capabilities.iter().copied())
}

/// Validates that an adapter may execute the requested tool.
pub fn validate_tool_capabilities(
    tool: ToolKind,
    available: &CapabilitySet,
) -> Result<(), MissingCapabilities> {
    let missing = available.missing(&required_capabilities(tool));
    if missing.is_empty() {
        Ok(())
    } else {
        Err(MissingCapabilities { tool, missing })
    }
}

/// A tool is unavailable because the configured engine lacks proven behavior.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MissingCapabilities {
    tool: ToolKind,
    missing: Vec<PdfCapability>,
}

impl MissingCapabilities {
    /// Returns the unavailable tool.
    #[must_use]
    pub const fn tool(&self) -> ToolKind {
        self.tool
    }

    /// Returns missing capabilities in stable order.
    #[must_use]
    pub fn missing(&self) -> &[PdfCapability] {
        &self.missing
    }
}

impl fmt::Display for MissingCapabilities {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "engine cannot run {:?}; missing capabilities: {:?}",
            self.tool, self.missing
        )
    }
}

impl Error for MissingCapabilities {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_requires_inspection_and_merge() {
        let available = CapabilitySet::from_capabilities([
            PdfCapability::Inspect,
            PdfCapability::Merge,
        ]);
        assert_eq!(validate_tool_capabilities(ToolKind::Merge, &available), Ok(()));
    }

    #[test]
    fn bookmark_split_reports_all_missing_capabilities() {
        let available = CapabilitySet::from_capabilities([PdfCapability::Inspect]);
        let error = validate_tool_capabilities(ToolKind::SplitByBookmarks, &available)
            .expect_err("split and bookmark support are missing");
        assert_eq!(
            error.missing(),
            &[PdfCapability::Split, PdfCapability::Bookmarks]
        );
    }
}
