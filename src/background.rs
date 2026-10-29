#![windows_subsystem = "windows"]
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("watch") => chd312_win11_fix::monitor::watch(),
        None | Some("start") => chd312_win11_fix::system::start_task(),
        _ => Err(anyhow::anyhow!("Unknown background command")),
    };
    if let Err(e) = result {
        chd312_win11_fix::system::log(&format!("BACKGROUND_FATAL {e:#}"));
        std::process::exit(1);
    }
}
