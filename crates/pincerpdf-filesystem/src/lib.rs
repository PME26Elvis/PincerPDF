#![forbid(unsafe_code)]
//! Filesystem planning primitives for safe PDF output finalization.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

/// Policy applied when the final destination already exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExistingOutputPolicy {
    /// Reject the operation before writing begins.
    Fail,
    /// Permit atomic replacement after successful temporary output validation.
    Replace,
}

/// Pure plan used by adapters to write beside the final destination then rename.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPathPlan {
    /// User-requested final path.
    pub final_path: PathBuf,
    /// Temporary sibling path used while output is incomplete.
    pub temporary_path: PathBuf,
    /// Whether an existing final path may be atomically replaced.
    pub replace_existing: bool,
}

/// Constructs an output path plan without touching the filesystem.
pub fn plan_output_path(
    requested: &Path,
    final_path_exists: bool,
    policy: ExistingOutputPolicy,
    operation_token: &str,
) -> Result<OutputPathPlan, OutputPlanError> {
    let file_name = requested
        .file_name()
        .ok_or(OutputPlanError::MissingFileName)?;

    if operation_token.is_empty()
        || !operation_token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(OutputPlanError::InvalidOperationToken);
    }

    if final_path_exists && policy == ExistingOutputPolicy::Fail {
        return Err(OutputPlanError::ExistingOutputConflict);
    }

    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(".pincerpdf-");
    temporary_name.push(operation_token);
    temporary_name.push(".tmp");
    let temporary_path = requested.with_file_name(temporary_name);

    Ok(OutputPathPlan {
        final_path: requested.to_path_buf(),
        temporary_path,
        replace_existing: final_path_exists && policy == ExistingOutputPolicy::Replace,
    })
}

/// Output planning failure before any file is written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputPlanError {
    /// Destination has no usable file name.
    MissingFileName,
    /// Destination exists and overwrite policy forbids replacement.
    ExistingOutputConflict,
    /// Temporary-file token contains unsafe/non-portable characters.
    InvalidOperationToken,
}

impl fmt::Display for OutputPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFileName => formatter.write_str("output path must include a file name"),
            Self::ExistingOutputConflict => {
                formatter.write_str("output already exists and replacement is disabled")
            }
            Self::InvalidOperationToken => formatter.write_str(
                "operation token must contain only ASCII letters, digits, hyphens or underscores",
            ),
        }
    }
}

impl Error for OutputPlanError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_output_is_a_hidden_sibling_of_the_destination() {
        let plan = plan_output_path(
            Path::new("/tmp/report.pdf"),
            false,
            ExistingOutputPolicy::Fail,
            "task_42",
        )
        .expect("valid plan");

        assert_eq!(plan.final_path, PathBuf::from("/tmp/report.pdf"));
        assert_eq!(
            plan.temporary_path,
            PathBuf::from("/tmp/.report.pdf.pincerpdf-task_42.tmp")
        );
        assert!(!plan.replace_existing);
    }

    #[test]
    fn existing_output_requires_explicit_replace_policy() {
        assert_eq!(
            plan_output_path(
                Path::new("report.pdf"),
                true,
                ExistingOutputPolicy::Fail,
                "task1",
            ),
            Err(OutputPlanError::ExistingOutputConflict)
        );

        let plan = plan_output_path(
            Path::new("report.pdf"),
            true,
            ExistingOutputPolicy::Replace,
            "task1",
        )
        .expect("replacement explicitly allowed");
        assert!(plan.replace_existing);
    }

    #[test]
    fn operation_token_rejects_path_separators_and_spaces() {
        for token in ["", "../escape", "space token", "slash/token"] {
            assert_eq!(
                plan_output_path(
                    Path::new("report.pdf"),
                    false,
                    ExistingOutputPolicy::Fail,
                    token,
                ),
                Err(OutputPlanError::InvalidOperationToken),
                "token: {token}"
            );
        }
    }
}
