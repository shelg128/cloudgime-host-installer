// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(exit_code) = hostcontrolapptauri_lib::run_headless_from_args(&args) {
        std::process::exit(exit_code);
    }

    hostcontrolapptauri_lib::run()
}
