#![forbid(unsafe_code)]
//! Real QPDF split materialization contract.

use pincerpdf_domain::ErrorCode;
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::ExecutionControl;
use pincerpdf_split::{SplitRule, plan_split};
use serde_json::{Value, json};
use std::fs;
use std::num::NonZeroU64;
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
    let base = std::env::var_os("PINCERPDF_SPLIT_EVIDENCE_DIR").map_or_else(
        || std::env::temp_dir().join("pincerpdf-qpdf-split-contract"),
        PathBuf::from,
    );
    base.join(format!(
        "case-{}-{}",
        process::id(),
        NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
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

fn qpdf_outlines(path: &std::path::Path) -> Value {
    let output = Command::new("qpdf")
        .args(["--json=2", "--json-key=outlines"])
        .arg(path)
        .output()
        .expect("qpdf starts");
    assert!(
        output.status.success(),
        "qpdf stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid qpdf outline JSON")
}

fn qpdf_outline_document(path: &std::path::Path) -> Value {
    let output = Command::new("qpdf")
        .args(["--json=2", "--json-key=outlines", "--json-key=qpdf"])
        .arg(path)
        .output()
        .expect("qpdf starts");
    assert!(
        output.status.success(),
        "qpdf stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("valid qpdf outline/object JSON")
}

fn qpdf_outline_object<'a>(document: &'a Value, outline: &Value) -> &'a Value {
    let reference = outline["object"]
        .as_str()
        .expect("outline object reference");
    let key = format!("obj:{reference}");
    document["qpdf"][1]
        .as_object()
        .expect("qpdf object map")
        .get(&key)
        .and_then(|wrapper| wrapper.get("value"))
        .expect("raw outline object")
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
    assert_eq!(report.evidence.len(), 7);
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

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn split_materialization_reconstructs_surviving_bookmarks() {
    let source = fixture_root().join("bookmarks.pdf");
    let work = work_directory();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale split bookmark directory");
    }
    fs::create_dir_all(&work).expect("create split bookmark directory");

    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let boundaries = adapter
        .inspect_bookmark_boundaries(&source, &ExecutionControl::default())
        .expect("inspect source outlines");
    let plan = plan_split(&source, 3, &SplitRule::Bookmarks(boundaries))
        .expect("valid bookmark split plan");
    let report = adapter
        .split(&plan, &work, &ExecutionControl::default())
        .expect("split with reconstructed bookmarks");
    assert_eq!(report.outputs.len(), 2);

    let first = qpdf_outlines(&report.outputs[0]);
    let first_outline = first["outlines"].as_array().expect("first outline array");
    assert_eq!(first_outline.len(), 1);
    assert_eq!(first_outline[0]["title"], "Chapter 1");
    assert_eq!(first_outline[0]["destpageposfrom1"], 1);

    let second = qpdf_outlines(&report.outputs[1]);
    let second_outline = second["outlines"].as_array().expect("second outline array");
    assert_eq!(second_outline.len(), 1);
    assert_eq!(second_outline[0]["title"], "Chapter 2");
    assert_eq!(second_outline[0]["destpageposfrom1"], 1);
    assert_eq!(second_outline[0]["kids"][0]["title"], "Appendix");
    assert_eq!(second_outline[0]["kids"][0]["destpageposfrom1"], 2);
    fs::remove_dir_all(work).expect("clean split bookmark directory");
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn split_materialization_preserves_resolved_destinations_and_presentation() {
    let source = fixture_root().join("outline-safe-metadata.pdf");
    let work = work_directory();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale split metadata directory");
    }
    fs::create_dir_all(&work).expect("create split metadata directory");

    let plan = plan_split(&source, 3, &SplitRule::EveryPage).expect("valid metadata split plan");
    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let report = adapter
        .split(&plan, &work, &ExecutionControl::default())
        .expect("split metadata materialization succeeds");
    assert_eq!(report.outputs.len(), 3);

    let first = qpdf_outline_document(&report.outputs[0]);
    let first_outline = &first["outlines"][0];
    assert_eq!(first_outline["title"], "第一章 — 導言 📄");
    assert_eq!(first_outline["destpageposfrom1"], 1);
    let first_dest = first_outline["dest"].as_array().expect("first destination");
    assert_eq!(first_dest[1], json!("/XYZ"));
    assert_eq!(&first_dest[2..], &[json!(72), json!(720), json!(1.5)]);
    let first_raw = qpdf_outline_object(&first, first_outline);
    assert_eq!(first_raw["/F"], json!(3));
    assert_eq!(first_raw["/C"], json!([0.2, 0.4, 0.6]));

    let second = qpdf_outline_document(&report.outputs[1]);
    let second_root = &second["outlines"][0];
    assert_eq!(second_root["title"], "第一章 — 導言 📄");
    assert_eq!(second_root["destpageposfrom1"], Value::Null);
    assert_eq!(second_root["open"], false);
    let second_root_raw = qpdf_outline_object(&second, second_root);
    assert_eq!(second_root_raw["/Count"], json!(-1));
    assert_eq!(second_root_raw["/F"], json!(3));
    assert_eq!(second_root_raw["/C"], json!([0.2, 0.4, 0.6]));

    let named = &second_root["kids"][0];
    assert_eq!(named["title"], "Named destination");
    assert_eq!(named["destpageposfrom1"], 1);
    let named_dest = named["dest"].as_array().expect("named destination");
    assert_eq!(named_dest[1], json!("/FitH"));
    assert_eq!(named_dest[2], json!(700));
    let named_raw = qpdf_outline_object(&second, named);
    let named_object = named_raw.as_object().expect("named raw object");
    assert!(named_object.get("/A").is_none());
    assert!(named_object.get("/Dest").is_some_and(Value::is_array));
    assert_eq!(named_raw["/F"], json!(1));
    assert_eq!(named_raw["/C"], json!([0, 0.5, 1]));

    let third = qpdf_outline_document(&report.outputs[2]);
    let action = &third["outlines"][0];
    assert_eq!(action["title"], "Local GoTo action");
    assert_eq!(action["destpageposfrom1"], 1);
    let action_dest = action["dest"].as_array().expect("action destination");
    assert_eq!(action_dest[1], json!("/FitV"));
    assert_eq!(action_dest[2], json!(42));
    let action_raw = qpdf_outline_object(&third, action)
        .as_object()
        .expect("action raw object");
    assert!(action_raw.get("/A").is_none());
    assert!(action_raw.get("/Dest").is_some_and(Value::is_array));

    fs::remove_dir_all(work).expect("clean split metadata directory");
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn split_materialization_rejects_non_local_outline_actions() {
    let source = fixture_root().join("outline-unsafe-action.pdf");
    let work = work_directory();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale split unsafe-action directory");
    }
    fs::create_dir_all(&work).expect("create split unsafe-action directory");

    let plan = plan_split(&source, 1, &SplitRule::EveryPage).expect("valid action split plan");
    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let error = adapter
        .split(&plan, &work, &ExecutionControl::default())
        .expect_err("non-local outline action must fail closed");
    assert_eq!(error.code(), ErrorCode::CapabilityUnavailable);
    assert!(error.to_string().contains("local GoTo"));
    assert_eq!(
        fs::read_dir(&work)
            .expect("read split unsafe-action directory")
            .count(),
        0
    );

    fs::remove_dir_all(work).expect("clean split unsafe-action directory");
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn qpdf_outline_boundaries_feed_bookmark_split_planner() {
    let source = fixture_root().join("bookmarks.pdf");
    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let boundaries = adapter
        .inspect_bookmark_boundaries(&source, &ExecutionControl::default())
        .expect("inspect top-level bookmark boundaries");
    assert_eq!(boundaries.len(), 2);
    assert_eq!(boundaries[0].title, "Chapter 1");
    assert_eq!(boundaries[0].depth, 0);
    assert_eq!(boundaries[1].page.get(), 2);
    let plan = plan_split(&source, 3, &SplitRule::Bookmarks(boundaries))
        .expect("bookmark boundaries produce a split plan");
    assert_eq!(plan.parts.len(), 2);
    assert_eq!(plan.parts[0].pages.len(), 1);
    assert_eq!(plan.parts[1].pages.len(), 2);

    let nested = adapter
        .inspect_bookmark_boundaries_at_depth(&source, 1, &ExecutionControl::default())
        .expect("inspect nested bookmark boundaries");
    assert_eq!(nested.len(), 1);
    assert_eq!(nested[0].title, "Appendix");
    assert_eq!(nested[0].page.get(), 3);
    assert_eq!(nested[0].depth, 1);
    let nested_plan = plan_split(&source, 3, &SplitRule::Bookmarks(nested))
        .expect("nested bookmark boundary produces a split plan");
    assert_eq!(nested_plan.parts.len(), 1);
    assert_eq!(nested_plan.parts[0].pages.len(), 1);
}

#[test]
#[ignore = "requires pinned qpdf/mutool and generated PDF fixtures"]
fn qpdf_page_size_estimates_feed_size_split_and_bound_outputs() {
    let source = fixture_root().join("plain-three-pages.pdf");
    let work = work_directory();
    if work.exists() {
        fs::remove_dir_all(&work).expect("remove stale split contract directory");
    }
    fs::create_dir_all(&work).expect("create split contract directory");

    let adapter = QpdfAdapter::discover().expect("discover qpdf");
    let estimate_report = adapter
        .estimate_page_sizes(&source, 3, &ExecutionControl::default())
        .expect("estimate page sizes");
    assert_eq!(estimate_report.estimates.len(), 3);
    assert_eq!(estimate_report.evidence.len(), 3);
    let max_bytes = estimate_report
        .estimates
        .iter()
        .map(|estimate| estimate.estimated_bytes.get())
        .max()
        .and_then(NonZeroU64::new)
        .expect("nonzero estimate limit");
    let plan = plan_split(
        &source,
        3,
        &SplitRule::BySize {
            max_bytes,
            page_estimates: estimate_report.estimates,
        },
    )
    .expect("size estimates produce a plan");
    let report = adapter
        .split(&plan, &work, &ExecutionControl::default())
        .expect("size-bounded split materialization succeeds");
    assert_eq!(report.outputs.len(), plan.parts.len());
    for output in &report.outputs {
        assert!(
            fs::metadata(output).expect("output metadata").len() <= max_bytes.get(),
            "size-bounded output exceeded the estimate limit: {}",
            output.display()
        );
    }
    fs::remove_dir_all(work).expect("clean split contract directory");
}
