#![forbid(unsafe_code)]
//! Real QPDF split materialization contract.

use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::ExecutionControl;
use pincerpdf_split::{SplitRule, plan_split};
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

fn fixture_root() -> PathBuf {
    PathBuf::from(
        std::env::var("PINCERPDF_PDF_FIXTURES")
            .expect("PINCERPDF_PDF_FIXTURES must point at generated fixtures"),
    )
}

fn work_directory() -> PathBuf {
    std::env::var_os("PINCERPDF_SPLIT_EVIDENCE_DIR").map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "pincerpdf-qpdf-split-contract-{}-{}",
                process::id(),
                NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
            ))
        },
        PathBuf::from,
    )
}

fn qpdf_page_count(path: &std::path::Path) -> u32 {
    let output = Command::new("qpdf")
        .args(["--show-npages"])
        .arg(path)
        .output()
        .expect("qpdf starts");
    assert!(
        output.status.success(),
        "qpdf stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .expect("integer qpdf page count")
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn split_materialization_conserves_pages_and_finalizes_outputs() {
    let source = fixture_root().join("plain-three-pages.pdf");
    let work = work_directory();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale split contract directory");
    }
    fs::create_dir_all(&work).expect("create split contract directory");

    let plan = plan_split(&source, 3, &SplitRule::EveryPage).expect("valid split plan");
    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let report = adapter
        .split(&plan, &work, &ExecutionControl::default())
        .expect("split materialization succeeds");

    assert_eq!(report.outputs.len(), 3);
    assert_eq!(report.evidence.len(), 6);
    for (ordinal, output) in report.outputs.iter().enumerate() {
        assert!(
            output.is_file(),
            "missing split output {}",
            output.display()
        );
        assert_eq!(qpdf_page_count(output), 1);
        assert!(output.file_name().is_some_and(|name| {
            name.to_string_lossy()
                .contains(&format!("-{:03}", ordinal + 1))
        }));
    }
    assert_eq!(
        fs::read_dir(&work).expect("read split directory").count(),
        3
    );
    fs::remove_dir_all(work).expect("clean split contract directory");
}
