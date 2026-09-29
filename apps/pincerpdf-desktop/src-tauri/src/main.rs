#![forbid(unsafe_code)]
//! Tauri 2 composition root for the `PincerPDF` desktop application.

mod merge_commands;
mod split_commands;

use merge_commands::{
    DesktopState, cancel_merge, merge_engine_status, pick_merge_destination, pick_merge_sources,
    run_merge,
};
use split_commands::{cancel_split, pick_split_destination, pick_split_source, run_split};
use std::sync::Arc;

fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(DesktopState::default()))
        .invoke_handler(tauri::generate_handler![
            merge_engine_status,
            pick_merge_sources,
            pick_merge_destination,
            run_merge,
            cancel_merge,
            pick_split_source,
            pick_split_destination,
            run_split,
            cancel_split
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("PincerPDF desktop host failed: {error}");
        std::process::exit(1);
    }
}
