#![forbid(unsafe_code)]
//! Real QPDF/MuPDF contract tests for the first Merge-core checkpoint.

use pincerpdf_engine_api::{InspectOptions, PdfEnginePort};
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::{
    MergeError, MergeExecutionOptions, MergeRequest, MergeService, MergeSource, SecretString,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

fn unique_directory() -> PathBuf {
    std::env::var_os("PINCERPDF_MERGE_EVIDENCE_DIR").map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "pincerpdf-qpdf-contract-{}-{}",
                process::id(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
            ))
        },
        PathBuf::from,
    )
}

fn fixture_root() -> PathBuf {
    PathBuf::from(
        std::env::var("PINCERPDF_PDF_FIXTURES")
            .expect("PINCERPDF_PDF_FIXTURES must point at generated fixtures"),
    )
}

fn mutool_text(path: &Path) -> String {
    let output = Command::new("mutool")
        .args(["draw", "-q", "-F", "txt", "-o", "-"])
        .arg(path)
        .output()
        .expect("mutool starts");
    assert!(
        output.status.success(),
        "mutool stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn merge_core_preserves_order_rejects_forms_and_redacts_passwords() {
    let fixtures = fixture_root();
    let plain = fixtures.join("plain-three-pages.pdf");
    let bookmarks = fixtures.join("bookmarks.pdf");
    let form = fixtures.join("acroform.pdf");
    let work = unique_directory();
    let retain_evidence = std::env::var_os("PINCERPDF_MERGE_EVIDENCE_DIR").is_some();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale contract work directory");
    }
    fs::create_dir_all(&work).expect("create contract work directory");

    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let service = MergeService::new(&adapter);
    let output = work.join("ordered.pdf");
    let request = MergeRequest::new(
        [
            MergeSource::new(&plain).with_selection("3,1,3".parse().expect("valid selection")),
            MergeSource::new(&bookmarks).with_selection("2-3".parse().expect("valid selection")),
        ],
        &output,
    )
    .expect("valid merge request");
    let report = service
        .execute(&request, &MergeExecutionOptions::default())
        .expect("ordered merge succeeds");
    assert_eq!(report.page_count, 5);
    assert_eq!(report.bookmark_sources_discarded, 1);

    let metadata = adapter
        .inspect(&output, InspectOptions::default())
        .expect("inspect merged output");
    assert_eq!(metadata.page_count, 5);
    assert!(!metadata.has_bookmarks);
    assert!(!metadata.has_forms);

    let text = mutool_text(&output);
    let expected = [
        "Plain page 3 rotated",
        "Plain page 1",
        "Plain page 3 rotated",
        "Bookmark chapter 2",
        "Bookmark appendix",
    ];
    let mut previous = 0;
    for marker in expected {
        let position = text[previous..].find(marker).map_or_else(
            || panic!("missing ordered page marker {marker:?} in {text:?}"),
            |offset| previous + offset,
        );
        previous = position + marker.len();
    }

    let forms_output = work.join("forms.pdf");
    let forms_request = MergeRequest::new(
        [MergeSource::new(&form), MergeSource::new(&plain)],
        &forms_output,
    )
    .expect("valid request shape");
    assert!(matches!(
        service.execute(&forms_request, &MergeExecutionOptions::default()),
        Err(MergeError::FormsUnsupported { source_index: 0 })
    ));
    assert!(!forms_output.exists());

    let encrypted = work.join("encrypted.pdf");
    let encrypted_status = Command::new("qpdf")
        .args(["--encrypt", "p4-secret", "p4-owner", "256", "--"])
        .arg(&plain)
        .arg(&encrypted)
        .status()
        .expect("qpdf encryption starts");
    assert!(encrypted_status.success());

    let encrypted_output = work.join("encrypted-merge.pdf");
    let encrypted_request = MergeRequest::new(
        [
            MergeSource::new(&encrypted)
                .with_password(SecretString::new("p4-secret").expect("valid password")),
            MergeSource::new(&plain),
        ],
        &encrypted_output,
    )
    .expect("valid encrypted request");
    let encrypted_report = service
        .execute(&encrypted_request, &MergeExecutionOptions::default())
        .expect("encrypted merge succeeds");
    let evidence = format!("{:?}", encrypted_report.evidence);
    assert!(!evidence.contains("p4-secret"));
    assert!(!evidence.contains("p4-owner"));
    assert!(evidence.contains("redacted"));
    assert_eq!(encrypted_report.page_count, 6);

    if !retain_evidence {
        fs::remove_dir_all(work).expect("remove contract work directory");
    }
}
