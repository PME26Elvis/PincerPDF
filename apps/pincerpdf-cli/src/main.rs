#![forbid(unsafe_code)]
//! Minimal internal CLI used to exercise dependency-free application foundations.

use pincerpdf_application::validate_tool_capabilities;
use pincerpdf_domain::{PageNumber, PageSelection, ToolKind};
use pincerpdf_engine_api::{CapabilitySet, PdfCapability};
use std::env;
use std::error::Error;
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
        Some(command) => Err(format!("unknown command: {command}").into()),
    }
}

fn doctor() {
    let available =
        CapabilitySet::from_capabilities([PdfCapability::Inspect, PdfCapability::Merge]);
    let merge_ready = validate_tool_capabilities(ToolKind::Merge, &available).is_ok();
    let rotate_ready = validate_tool_capabilities(ToolKind::Rotate, &available).is_ok();

    println!("pincerpdf.version={}", env!("CARGO_PKG_VERSION"));
    println!("foundation.page_selection=ready");
    println!("foundation.task_state=ready");
    println!("foundation.engine_port=ready");
    println!("sample_engine.merge_ready={merge_ready}");
    println!("sample_engine.rotate_ready={rotate_ready}");
    println!("pdf_engine.selected=false");
    println!("desktop_shell.scaffolded=false");
}

fn print_help() {
    println!("PincerPDF internal CLI");
    println!();
    println!("USAGE:");
    println!("  pincerpdf-cli doctor");
    println!("  pincerpdf-cli selection <EXPRESSION> <TOTAL_PAGES>");
    println!("  pincerpdf-cli --version");
}
