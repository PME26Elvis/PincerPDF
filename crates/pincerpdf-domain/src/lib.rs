#![forbid(unsafe_code)]
//! Pure domain values and state machines for `PincerPDF`.

use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

/// Stable, presentation-independent error categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// User input could not be validated.
    InvalidInput,
    /// The configured PDF engine cannot provide the requested behavior.
    CapabilityUnavailable,
    /// A source PDF cannot be read.
    InputUnreadable,
    /// A password is required but was not supplied.
    PasswordRequired,
    /// A supplied password did not unlock the PDF.
    IncorrectPassword,
    /// The requested output already exists and policy forbids replacement.
    OutputConflict,
    /// Output could not be written or atomically finalized.
    OutputWriteFailed,
    /// A PDF engine failed while processing a request.
    EngineFailure,
    /// A task was cancelled.
    Cancelled,
    /// An invariant failed inside `PincerPDF`.
    Internal,
}

impl ErrorCode {
    /// Returns the stable machine-readable code used by logs and IPC.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::InputUnreadable => "input_unreadable",
            Self::PasswordRequired => "password_required",
            Self::IncorrectPassword => "incorrect_password",
            Self::OutputConflict => "output_conflict",
            Self::OutputWriteFailed => "output_write_failed",
            Self::EngineFailure => "engine_failure",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

/// A one-based PDF page number.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageNumber(NonZeroU32);

impl PageNumber {
    /// Constructs a page number, rejecting zero.
    ///
    /// # Errors
    ///
    /// Returns [`PageNumberError`] when `value` is zero.
    pub fn new(value: u32) -> Result<Self, PageNumberError> {
        NonZeroU32::new(value).map(Self).ok_or(PageNumberError)
    }

    /// Returns the one-based numeric value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Display for PageNumber {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(formatter)
    }
}

/// Error returned when page zero is requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageNumberError;

impl fmt::Display for PageNumberError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PDF page numbers are one-based; zero is invalid")
    }
}

impl Error for PageNumberError {}

/// A segment in a page-selection expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageSpan {
    /// One page.
    Single(PageNumber),
    /// An inclusive bounded range.
    Inclusive {
        /// First selected page.
        start: PageNumber,
        /// Last selected page.
        end: PageNumber,
    },
    /// A range from the given page through the document end.
    From(PageNumber),
}

/// Parsed page-selection expression preserving segment order and duplicates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageSelection(Vec<PageSpan>);

impl PageSelection {
    /// Returns parsed spans in user-specified order.
    #[must_use]
    pub fn spans(&self) -> &[PageSpan] {
        &self.0
    }

    /// Resolves the expression against a concrete PDF page count.
    ///
    /// Segment order and deliberate duplicates are preserved.
    ///
    /// # Errors
    ///
    /// Returns [`ResolveSelectionError`] when any selected page exceeds `total_pages`.
    pub fn resolve(&self, total_pages: u32) -> Result<Vec<PageNumber>, ResolveSelectionError> {
        let mut pages = Vec::new();

        for span in &self.0 {
            match *span {
                PageSpan::Single(page) => {
                    ensure_in_bounds(page, total_pages)?;
                    pages.push(page);
                }
                PageSpan::Inclusive { start, end } => {
                    ensure_in_bounds(end, total_pages)?;
                    pages.extend((start.get()..=end.get()).map(nonzero_page));
                }
                PageSpan::From(start) => {
                    ensure_in_bounds(start, total_pages)?;
                    pages.extend((start.get()..=total_pages).map(nonzero_page));
                }
            }
        }

        Ok(pages)
    }
}

impl FromStr for PageSelection {
    type Err = ParseSelectionError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        if input.trim().is_empty() {
            return Err(ParseSelectionError::new(
                0,
                input,
                ParseSelectionErrorKind::EmptySelection,
            ));
        }

        let spans = input
            .split(',')
            .enumerate()
            .map(|(index, raw)| parse_span(index, raw))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self(spans))
    }
}

fn parse_span(index: usize, raw: &str) -> Result<PageSpan, ParseSelectionError> {
    let segment = raw.trim();
    if segment.is_empty() {
        return Err(ParseSelectionError::new(
            index,
            raw,
            ParseSelectionErrorKind::EmptySegment,
        ));
    }

    let hyphen_count = segment.bytes().filter(|byte| *byte == b'-').count();
    match hyphen_count {
        0 => parse_page(index, segment).map(PageSpan::Single),
        1 => {
            let (start_raw, end_raw) = segment
                .split_once('-')
                .expect("hyphen count was checked before split");
            let start = parse_page(index, start_raw.trim())?;
            let end_raw = end_raw.trim();
            if end_raw.is_empty() {
                return Ok(PageSpan::From(start));
            }
            let end = parse_page(index, end_raw)?;
            if start > end {
                return Err(ParseSelectionError::new(
                    index,
                    segment,
                    ParseSelectionErrorKind::DescendingRange,
                ));
            }
            Ok(PageSpan::Inclusive { start, end })
        }
        _ => Err(ParseSelectionError::new(
            index,
            segment,
            ParseSelectionErrorKind::MalformedRange,
        )),
    }
}

fn parse_page(index: usize, raw: &str) -> Result<PageNumber, ParseSelectionError> {
    if raw.is_empty() {
        return Err(ParseSelectionError::new(
            index,
            raw,
            ParseSelectionErrorKind::MissingRangeStart,
        ));
    }

    let value = raw.parse::<u32>().map_err(|_| {
        ParseSelectionError::new(index, raw, ParseSelectionErrorKind::InvalidNumber)
    })?;
    PageNumber::new(value)
        .map_err(|_| ParseSelectionError::new(index, raw, ParseSelectionErrorKind::ZeroPage))
}

fn ensure_in_bounds(requested: PageNumber, total_pages: u32) -> Result<(), ResolveSelectionError> {
    if requested.get() > total_pages {
        Err(ResolveSelectionError {
            requested,
            total_pages,
        })
    } else {
        Ok(())
    }
}

fn nonzero_page(value: u32) -> PageNumber {
    PageNumber::new(value).expect("resolved page ranges always start at one or greater")
}

/// The reason a page-selection segment failed to parse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseSelectionErrorKind {
    /// The full expression is blank.
    EmptySelection,
    /// A comma introduced a blank segment.
    EmptySegment,
    /// A page token is not an unsigned integer.
    InvalidNumber,
    /// Page zero was specified.
    ZeroPage,
    /// The left side of a range is missing.
    MissingRangeStart,
    /// A range contains more than one separator.
    MalformedRange,
    /// A bounded range ends before it starts.
    DescendingRange,
}

/// Parse failure with the exact zero-based segment index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseSelectionError {
    segment_index: usize,
    segment: String,
    kind: ParseSelectionErrorKind,
}

impl ParseSelectionError {
    fn new(index: usize, segment: &str, kind: ParseSelectionErrorKind) -> Self {
        Self {
            segment_index: index,
            segment: segment.to_owned(),
            kind,
        }
    }

    /// Returns the zero-based comma-separated segment index.
    #[must_use]
    pub const fn segment_index(&self) -> usize {
        self.segment_index
    }

    /// Returns the original segment text.
    #[must_use]
    pub fn segment(&self) -> &str {
        &self.segment
    }

    /// Returns the stable reason category.
    #[must_use]
    pub const fn kind(&self) -> ParseSelectionErrorKind {
        self.kind
    }
}

impl fmt::Display for ParseSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid page-selection segment {} ({:?}): {:?}",
            self.segment_index + 1,
            self.segment,
            self.kind
        )
    }
}

impl Error for ParseSelectionError {}

/// A selection references a page beyond the inspected document size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResolveSelectionError {
    requested: PageNumber,
    total_pages: u32,
}

impl ResolveSelectionError {
    /// Returns the first page that exceeded the document boundary.
    #[must_use]
    pub const fn requested(&self) -> PageNumber {
        self.requested
    }

    /// Returns the inspected document page count.
    #[must_use]
    pub const fn total_pages(&self) -> u32 {
        self.total_pages
    }
}

impl fmt::Display for ResolveSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "page {} is outside a document containing {} pages",
            self.requested, self.total_pages
        )
    }
}

impl Error for ResolveSelectionError {}

/// The eight PDF tool families targeted for parity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ToolKind {
    /// Merge multiple PDFs.
    Merge,
    /// Split after selected pages or fixed page intervals.
    Split,
    /// Split at bookmark levels.
    SplitByBookmarks,
    /// Split while respecting an approximate output-size target.
    SplitBySize,
    /// Interleave pages from two or more sources.
    AlternateMix,
    /// Insert selected pages repeatedly.
    InsertPages,
    /// Extract selected pages.
    Extract,
    /// Rotate selected pages.
    Rotate,
}

/// Durable task lifecycle states shared by CLI, UI and worker adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskState {
    /// Accepted but not yet executing.
    Queued,
    /// Actively executing.
    Running,
    /// Cancellation has been requested and cleanup/finalization is pending.
    Cancelling,
    /// Completed successfully and output was finalized.
    Succeeded,
    /// Stopped because an error occurred.
    Failed,
    /// Stopped without a finalized result because cancellation completed.
    Cancelled,
}

impl TaskState {
    /// Returns whether this state may legally transition to `next`.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Cancelling | Self::Succeeded | Self::Failed
                )
                | (Self::Cancelling, Self::Cancelled | Self::Failed)
        )
    }

    /// Validates and returns the next state.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTaskTransition`] when `next` is not a documented lifecycle transition.
    pub fn transition_to(self, next: Self) -> Result<Self, InvalidTaskTransition> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(InvalidTaskTransition {
                from: self,
                to: next,
            })
        }
    }

    /// Returns whether no further transition is legal.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

/// An attempted task lifecycle transition violated the state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidTaskTransition {
    from: TaskState,
    to: TaskState,
}

impl InvalidTaskTransition {
    /// Returns the current state.
    #[must_use]
    pub const fn from(&self) -> TaskState {
        self.from
    }

    /// Returns the rejected target state.
    #[must_use]
    pub const fn to(&self) -> TaskState {
        self.to
    }
}

impl fmt::Display for InvalidTaskTransition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid task-state transition from {:?} to {:?}",
            self.from, self.to
        )
    }
}

impl Error for InvalidTaskTransition {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_number_rejects_zero() {
        assert_eq!(PageNumber::new(0), Err(PageNumberError));
        assert_eq!(PageNumber::new(1).expect("valid page").get(), 1);
    }

    #[test]
    fn selection_parses_single_bounded_and_open_ranges() {
        let selection: PageSelection = "1, 3-5, 8-".parse().expect("valid selection");
        assert_eq!(
            selection.spans(),
            &[
                PageSpan::Single(PageNumber::new(1).expect("valid")),
                PageSpan::Inclusive {
                    start: PageNumber::new(3).expect("valid"),
                    end: PageNumber::new(5).expect("valid"),
                },
                PageSpan::From(PageNumber::new(8).expect("valid")),
            ]
        );
    }

    #[test]
    fn selection_resolution_preserves_order_and_duplicates() {
        let selection: PageSelection = "3,1-2,3".parse().expect("valid selection");
        let pages = selection.resolve(4).expect("in bounds");
        let values: Vec<u32> = pages.into_iter().map(PageNumber::get).collect();
        assert_eq!(values, vec![3, 1, 2, 3]);
    }

    #[test]
    fn selection_rejects_invalid_segments() {
        for (input, expected) in [
            ("", ParseSelectionErrorKind::EmptySelection),
            ("1,,2", ParseSelectionErrorKind::EmptySegment),
            ("0", ParseSelectionErrorKind::ZeroPage),
            ("4-2", ParseSelectionErrorKind::DescendingRange),
            ("1-2-3", ParseSelectionErrorKind::MalformedRange),
            ("-3", ParseSelectionErrorKind::MissingRangeStart),
            ("abc", ParseSelectionErrorKind::InvalidNumber),
        ] {
            let error = input
                .parse::<PageSelection>()
                .expect_err("input should be invalid");
            assert_eq!(error.kind(), expected, "input: {input}");
        }
    }

    #[test]
    fn selection_rejects_out_of_bounds_pages() {
        let selection: PageSelection = "2-5".parse().expect("valid syntax");
        let error = selection
            .resolve(4)
            .expect_err("page five is out of bounds");
        assert_eq!(error.requested().get(), 5);
        assert_eq!(error.total_pages(), 4);
    }

    #[test]
    fn task_state_machine_accepts_only_documented_transitions() {
        assert_eq!(
            TaskState::Queued.transition_to(TaskState::Running),
            Ok(TaskState::Running)
        );
        assert_eq!(
            TaskState::Running.transition_to(TaskState::Succeeded),
            Ok(TaskState::Succeeded)
        );
        assert!(TaskState::Succeeded.is_terminal());
        assert_eq!(
            TaskState::Succeeded
                .transition_to(TaskState::Running)
                .expect_err("terminal state must not restart")
                .from(),
            TaskState::Succeeded
        );
    }
}
