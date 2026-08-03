≠rá^—f•ñÿ¶{^Ïy 'v√Æ∂õ≠#![forbid(unsafe_code)]
//! Real QPDF/MuPDF contract tests for the first Merge-core checkpoint.

use pincerpdf_engine_api::{InspectOptions, PdfEnginePort};
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::{
    BookmarkPolicy, MergeError, MergeExecutionOptions, MergeRequest, MergeService, MergeSource,
    SecretString,
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

fn mutool_value(path: &Path, selector: &str) -> String {
    let output = Command::new("mutool")
        .args(["show"])
        .arg(path)
        .arg(selector)
        .output()
        .expect("mutool show starts");
    assert!(
        output.status.success(),
        "mutool show {selector} stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn qpdf_check(path: &Path) {
    let output = Command::new("qpdf")
        .arg("--check")
        .arg(path)
        .output()
        .expect("qpdf check starts");
    assert!(
        output.status.success(),
        "qpdf check stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn qpdf_outlines(path: &Path) -> serde_json::Value {
    let output = Command::new("qpdf")
        .args(["--json=2", "--json-key=outlines"])
        .arg(path)
        .output()
        .expect("qpdf outline JSON starts");
    assert!(
        output.status.success(),
        "qpdf outline JSON stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid qpdf outline JSON")
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
#[allow(clippy::too_many_lines)]
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

    let footer_output = work.join("filename-footers.pdf");
    let footer_request = MergeRequest::new(
        [
            MergeSource::new(&plain).with_selection("3,1".parse().expect("valid selection")),
            MergeSource::new(&bookmarks).with_selection("2-3".parse().expect("valid selection")),
        ],
        &footer_output,
    )
    .expect("valid filename-footer request");
    let footer_report = service
        .execute(
            &footer_request,
            &MergeExecutionOptions {
                add_filename_footer: true,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("filename footer merge succeeds");
    assert_eq!(footer_report.page_count, 4);
    qpdf_check(&footer_output);
    let footer_text = mutool_text(&footer_output);
    assert!(footer_text.matches("plain-three-pages").count() >= 2);
    assert!(footer_text.matches("bookmarks").count() >= 2);

    let blank_output = work.join("odd-page-blanks.pdf");
    let blank_request = MergeRequest::new(
        [
            MergeSource::new(&plain),
            MergeSource::new(&bookmarks).with_selection("2-3".parse().expect("valid selection")),
        ],
        &blank_output,
    )
    .expect("valid odd-page merge request");
    let blank_report = service
        .execute(
            &blank_request,
            &MergeExecutionOptions {
                add_blank_page_if_odd: true,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("odd-page blank insertion succeeds");
    assert_eq!(blank_report.page_count, 6);
    qpdf_check(&blank_output);
    assert_eq!(
        mutool_value(&blank_output, "pages/4/MediaBox"),
        "[ 0 0 612 792 ]"
    );
    assert_eq!(mutool_value(&blank_output, "pages/4/Rotate"), "90");
    let blank_text = Command::new("mutool")
        .args(["draw", "-q", "-F", "txt", "-o", "-"])
        .arg(&blank_output)
        .arg("4")
        .output()
        .expect("render blank page starts");
    assert!(blank_text.status.success());
    assert!(
        String::from_utf8_lossy(&blank_text.stdout)
            .trim()
            .is_empty()
    );

    let geometry_blank_output = work.join("odd-page-geometry-blanks.pdf");
    let geometry_blank_request = MergeRequest::new(
        [
            MergeSource::new(fixtures.join("geometry-metadata.pdf"))
                .with_selection("1".parse().expect("valid geometry selection")),
            MergeSource::new(&bookmarks).with_selection("2-3".parse().expect("valid selection")),
        ],
        &geometry_blank_output,
    )
    .expect("valid geometry odd-page request");
    let geometry_blank_report = service
        .execute(
            &geometry_blank_request,
            &MergeExecutionOptions {
                add_blank_page_if_odd: true,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("geometry-matched blank insertion succeeds");
    assert_eq!(geometry_blank_report.page_count, 4);
    qpdf_check(&geometry_blank_output);
    assert_eq!(
        mutool_value(&geometry_blank_output, "pages/2/MediaBox"),
        "[ 0 0 300 500 ]"
    );
    assert_eq!(
        mutool_value(&geometry_blank_output, "pages/2/CropBox"),
        "[ 10 20 290 480 ]"
    );
    assert_eq!(
        mutool_value(&geometry_blank_output, "pages/2/Rotate"),
        "null"
    );
    let geometry_blank_text = Command::new("mutool")
        .args(["draw", "-q", "-F", "txt", "-o", "-"])
        .arg(&geometry_blank_output)
        .arg("2")
        .output()
        .expect("render geometry blank page starts");
    assert!(geometry_blank_text.status.success());
    assert!(
        String::from_utf8_lossy(&geometry_blank_text.stdout)
            .trim()
            .is_empty()
    );

    let bookmarked_blank_output = work.join("odd-page-bookmarks.pdf");
    let bookmarked_blank_request = MergeRequest::new(
        [MergeSource::new(&plain), MergeSource::new(&bookmarks)],
        &bookmarked_blank_output,
    )
    .expect("valid odd-page bookmark request");
    service
        .execute(
            &bookmarked_blank_request,
            &MergeExecutionOptions {
                add_blank_page_if_odd: true,
                bookmark_policy: BookmarkPolicy::OneEntryPerDocument,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("bookmarks remain mapped after blank insertion");
    let outlines = qpdf_outlines(&bookmarked_blank_output);
    assert_eq!(outlines["outlines"][0]["destpageposfrom1"], 1);
    assert_eq!(outlines["outlines"][1]["destpageposfrom1"], 5);

    merge_one_entry_per_document(&adapter, &fixtures, &work);
    merge_retained_source_bookmarks(&adapter, &fixtures, &work);
    merge_retained_bookmarks_under_document_entries(&adapter, &fixtures, &work);

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

    merge_parity_preserves_geometry_discards_metadata_and_supports_unicode_long_paths(
        &fixtures, &work,
    );

    if !retain_evidence {
        fs::remove_dir_all(work).expect("remove contract work directory");
    }
}

fn merge_one_entry_per_document(adapter: &QpdfAdapter, fixtures: &Path, work: &Path) {
    let output = work.join("one-entry-bookmarks.pdf");
    let request = MergeRequest::new(
        [
            MergeSource::new(fixtures.join("plain-three-pages.pdf"))
                .with_selection("3".parse().expect("valid selection")),
            MergeSource::new(fixtures.join("bookmarks.pdf"))
                .with_selection("2-3".parse().expect("valid selection")),
        ],
        &output,
    )
    .expect("valid document-bookmark request");
    let report = MergeService::new(adapter)
        .execute(
            &request,
            &MergeExecutionOptions {
                bookmark_policy: BookmarkPolicy::OneEntryPerDocument,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("document-bookmark merge succeeds");

    assert_eq!(report.page_count, 3);
    assert_eq!(report.bookmark_entries, 2);
    assert_eq!(report.bookmark_sources_discarded, 0);
    qpdf_check(&output);
    let outlines = qpdf_outlines(&output);
    assert_eq!(outlines["outlines"].as_array().map(Vec::len), Some(2));
    assert_eq!(outlines["outlines"][0]["title"], "plain-three-pages");
    assert_eq!(outlines["outlines"][0]["destpageposfrom1"], 1);
    assert_eq!(outlines["outlines"][1]["title"], "bookmarks");
    assert_eq!(outlines["outlines"][1]["destpageposfrom1"], 2);
}

fn merge_retained_source_bookmarks(adapter: &QpdfAdapter, fixtures: &Path, work: &Path) {
    let output = work.join("retained-source-bookmarks.pdf");
    let request = MergeRequest::new(
        [
            MergeSource::new(fixtures.join("bookmarks.pdf"))
                .with_selection("2-3".parse().expect("valid selection")),
            MergeSource::new(fixtures.join("plain-three-pages.pdf"))
                .with_selection("1".parse().expect("valid selection")),
        ],
        &output,
    )
    .expect("valid retained-bookmark request");
    let report = MergeService::new(adapter)
        .execute(
            &request,
            &MergeExecutionOptions {
                bookmark_policy: BookmarkPolicy::Retain,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("retained source bookmarks succeed");

    assert_eq!(report.page_count, 3);
    assert_eq!(report.bookmark_entries, 1);
    assert_eq!(report.bookmark_sources_discarded, 0);
    qpdf_check(&output);
    let outlines = qpdf_outlines(&output);
    assert_eq!(outlines["outlines"].as_array().map(Vec::len), Some(1));
    assert_eq!(outlines["outlines"][0]["title"], "Chapter 2");
    assert_eq!(outlines["outlines"][0]["destpageposfrom1"], 1);
    assert_eq!(outlines["outlines"][0]["kids"][0]["title"], "Appendix");
    assert_eq!(outlines["outlines"][0]["kids"][0]["destpageposfrom1"], 2);
}

fn merge_retained_bookmarks_under_document_entries(
    adapter: &QpdfAdapter,
    fixtures: &Path,
    work: &Path,
) {
    let output = work.join("retained-under-document-bookmarks.pdf");
    let request = MergeRequest::new(
        [
            MergeSource::new(fixtures.join("plain-three-pages.pdf"))
                .with_selection("3".parse().expect("valid selection")),
            MergeSource::new(fixtures.join("bookmarks.pdf"))
                .with_selection("2-3".parse().expect("valid selection")),
        ],
        &output,
    )
    .expect("valid grouped-bookmark request");
    let report = MergeService::new(adapter)
        .execute(
            &request,
            &MergeExecutionOptions {
                bookmark_policy: BookmarkPolicy::RetainAsOneEntryPerDocument,
                ..MergeExecutionOptions::default()
            },
        )
        .expect("grouped source bookmarks succeed");

    assert_eq!(report.page_count, 3);
    assert_eq!(report.bookmark_entries, 2);
    assert_eq!(report.bookmark_sources_discarded, 0);
    qpdf_check(&output);
    let outlines = qpdf_outlines(&output);
    assert_eq!(outlines["outlines"].as_array().map(Vec::len), Some(2));
    assert_eq!(outlines["outlines"][0]["title"], "plain-three-pages");
    assert_eq!(outlines["outlines"][0]["destpageposfrom1"], 1);
    assert!(
        outlines["outlines"][0]["kids"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(outlines["outlines"][1]["title"], "bookmarks");
    assert_eq!(outlines["outlines"][1]["destpageposfrom1"], 2);
    assert_eq!(outlines["outlines"][1]["kids"][0]["title"], "Chapter 2");
    assert_eq!(outlines["outlines"][1]["kids"][0]["destpageposfrom1"], 2);
    assert_eq!(
        outlines["outlines"][1]["kids"][0]["kids"][0]["title"],
        "Appendix"
    );
    assert_eq!(
        outlines["outlines"][1]["kids"][0]["kids"][0]["destpageposfrom1"],
        3
    );
}

fn merge_parity_preserves_geometry_discards_metadata_and_supports_unicode_long_paths(
    fixtures: &Path,
    work: &Path,
) {
    let geometry = fixtures.join("geometry-metadata.pdf");
    let plain = fixtures.join("plain-three-pages.pdf");

    let source_directory = work.join(
        "‰æÜÊ∫ê-merge-parity-long-path-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    let output_directory = work.join(
        "Ëº∏Âá∫-merge-parity-long-path-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    fs::create_dir_all(&source_directory).expect("create Unicode long source path");
    fs::create_dir_all(&output_directory).expect("create Unicode long output path");
    let copied_geometry = source_directory.join("Âπæ‰Ωï-metadata-source.pdf");
    fs::copy(&geometry, &copied_geometry).expect("copy geometry fixture to Unicode long path");
    let output = output_directory.join("Âπæ‰Ωï-metadata.pdf");

    let request = MergeRequest::new(
        [
            MergeSource::new(&copied_geometry)
                .with_selection("2,1,3".parse().expect("valid geometry selection")),
            MergeSource::new(&plain)
                .with_selection("2-".parse().expect("valid open-ended selection")),
        ],
        &output,
    )
    .expect("valid parity request");
    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let report = MergeService::new(&adapter)
        .execute(&request, &MergeExecutionOptions::default())
        .expect("geometry merge succeeds");

    assert_eq!(report.page_count, 5);
    assert_eq!(report.bookmark_sources_discarded, 0);
    qpdf_check(&output);
    assert_eq!(mutool_value(&output, "pages/1/MediaBox"), "[ 0 0 842 595 ]");
    assert_eq!(mutool_value(&output, "pages/1/CropBox"), "[ 0 0 800 550 ]");
    assert_eq!(mutool_value(&output, "pages/1/Rotate"), "90");
    assert_eq!(mutool_value(&output, "pages/2/MediaBox"), "[ 0 0 300 500 ]");
    assert_eq!(
        mutool_value(&output, "pages/2/CropBox"),
        "[ 10 20 290 480 ]"
    );
    assert_eq!(mutool_value(&output, "pages/2/Rotate"), "null");
    assert_eq!(
        mutool_value(&output, "pages/3/MediaBox"),
        "[ -10 -20 602 772 ]"
    );
    assert_eq!(mutool_value(&output, "pages/3/CropBox"), "null");
    assert_eq!(mutool_value(&output, "pages/3/Rotate"), "270");
    assert_eq!(mutool_value(&output, "pages/4/MediaBox"), "[ 0 0 595 842 ]");
    assert_eq!(mutool_value(&output, "pages/5/MediaBox"), "[ 0 0 612 792 ]");
    assert_eq!(mutool_value(&output, "pages/5/Rotate"), "90");
    assert_eq!(mutool_value(&output, "trailer/Info"), "null");

    let text = mutool_text(&output);
    let expected = [
        "Geometry landscape rotated",
        "Geometry portrait crop",
        "Geometry offset rotated",
        "Plain page 2",
        "Plain page 3 rotated",
    ];
    let mut previous = 0;
    for marker in expected {
        let position = text[previous..].find(marker).map_or_else(
            || panic!("missing parity page marker {marker:?} in {text:?}"),
            |offset| previous + offset,
        );
        previous = position + marker.len();
    }
}
