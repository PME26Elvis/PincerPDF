#![forbid(unsafe_code)]
//! Deterministic split planning independent of a concrete PDF engine.

use pincerpdf_domain::{PageNumber, PageSelection, ResolveSelectionError};
use std::error::Error;
use std::fmt;
use std::num::{NonZeroU32, NonZeroU64};
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
    /// Start a new output at each ordered bookmark boundary selected by the
    /// engine adapter. The adapter records the source outline depth on each
    /// boundary so nested-bookmark policy remains explicit at the boundary.
    Bookmarks(Vec<BookmarkBoundary>),
    /// Greedily group consecutive pages under a conservative byte estimate.
    BySize {
        /// Maximum estimated bytes allowed in one output.
        max_bytes: NonZeroU64,
        /// One estimate for every source page, in one-based page order.
        page_estimates: Vec<PageSizeEstimate>,
    },
}

/// A validated bookmark boundary supplied by an engine adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkBoundary {
    /// User-visible bookmark title retained for downstream UI and naming.
    pub title: String,
    /// One-based source page at which this output starts.
    pub page: PageNumber,
    /// Zero-based depth in the source outline tree (top-level is `0`).
    pub depth: u32,
}

/// Conservative serialized-size estimate for one source page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageSizeEstimate {
    /// One-based source page represented by this estimate.
    pub page: PageNumber,
    /// Estimated bytes of a single-page QPDF materialization.
    pub estimated_bytes: NonZeroU64,
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
    /// Optional bookmark title that introduced this output.
    pub label: Option<String>,
    /// Stable filename stem before the `.pdf` extension.
    pub filename_stem: String,
}

type PageGroup = (Vec<PageNumber>, Option<String>);

/// Complete engine-independent split plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SplitPlan {
    /// Original source path.
    pub source: PathBuf,
    /// Ordered output parts.
    pub parts: Vec<SplitPart>,
    /// Optional conservative limit that the materializer must verify.
    pub size_limit_bytes: Option<NonZeroU64>,
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
    /// No bookmark boundaries were supplied.
    NoBookmarks,
    /// A bookmark boundary referenced a page beyond the source.
    BookmarkOutOfBounds {
        /// Zero-based boundary index.
        ordinal: usize,
        /// Referenced page outside the inspected document.
        page: PageNumber,
    },
    /// Bookmark boundaries must be strictly ascending.
    BookmarkOrder {
        /// Zero-based boundary index that violated ascending order.
        ordinal: usize,
    },
    /// The size rule did not provide one estimate for each source page.
    SizeEstimateCount {
        /// Expected number of page estimates.
        expected: u32,
        /// Number of estimates supplied by the engine.
        actual: usize,
    },
    /// A size estimate was not for the next page in one-based order.
    SizeEstimateOrder {
        /// Zero-based estimate index that violated ordering.
        ordinal: usize,
        /// Expected page number.
        expected: PageNumber,
        /// Supplied page number.
        actual: PageNumber,
    },
    /// A single page cannot fit under the requested limit.
    PageExceedsSizeLimit {
        /// One-based page that exceeded the limit.
        page: PageNumber,
        /// Its estimated bytes.
        estimated_bytes: NonZeroU64,
        /// Requested maximum bytes.
        max_bytes: NonZeroU64,
    },
    /// The estimate accumulator overflowed `u64`.
    SizeEstimateOverflow,
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
            Self::NoBookmarks => {
                formatter.write_str("bookmark split requires at least one boundary")
            }
            Self::BookmarkOutOfBounds { ordinal, page } => {
                write!(
                    formatter,
                    "bookmark boundary {ordinal} references page {page} outside the document"
                )
            }
            Self::BookmarkOrder { ordinal } => {
                write!(
                    formatter,
                    "bookmark boundary {ordinal} is not strictly after the previous boundary"
                )
            }
            Self::SizeEstimateCount { expected, actual } => {
                write!(
                    formatter,
                    "size split requires {expected} page estimates, got {actual}"
                )
            }
            Self::SizeEstimateOrder {
                ordinal,
                expected,
                actual,
            } => write!(
                formatter,
                "size estimate {ordinal} targets page {actual}, expected page {expected}"
            ),
            Self::PageExceedsSizeLimit {
                page,
                estimated_bytes,
                max_bytes,
            } => write!(
                formatter,
                "page {page} estimate {estimated_bytes} exceeds split limit {max_bytes}"
            ),
            Self::SizeEstimateOverflow => {
                formatter.write_str("split size estimate overflowed the supported range")
            }
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
    let (page_groups, size_limit_bytes) = match rule {
        SplitRule::EveryPage => (
            (1..=total_pages)
                .map(|page| numbered_range(page, page))
                .map(|pages| pages.map(|pages| (pages, None)))
                .collect::<Result<Vec<_>, _>>()?,
            None,
        ),
        SplitRule::FixedPageCount(count) => {
            let count = count.get();
            (
                (1..=total_pages)
                    .step_by(usize::try_from(count).unwrap_or(usize::MAX))
                    .map(|start| {
                        let end = start
                            .saturating_add(count)
                            .saturating_sub(1)
                            .min(total_pages);
                        numbered_range(start, end).map(|pages| (pages, None))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                None,
            )
        }
        SplitRule::Ranges(ranges) => (
            ranges
                .iter()
                .enumerate()
                .map(|(ordinal, range)| {
                    let pages = range.resolve(total_pages)?;
                    if pages.is_empty() {
                        return Err(SplitPlanError::EmptyRange { ordinal });
                    }
                    Ok((pages, None))
                })
                .collect::<Result<Vec<_>, SplitPlanError>>()?,
            None,
        ),
        SplitRule::Bookmarks(boundaries) => {
            if boundaries.is_empty() {
                return Err(SplitPlanError::NoBookmarks);
            }
            for (ordinal, boundary) in boundaries.iter().enumerate() {
                if boundary.page.get() > total_pages {
                    return Err(SplitPlanError::BookmarkOutOfBounds {
                        ordinal,
                        page: boundary.page,
                    });
                }
                if ordinal > 0 && boundary.page <= boundaries[ordinal - 1].page {
                    return Err(SplitPlanError::BookmarkOrder { ordinal });
                }
            }
            (
                boundaries
                    .iter()
                    .enumerate()
                    .map(|(ordinal, boundary)| {
                        let end = boundaries
                            .get(ordinal + 1)
                            .map_or(total_pages, |next| next.page.get().saturating_sub(1));
                        numbered_range(boundary.page.get(), end)
                            .map(|pages| (pages, Some(boundary.title.clone())))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                None,
            )
        }
        SplitRule::BySize {
            max_bytes,
            page_estimates,
        } => (
            plan_by_size(total_pages, *max_bytes, page_estimates)?,
            Some(*max_bytes),
        ),
    };
    let stem = source_stem(&source);
    let parts = page_groups
        .into_iter()
        .enumerate()
        .map(|(index, (pages, label))| {
            let ordinal = u32::try_from(index + 1).map_err(|_| SplitPlanError::TooManyParts)?;
            Ok(SplitPart {
                ordinal,
                pages,
                label,
                filename_stem: format!("{stem}-{ordinal:03}"),
            })
        })
        .collect::<Result<Vec<_>, SplitPlanError>>()?;
    Ok(SplitPlan {
        source,
        parts,
        size_limit_bytes,
    })
}

fn plan_by_size(
    total_pages: u32,
    max_bytes: NonZeroU64,
    estimates: &[PageSizeEstimate],
) -> Result<Vec<PageGroup>, SplitPlanError> {
    if estimates.len() != usize::try_from(total_pages).unwrap_or(usize::MAX) {
        return Err(SplitPlanError::SizeEstimateCount {
            expected: total_pages,
            actual: estimates.len(),
        });
    }
    let mut groups: Vec<PageGroup> = Vec::new();
    let mut current_pages = Vec::new();
    let mut current_bytes = 0_u64;
    for (ordinal, estimate) in estimates.iter().enumerate() {
        let expected = PageNumber::new(u32::try_from(ordinal + 1).unwrap_or(u32::MAX))
            .map_err(|_| SplitPlanError::SizeEstimateOverflow)?;
        if estimate.page != expected {
            return Err(SplitPlanError::SizeEstimateOrder {
                ordinal,
                expected,
                actual: estimate.page,
            });
        }
        if estimate.estimated_bytes.get() > max_bytes.get() {
            return Err(SplitPlanError::PageExceedsSizeLimit {
                page: estimate.page,
                estimated_bytes: estimate.estimated_bytes,
                max_bytes,
            });
        }
        let next_bytes = current_bytes
            .checked_add(estimate.estimated_bytes.get())
            .ok_or(SplitPlanError::SizeEstimateOverflow)?;
        if !current_pages.is_empty() && next_bytes > max_bytes.get() {
            groups.push((current_pages, None));
            current_pages = Vec::new();
            current_bytes = 0;
        }
        current_pages.push(estimate.page);
        current_bytes = current_bytes
            .checked_add(estimate.estimated_bytes.get())
            .ok_or(SplitPlanError::SizeEstimateOverflow)?;
    }
    if !current_pages.is_empty() {
        groups.push((current_pages, None));
    }
    Ok(groups)
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
    fn size_estimates_greedily_partition_consecutive_pages() {
        let estimates = (1..=5)
            .map(|page| PageSizeEstimate {
                page: PageNumber::new(page).expect("page"),
                estimated_bytes: NonZeroU64::new(40).expect("bytes"),
            })
            .collect();
        let plan = plan_split(
            "book.pdf",
            5,
            &SplitRule::BySize {
                max_bytes: NonZeroU64::new(100).expect("limit"),
                page_estimates: estimates,
            },
        )
        .expect("valid size plan");
        assert_eq!(
            plan.parts
                .iter()
                .map(|part| part.pages.len())
                .collect::<Vec<_>>(),
            vec![2, 2, 1]
        );
        assert_eq!(plan.size_limit_bytes, NonZeroU64::new(100));
    }

    #[test]
    fn size_estimates_reject_missing_or_oversized_pages() {
        let page = PageNumber::new(1).expect("page");
        assert!(matches!(
            plan_split(
                "book.pdf",
                2,
                &SplitRule::BySize {
                    max_bytes: NonZeroU64::new(100).expect("limit"),
                    page_estimates: vec![PageSizeEstimate {
                        page,
                        estimated_bytes: NonZeroU64::new(40).expect("bytes"),
                    }],
                },
            ),
            Err(SplitPlanError::SizeEstimateCount { .. })
        ));
        assert!(matches!(
            plan_split(
                "book.pdf",
                1,
                &SplitRule::BySize {
                    max_bytes: NonZeroU64::new(40).expect("limit"),
                    page_estimates: vec![PageSizeEstimate {
                        page,
                        estimated_bytes: NonZeroU64::new(41).expect("bytes"),
                    }],
                },
            ),
            Err(SplitPlanError::PageExceedsSizeLimit { .. })
        ));
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
    fn bookmark_boundaries_partition_to_document_end_and_retain_labels() {
        let boundaries = vec![
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
        ];
        let plan = plan_split("book.pdf", 4, &SplitRule::Bookmarks(boundaries))
            .expect("valid bookmark plan");
        assert_eq!(
            plan.parts
                .iter()
                .map(|part| part.pages.iter().map(|page| page.get()).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![vec![1, 2], vec![3, 4]]
        );
        assert_eq!(plan.parts[1].label.as_deref(), Some("Chapter 2"));
    }

    #[test]
    fn bookmark_boundaries_must_be_strictly_ascending() {
        let boundaries = vec![
            BookmarkBoundary {
                title: "A".to_owned(),
                page: PageNumber::new(2).expect("page"),
                depth: 0,
            },
            BookmarkBoundary {
                title: "B".to_owned(),
                page: PageNumber::new(2).expect("page"),
                depth: 0,
            },
        ];
        assert!(matches!(
            plan_split("book.pdf", 3, &SplitRule::Bookmarks(boundaries)),
            Err(SplitPlanError::BookmarkOrder { ordinal: 1 })
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
