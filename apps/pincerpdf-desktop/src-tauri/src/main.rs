#![forbid(unsafe_code)]
//! Minimal Tauri 2 composition root for the `PincerPDF` desktop shell.

fn main() {
    if let Err(error) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("PincerPDF desktop host failed: {error}");
        std::process::exit(1);
    }
}
