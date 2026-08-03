#![forbid(unsafe_code)]
//! Deterministic split planning independent of a concrete PDF engine.

use pincerpdf_domain::{PageNumber, PageSelection, ResolveSelectionError};
use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// How a source document is partitioned into output parts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SplitRule {
    /// Emit one output per source page.
    EveryPage,
    /// Emit consecutive parts containing at most this many pages.
    FixedPageCount(NonZeroU32),
    /// Emit the explicitly selected ranges as separate outputs.
    Ranges(Vec<PageSelection>),
}

impl FromStr for SplitRule {
    type Err = SplitRuleParseError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        if input.eq_ignore_ascii_case("every-page") {
            return Ok(Self::EveryPage);
        }
        if let Some(value) = input.strip_prefix("every:") {
            let count = value
                .parse::<u32>()
                .ok()
                .and_then(NonZeroU32::new)
                .ok_or(SplitRuleParseError)?;
            return Ok(Self::FixedPageCount(count));
        }
        let ranges = input
            .split(';')
            .map(str::parse::<PageSelection>)
            .collect::<Result<Vec<_>, _>>()?;
        (!ranges.is_empty())
            .then_some(Self::Ranges(ranges))
            .ok_or(SplitRuleParseError)
    }
}

/// Stable parse error for CLI and UI validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitRuleParseError;

impl fmt::Display for SplitRuleParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("split rule must be every-page, every:N, or semicolon-separated ranges")
    }
}

impl Error for SplitRuleParseError {}

impl From<pincerpdf_domain::ParseSelectionError> for SplitRuleParseError {
    fn from(_: pincerpdf_domain::ParseSelectionError) -> Self {
        Self
    }
}

/// One planned split output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitPart {
    /// One-based output ordinal.
    pub ordinal: u32,
    /// Exact source page order for this output.
    pub pages: Vec<PageNumber>,
    /// Stable filename stem before the `.pdf` extension.
    pub filename_stem: String,
}

/// Complete engine-independent split plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitPlan {
    /// Original source path.
    pub source: PathBuf,
    /// Ordered output parts.
    pub parts: Vec<SplitPart>,
}

/// Planner failure with no filesystem or engine side effects.
#[derive(Debug, Eq, PartialEq)]
pub enum SplitPlanError {
    /// A document with zero pages cannot be split.
    EmptyDocument,
    /// A range resolved to no pages.
    EmptyRange {
        /// Zero-based range index in the requested rule.
        ordinal: usize,
    },
    /// A range referenced a page beyond the source.
    Selection(ResolveSelectionError),
    /// An output ordinal overflowed the supported one-based range.
    TooManyParts,
}

impl fmt::Display for SplitPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => formatter.write_str("cannot split a zero-page document"),
            Self::EmptyRange { ordinal } => {
                write!(formatter, "split range {ordinal} selected no pages")
            }
            Self::Selection(error) => error.fmt(formatter),
            Self::TooManyParts => {
                formatter.write_str("split output count exceeded the supported range")
            }
        }
    }
}

impl Error for SplitPlanError {}

impl From<ResolveSelectionError> for SplitPlanError {
    fn from(error: ResolveSelectionError) -> Self {
        Self::Selection(error)
    }
}

/// Builds a deterministic split plan for `source`.
///
/// # Errors
///
/// Returns a [`SplitPlanError`] when the source has no pages, a range is out
/// of bounds, or the output count cannot be represented.
pub fn plan_split(
    source: impl Into<PathBuf>,
    total_pages: u32,
    rule: &SplitRule,
) -> Result<SplitPlan, SplitPlanError> {
    if total_pages == 0 {
        return Err(SplitPlanError::EmptyDocument);
    }
    let source = source.into();
    let page_groups = match rule {
        SplitRule::EveryPage => (1..=total_pages)
            .map(|page| numbered_range(page, page))
            .collect::<Result<Vec<_>, _>>()?,
        SplitRule::FixedPageCount(count) => {
            let count = count.get();
            (1..=total_pages)
                .step_by(usize::try_from(count).unwrap_or(usize::MAX))
                .map(|start| {
                    let end = start
                        .saturating_add(count)
                        .saturating_sub(1)
                        .min(total_pages);
                    numbered_range(start, end)
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        SplitRule::Ranges(ranges) => ranges
            .iter()
            .enumerate()
            .map(|(ordinal, range)| {
                let pages = range.resolve(total_pages)?;
                if pages.is_empty() {
                    return Err(SplitPlanError::EmptyRange { ordinal });
                }
                Ok(pages)
            })
            .collect::<Result<Vec<_>, SplitPlanError>>()?,
    };
    let stem = source_stem(&source);
    let parts = page_groups
        .into_iter()
        .enumerate()
        .map(|(index, pages)| {
            let ordinal = u32::try_from(index + 1).map_err(|_| SplitPlanError::TooManyParts)?;
            Ok(SplitPart {
                ordinal,
                pages,
                filename_stem: format!("{stem}-{ordinal:03}"),
            })
        })
        .collect::<Result<Vec<_>, SplitPlanError>>()?;
    Ok(SplitPlan { source, parts })
}

fn source_stem(source: &Path) -> String {
    let hidden_extension_only = source
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.') && !name[1..].contains('.'));
    if hidden_extension_only {
        return "document".to_owned();
    }
    let raw = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document");
    let mut output = String::with_capacity(raw.len());
    for character in raw.chars() {
        if character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | ' ') {
            output.push(character);
        } else {
            output.push('_');
        }
    }
    let trimmed = output.trim().trim_matches('.');
    if trimmed.is_empty() {
        "document".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn numbered_range(start: u32, end: u32) -> Result<Vec<PageNumber>, SplitPlanError> {
    (start..=end)
        .map(PageNumber::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| SplitPlanError::EmptyDocument)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_preserves_all_pages_without_loss() {
        let plan = plan_split("book.pdf", 3, &SplitRule::EveryPage).expect("valid plan");
        assert_eq!(
            plan.parts
                .iter()
                .map(|part| part.pages.len())
                .sum::<usize>(),
            3
        );
        assert_eq!(plan.parts[2].pages, vec![PageNumber::new(3).expect("page")]);
        assert_eq!(plan.parts[0].filename_stem, "book-001");
    }

    #[test]
    fn fixed_count_partitions_the_final_short_part() {
        let plan = plan_split(
            "book.pdf",
            5,
            &SplitRule::FixedPageCount(NonZeroU32::new(2).expect("nonzero")),
        )
        .expect("valid plan");
        assert_eq!(
            plan.parts
                .iter()
                .map(|part| part.pages.len())
                .collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
    }

    #[test]
    fn explicit_ranges_keep_order_and_duplicates() {
        let ranges = vec![
            "3,1-2".parse().expect("range"),
            "4-".parse().expect("range"),
        ];
        let plan = plan_split("report.pdf", 4, &SplitRule::Ranges(ranges)).expect("valid plan");
        assert_eq!(
            plan.parts[0]
                .pages
                .iter()
                .map(|page| page.get())
                .collect::<Vec<_>>(),
            vec![3, 1, 2]
        );
        assert_eq!(
            plan.parts[1]
                .pages
                .iter()
                .map(|page| page.get())
                .collect::<Vec<_>>(),
            vec![4]
        );
    }

    #[test]
    fn out_of_bounds_range_fails_before_output_creation() {
        let ranges = vec!["2-5".parse().expect("range")];
        assert!(matches!(
            plan_split("report.pdf", 4, &SplitRule::Ranges(ranges)),
            Err(SplitPlanError::Selection(_))
        ));
    }

    #[test]
    fn source_stem_sanitizes_pathological_names() {
        assert_eq!(source_stem(Path::new("測試:report.pdf")), "測試_report");
        assert_eq!(source_stem(Path::new(".pdf")), "document");
    }

    #[test]
    fn rule_parser_supports_every_page_and_fixed_count() {
        assert_eq!(
            "every-page".parse::<SplitRule>().expect("rule"),
            SplitRule::EveryPage
        );
        assert_eq!(
            "every:3".parse::<SplitRule>().expect("rule"),
            SplitRule::FixedPageCount(NonZeroU32::new(3).expect("nonzero"))
        );
        assert!("every:0".parse::<SplitRule>().is_err());
    }
}
