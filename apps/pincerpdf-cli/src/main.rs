#![forbid(unsafe_code)]
//! Internal CLI for exercising verified `PincerPDF` foundations and vertical slices.

use pincerpdf_application::validate_tool_capabilities;
use pincerpdf_domain::{PageNumber, PageSelection, ToolKind};
use pincerpdf_engine_api::{CapabilitySet, InspectOptions, PdfCapability, PdfEnginePort};
use pincerpdf_engine_qpdf::QpdfAdapter;
use pincerpdf_merge::{MergeExecutionOptions, MergeRequest, MergeService, MergeSource};
use pincerpdf_split::{SplitRule, plan_split};
use std::env;
use std::error::Error;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run(env::args().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    match arguments.next().as_deref() {
        None | Some("help" | "--help" | "-h") => {
            print_help();
            Ok(())
        }
        Some("--version" | "-V" | "version") => {
            println!("pincerpdf-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("doctor") => {
            doctor();
            Ok(())
        }
        Some("selection") => {
            let expression = arguments.next().ok_or("selection expression is required")?;
            let total_pages = arguments
                .next()
                .ok_or("total page count is required")?
                .parse::<u32>()?;
            if arguments.next().is_some() {
                return Err("selection accepts exactly two arguments".into());
            }
            let selection = expression.parse::<PageSelection>()?;
            let resolved = selection.resolve(total_pages)?;
            let output = resolved
                .into_iter()
                .map(PageNumber::get)
                .map(|page| page.to_string())
                .collect::<Vec<_>>()
                .join(",");
            println!("{output}");
            Ok(())
        }
        Some("split-plan") => split_plan(arguments),
        Some("split") => split(arguments),
        Some("merge") => merge(arguments),
        Some(command) => Err(format!("unknown command: {command}").into()),
    }
}

fn split(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let output_directory = PathBuf::from(
        arguments
            .next()
            .ok_or("split output directory is required")?,
    );
    let source = PathBuf::from(arguments.next().ok_or("split source path is required")?);
    let rule_text = arguments.next().ok_or("split rule is required")?;
    if arguments.next().is_some() {
        return Err("split accepts exactly three arguments".into());
    }
    let adapter = QpdfAdapter::discover()?;
    let metadata = adapter.inspect(&source, InspectOptions::default())?;
    let control = pincerpdf_merge::ExecutionControl::default();
    let rule = if rule_text.eq_ignore_ascii_case("bookmarks") {
        SplitRule::Bookmarks(adapter.inspect_bookmark_boundaries(&source, &control)?)
    } else if let Some(value) = rule_text.strip_prefix("size:") {
        let max_bytes = value
            .parse::<u64>()
            .ok()
            .and_then(NonZeroU64::new)
            .ok_or("size rule must be size:<positive-bytes>")?;
        let estimates = adapter
            .estimate_page_sizes(&source, metadata.page_count, &control)?
            .estimates;
        SplitRule::BySize {
            max_bytes,
            page_estimates: estimates,
        }
    } else {
        rule_text.parse::<SplitRule>()?
    };
    let plan = plan_split(&source, metadata.page_count, &rule)?;
    let report = adapter.split(&plan, &output_directory, &control)?;
    for output in report.outputs {
        println!("split.output={}", output.display());
    }
    println!("split.parts={}", plan.parts.len());
    Ok(())
}

fn split_plan(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from(
        arguments
            .next()
            .ok_or("split-plan source path is required")?,
    );
    let total_pages = arguments
        .next()
        .ok_or("split-plan total page count is required")?
        .parse::<u32>()?;
    let rule = arguments
        .next()
        .ok_or("split-plan rule is required")?
        .parse::<SplitRule>()?;
    if arguments.next().is_some() {
        return Err("split-plan accepts exactly three arguments".into());
    }
    let plan = plan_split(source, total_pages, &rule)?;
    for part in plan.parts {
        let pages = part
            .pages
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "split.part={} pages={} stem={}",
            part.ordinal, pages, part.filename_stem
        );
    }
    Ok(())
}

fn merge(mut arguments: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    let output = PathBuf::from(arguments.next().ok_or("merge output path is required")?);
    let sources = arguments.map(MergeSource::new).collect::<Vec<_>>();
    let request = MergeRequest::new(sources, output)?;
    let adapter = QpdfAdapter::discover()?;
    let report =
        MergeService::new(&adapter).execute(&request, &MergeExecutionOptions::default())?;
    println!("merge.output={}", report.output.display());
    println!("merge.sources={}", report.source_count);
    println!("merge.pages={}", report.page_count);
    println!(
        "merge.bookmark_sources_discarded={}",
        report.bookmark_sources_discarded
    );
    println!("merge.engine.id={}", report.engine.id);
    println!("merge.engine.version={}", report.engine.version);
    Ok(())
}

fn doctor() {
    let available =
        CapabilitySet::from_capabilities([PdfCapability::Inspect, PdfCapability::Merge]);
    let merge_ready = validate_tool_capabilities(ToolKind::Merge, &available).is_ok();
    let rotate_ready = validate_tool_capabilities(ToolKind::Rotate, &available).is_ok();
    let qpdf_available = QpdfAdapter::discover().is_ok();

    println!("pincerpdf.version={}", env!("CARGO_PKG_VERSION"));
    println!("foundation.page_selection=ready");
    println!("foundation.task_state=ready");
    println!("foundation.engine_port=ready");
    println!("sample_engine.merge_ready={merge_ready}");
    println!("sample_engine.rotate_ready={rotate_ready}");
    println!("pdf_engine.selected={qpdf_available}");
    println!("merge.core_available={qpdf_available}");
    println!("desktop_shell.scaffolded=true");
}

fn print_help() {
    println!("PincerPDF internal CLI");
    println!();
    println!("USAGE:");
    println!("  pincerpdf-cli doctor");
    println!("  pincerpdf-cli selection <EXPRESSION> <TOTAL_PAGES>");
    println!(
        "  pincerpdf-cli split-plan <SOURCE.pdf> <TOTAL_PAGES> <every-page|every:N|RANGE;RANGE>"
    );
    println!(
        "  pincerpdf-cli split <OUTPUT_DIR> <SOURCE.pdf> <bookmarks|size:BYTES|every-page|every:N|RANGE;RANGE>"
    );
    println!("  pincerpdf-cli merge <OUTPUT.pdf> <SOURCE.pdf> <SOURCE.pdf> [SOURCE.pdf ...]");
    println!("  pincerpdf-cli --version");
}
