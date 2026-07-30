#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
//! Process-isolated QPDF adapter for proven `PincerPDF` capabilities.

use pincerpdf_domain::{ErrorCode, PageNumber};
use pincerpdf_engine_api::{
    CapabilitySet, EngineError, EngineIdentity, InspectOptions, PdfCapability, PdfEnginePort,
    PdfMetadata,
};
use pincerpdf_merge::{
    CancellationToken, CommandEvidence, ExecutionControl, MergeEnginePort, MergeEngineRequest,
    MergeEngineResult, SecretString,
};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

/// Configuration for external QPDF execution.
#[derive(Clone, Debug)]
pub struct QpdfConfig {
    /// QPDF executable name or path.
    pub executable: PathBuf,
    /// Limits used for discovery and inspection commands.
    pub inspection_control: ExecutionControl,
}

impl Default for QpdfConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("qpdf"),
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
        password_supplied: bool,
    ) -> Result<ProcessCapture, EngineError> {
        run_process(&self.config.executable, args, display_args, control)
            .map_err(|failure| map_process_failure(&failure, password_supplied))
    }
}

impl PdfEnginePort for QpdfAdapter {
    fn identity(&self) -> EngineIdentity {
        self.identity.clone()
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_capabilities([
            PdfCapability::Inspect,
            PdfCapability::Merge,
            PdfCapability::Encryption,
        ])
    }

    fn inspect(
        &self,
        source: &Path,
        options: InspectOptions<'_>,
    ) -> Result<PdfMetadata, EngineError> {
        if !source.is_file() {
            return Err(EngineError::new(
                ErrorCode::InputUnreadable,
                format!("PDF source is not a readable file: {}", source.display()),
            ));
        }
        let secret = options
            .password
            .map(SecretString::new)
            .transpose()
            .map_err(|error| EngineError::new(ErrorCode::InvalidInput, error.to_string()))?;
        let password_file = secret
            .as_ref()
            .map(PasswordFile::create)
            .transpose()
            .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;

        let (password_args, password_display) = password_arguments(password_file.as_ref());
        let mut page_args = password_args.clone();
        page_args.push(OsString::from("--show-npages"));
        page_args.push(source.as_os_str().to_os_string());
        let mut page_display = password_display.clone();
        page_display.push("--show-npages".to_owned());
        page_display.push(source.display().to_string());
        let page_capture = self.run_qpdf(
            &page_args,
            page_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let page_count = page_capture
            .evidence
            .stdout
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                EngineError::new(
                    ErrorCode::EngineFailure,
                    "qpdf returned a non-integer page count",
                )
            })?;

        let mut encryption_args = password_args.clone();
        encryption_args.push(OsString::from("--show-encryption"));
        encryption_args.push(source.as_os_str().to_os_string());
        let mut encryption_display = password_display.clone();
        encryption_display.push("--show-encryption".to_owned());
        encryption_display.push(source.display().to_string());
        let encryption_capture = self.run_qpdf(
            &encryption_args,
            encryption_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let encrypted = !encryption_capture
            .evidence
            .stdout
            .to_ascii_lowercase()
            .contains("file is not encrypted");

        let qdf_path = TemporaryPath::new("inspect-qdf", "pdf").map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot create private inspection directory: {error}"),
            )
        })?;
        let mut qdf_args = password_args;
        qdf_args.extend([
            OsString::from("--qdf"),
            OsString::from("--object-streams=disable"),
            source.as_os_str().to_os_string(),
            qdf_path.path().as_os_str().to_os_string(),
        ]);
        let mut qdf_display = password_display;
        qdf_display.extend([
            "--qdf".to_owned(),
            "--object-streams=disable".to_owned(),
            source.display().to_string(),
            qdf_path.path().display().to_string(),
        ]);
        self.run_qpdf(
            &qdf_args,
            qdf_display,
            &self.config.inspection_control,
            secret.is_some(),
        )?;
        let qdf = fs::read(qdf_path.path()).map_err(|error| {
            EngineError::new(
                ErrorCode::EngineFailure,
                format!("cannot read normalized QPDF output: {error}"),
            )
        })?;

        Ok(PdfMetadata {
            page_count,
            encrypted,
            pdf_version: read_pdf_version(source),
            has_bookmarks: qdf_catalog_has_key(&qdf, "/Outlines"),
            has_forms: qdf_catalog_has_key(&qdf, "/AcroForm"),
        })
    }
}

impl MergeEnginePort for QpdfAdapter {
    fn merge(
        &self,
        request: &MergeEngineRequest,
        control: &ExecutionControl,
    ) -> Result<MergeEngineResult, EngineError> {
        if request.inputs.len() < 2 {
            return Err(EngineError::new(
                ErrorCode::InvalidInput,
                "QPDF merge requires at least two inputs",
            ));
        }
        let mut evidence = Vec::new();
        let mut prepared = Vec::with_capacity(request.inputs.len());

        for input in &request.inputs {
            if input.pages.is_empty() {
                return Err(EngineError::new(
                    ErrorCode::InvalidInput,
                    "QPDF merge input resolved to zero pages",
                ));
            }
            if let Some(password) = input.password.as_ref() {
                let password_file = PasswordFile::create(password)
                    .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;
                let decrypted = TemporaryPath::new("decrypted-source", "pdf").map_err(|error| {
                    EngineError::new(
                        ErrorCode::Internal,
                        format!("cannot create private decryption directory: {error}"),
                    )
                })?;
                let args = vec![
                    password_file.argument(),
                    OsString::from("--decrypt"),
                    input.source.as_os_str().to_os_string(),
                    decrypted.path().as_os_str().to_os_string(),
                ];
                let display_args = vec![
                    "--password-file=<redacted>".to_owned(),
                    "--decrypt".to_owned(),
                    input.source.display().to_string(),
                    decrypted.path().display().to_string(),
                ];
                let capture = self.run_qpdf(&args, display_args, control, true)?;
                evidence.push(capture.evidence);
                prepared.push(PreparedSource {
                    path: decrypted.path().to_path_buf(),
                    pages: input.pages.clone(),
                    _password_file: Some(password_file),
                    _decrypted: Some(decrypted),
                });
            } else {
                prepared.push(PreparedSource {
                    path: input.source.clone(),
                    pages: input.pages.clone(),
                    _password_file: None,
                    _decrypted: None,
                });
            }
        }

        let mut args = vec![OsString::from("--empty"), OsString::from("--pages")];
        let mut display_args = vec!["--empty".to_owned(), "--pages".to_owned()];
        for source in &prepared {
            let page_spec = page_specification(&source.pages);
            args.push(source.path.as_os_str().to_os_string());
            args.push(OsString::from(&page_spec));
            display_args.push(source.path.display().to_string());
            display_args.push(page_spec);
        }
        args.push(OsString::from("--"));
        args.push(request.output.as_os_str().to_os_string());
        display_args.push("--".to_owned());
        display_args.push(request.output.display().to_string());
        let merge_capture = self.run_qpdf(&args, display_args, control, false)?;
        evidence.push(merge_capture.evidence);

        let page_capture = self.run_qpdf(
            &[
                OsString::from("--show-npages"),
                request.output.as_os_str().to_os_string(),
            ],
            vec![
                "--show-npages".to_owned(),
                request.output.display().to_string(),
            ],
            control,
            false,
        )?;
        let page_count = page_capture
            .evidence
            .stdout
            .trim()
            .parse::<u32>()
            .map_err(|_| {
                EngineError::new(
                    ErrorCode::EngineFailure,
                    "qpdf returned a non-integer merged page count",
                )
            })?;
        evidence.push(page_capture.evidence);
        Ok(MergeEngineResult {
            page_count,
            evidence,
        })
    }
}

fn password_arguments(password_file: Option<&PasswordFile>) -> (Vec<OsString>, Vec<String>) {
    password_file.map_or_else(
        || (Vec::new(), Vec::new()),
        |file| {
            (
                vec![file.argument()],
                vec!["--password-file=<redacted>".to_owned()],
            )
        },
    )
}

fn page_specification(pages: &[PageNumber]) -> String {
    pages
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn qdf_catalog_has_key(qdf: &[u8], key: &str) -> bool {
    String::from_utf8_lossy(qdf)
        .split("endobj")
        .any(|object| object.contains("/Type /Catalog") && object.contains(key))
}

fn read_pdf_version(source: &Path) -> Option<String> {
    let mut file = File::open(source).ok()?;
    let mut header = [0_u8; 16];
    let read = file.read(&mut header).ok()?;
    let header = std::str::from_utf8(&header[..read]).ok()?;
    header
        .strip_prefix("%PDF-")
        .and_then(|rest| rest.lines().next())
        .map(str::trim)
        .map(ToOwned::to_owned)
}

struct PreparedSource {
    path: PathBuf,
    pages: Vec<PageNumber>,
    _password_file: Option<PasswordFile>,
    _decrypted: Option<TemporaryPath>,
}

struct PasswordFile {
    temporary: TemporaryPath,
}

impl PasswordFile {
    fn create(secret: &SecretString) -> io::Result<Self> {
        let temporary = TemporaryPath::new("password", "txt")?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(temporary.path())?;
        file.write_all(secret.expose_secret().as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(Self { temporary })
    }

    fn argument(&self) -> OsString {
        let mut argument = OsString::from("--password-file=");
        argument.push(self.temporary.path());
        argument
    }
}

struct TemporaryPath {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryPath {
    fn new(purpose: &str, extension: &str) -> io::Result<Self> {
        for _ in 0..32 {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("pincerpdf-{purpose}-{}-{id}", std::process::id()));
            #[cfg(unix)]
            let builder = {
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder
            };
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            match builder.create(&directory) {
                Ok(()) => {
                    let path = directory.join(format!("payload.{extension}"));
                    return Ok(Self { directory, path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a private PincerPDF temporary directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}

struct BoundedText {
    text: String,
    truncated: bool,
}

#[derive(Debug)]
struct ProcessCapture {
    evidence: CommandEvidence,
}

#[derive(Clone, Copy, Debug)]
enum ProcessFailureKind {
    Spawn,
    Cancelled,
    TimedOut,
    Exit,
    Join,
}

#[derive(Debug)]
struct ProcessFailure {
    kind: ProcessFailureKind,
    evidence: Box<CommandEvidence>,
    message: String,
}

fn run_process(
    program: &Path,
    args: &[OsString],
    display_args: Vec<String>,
    control: &ExecutionControl,
) -> Result<ProcessCapture, ProcessFailure> {
    let started = Instant::now();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ProcessFailure {
            kind: ProcessFailureKind::Spawn,
            evidence: Box::new(empty_evidence(
                program,
                display_args.clone(),
                started.elapsed(),
            )),
            message: format!("cannot start PDF engine: {error}"),
        })?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let limit = control.output_limit_bytes();
    let stdout_reader = thread::spawn(move || read_bounded(stdout, limit));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, limit));

    let mut forced_kind = None;
    let status = loop {
        if control.cancellation().is_cancelled() {
            forced_kind = Some(ProcessFailureKind::Cancelled);
            let _ = child.kill();
            break child.wait();
        }
        if started.elapsed() >= control.timeout() {
            forced_kind = Some(ProcessFailureKind::TimedOut);
            let _ = child.kill();
            break child.wait();
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => break Err(error),
        }
    };

    let status = status.map_err(|error| ProcessFailure {
        kind: ProcessFailureKind::Exit,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: format!("cannot wait for PDF engine: {error}"),
    })?;
    let stdout = stdout_reader.join().map_err(|_| ProcessFailure {
        kind: ProcessFailureKind::Join,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: "PDF engine stdout reader panicked".to_owned(),
    })?;
    let stderr = stderr_reader.join().map_err(|_| ProcessFailure {
        kind: ProcessFailureKind::Join,
        evidence: Box::new(empty_evidence(
            program,
            display_args.clone(),
            started.elapsed(),
        )),
        message: "PDF engine stderr reader panicked".to_owned(),
    })?;
    let evidence = evidence(
        program,
        display_args,
        status,
        started.elapsed(),
        stdout,
        stderr,
    );

    if let Some(kind) = forced_kind {
        let message = match kind {
            ProcessFailureKind::Cancelled => "PDF engine operation was cancelled",
            ProcessFailureKind::TimedOut => "PDF engine operation timed out",
            _ => "PDF engine operation was interrupted",
        };
        return Err(ProcessFailure {
            kind,
            evidence: Box::new(evidence),
            message: message.to_owned(),
        });
    }
    if status.success() {
        Ok(ProcessCapture { evidence })
    } else {
        Err(ProcessFailure {
            kind: ProcessFailureKind::Exit,
            message: if evidence.stderr.is_empty() {
                "PDF engine exited unsuccessfully".to_owned()
            } else {
                format!("PDF engine failed: {}", evidence.stderr)
            },
            evidence: Box::new(evidence),
        })
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> BoundedText {
    let mut retained = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = limit.saturating_sub(retained.len());
                let keep = remaining.min(read);
                retained.extend_from_slice(&buffer[..keep]);
                truncated |= keep < read;
            }
        }
    }
    BoundedText {
        text: String::from_utf8_lossy(&retained).trim().to_owned(),
        truncated,
    }
}

fn empty_evidence(program: &Path, arguments: Vec<String>, duration: Duration) -> CommandEvidence {
    CommandEvidence {
        program: program.display().to_string(),
        arguments,
        exit_code: None,
        duration_ms: duration.as_millis(),
        stdout: String::new(),
        stderr: String::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    }
}

fn evidence(
    program: &Path,
    arguments: Vec<String>,
    status: ExitStatus,
    duration: Duration,
    stdout: BoundedText,
    stderr: BoundedText,
) -> CommandEvidence {
    CommandEvidence {
        program: program.display().to_string(),
        arguments,
        exit_code: status.code(),
        duration_ms: duration.as_millis(),
        stdout: stdout.text,
        stderr: stderr.text,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    }
}

fn map_process_failure(failure: &ProcessFailure, password_supplied: bool) -> EngineError {
    let combined =
        format!("{} {}", failure.evidence.stdout, failure.evidence.stderr).to_ascii_lowercase();
    let code = match failure.kind {
        ProcessFailureKind::Cancelled => ErrorCode::Cancelled,
        ProcessFailureKind::Spawn | ProcessFailureKind::TimedOut | ProcessFailureKind::Join => {
            ErrorCode::EngineFailure
        }
        ProcessFailureKind::Exit
            if combined.contains("invalid password")
                || combined.contains("incorrect password")
                || combined.contains("password is incorrect") =>
        {
            ErrorCode::IncorrectPassword
        }
        ProcessFailureKind::Exit if combined.contains("password") && !password_supplied => {
            ErrorCode::PasswordRequired
        }
        ProcessFailureKind::Exit => ErrorCode::EngineFailure,
    };
    let command = format!(
        "{} {}",
        failure.evidence.program,
        failure.evidence.arguments.join(" ")
    );
    EngineError::new(code, format!("{}; command: {command}", failure.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use pincerpdf_merge::CancellationToken;

    #[cfg(unix)]
    #[test]
    fn bounded_capture_drains_but_retains_only_the_configured_limit() {
        let control =
            ExecutionControl::new(Duration::from_secs(2), 5, CancellationToken::default());
        let capture = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("printf 1234567890")],
            vec!["-c".to_owned(), "printf <test-data>".to_owned()],
            &control,
        )
        .expect("process succeeds");
        assert_eq!(capture.evidence.stdout, "12345");
        assert!(capture.evidence.stdout_truncated);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_the_child_process() {
        let control = ExecutionControl::new(
            Duration::from_millis(40),
            1024,
            CancellationToken::default(),
        );
        let failure = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("sleep 1")],
            vec!["-c".to_owned(), "sleep 1".to_owned()],
            &control,
        )
        .expect_err("process must time out");
        assert!(matches!(failure.kind, ProcessFailureKind::TimedOut));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_kills_the_child_process() {
        let token = CancellationToken::default();
        let cancellation = token.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            cancellation.cancel();
        });
        let control = ExecutionControl::new(Duration::from_secs(2), 1024, token);
        let failure = run_process(
            Path::new("sh"),
            &[OsString::from("-c"), OsString::from("sleep 1")],
            vec!["-c".to_owned(), "sleep 1".to_owned()],
            &control,
        )
        .expect_err("process must be cancelled");
        handle.join().expect("cancellation thread joins");
        assert!(matches!(failure.kind, ProcessFailureKind::Cancelled));
    }

    #[test]
    fn page_specification_preserves_order_and_duplicates() {
        let pages = [
            PageNumber::new(3).expect("valid"),
            PageNumber::new(1).expect("valid"),
            PageNumber::new(3).expect("valid"),
        ];
        assert_eq!(page_specification(&pages), "3,1,3");
    }
}
