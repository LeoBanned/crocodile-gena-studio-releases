#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod update_core;
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 2 {
        let stage = std::path::PathBuf::from(&args[0]);
        let Some(expected) = args[1]
            .to_str()
            .filter(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
        else {
            return;
        };
        if let Err(error) = update_core::run(&stage, expected) {
            if let Ok(plan) = update_core::read_plan(&stage) {
                update_core::outcome(&stage, &plan, false, &error);
            }
        }
    }
}
