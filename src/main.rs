use anyhow::{bail, ensure, Result};
use chd312_win11_fix::{
    device,
    ipc::{self, Request, Response, Status},
    system,
};
use std::{path::PathBuf, thread, time::Duration};
#[cfg(feature = "bundle-worker")]
static WORKER: &[u8] = include_bytes!("../target/release/chd312-background.exe");
fn worker() -> Result<PathBuf> {
    #[cfg(not(feature = "bundle-worker"))]
    if let Ok(path) = system::adjacent_worker() {
        return Ok(path);
    }
    #[cfg(feature = "bundle-worker")]
    {
        use sha2::{Digest, Sha256};
        if let Ok(path) = system::adjacent_worker() {
            if std::fs::read(&path)
                .is_ok_and(|bytes| Sha256::digest(&bytes) == Sha256::digest(WORKER))
            {
                return Ok(path);
            }
        }
        let hash = format!("{:x}", Sha256::digest(WORKER));
        let dir = system::data_dir().join("worker").join(hash);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("chd312-background.exe");
        if !std::fs::read(&path).is_ok_and(|bytes| Sha256::digest(&bytes) == Sha256::digest(WORKER))
        {
            let temp = dir.join("worker.tmp");
            std::fs::write(&temp, WORKER)?;
            std::fs::rename(temp, &path)?;
        }
        return Ok(path);
    }
    #[cfg(not(feature = "bundle-worker"))]
    bail!("Background module missing; build both binaries, or use the portable release")
}
fn start() -> Result<()> {
    if ipc::call(&Request::Status).is_ok() {
        return Ok(());
    }
    if system::start_task().is_err() {
        system::start_worker(&worker()?)?;
    }
    for _ in 0..80 {
        thread::sleep(Duration::from_millis(250));
        if ipc::call(&Request::Status).is_ok() {
            return Ok(());
        }
    }
    bail!(
        "Monitor did not start. Check {}",
        system::log_path().display()
    )
}
fn display(response: Response, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", response.message);
        if let Some(s) = &response.status {
            print_status(s);
        }
    }
    ensure!(response.ok, "Command failed");
    Ok(())
}
fn print_status(s: &Status) {
    println!(
        "Monitor: {}  PID: {}  Automatic: {}  Restart required: {}",
        if s.running { "RUNNING" } else { "STOPPED" },
        s.pid,
        if s.automatic { "ON" } else { "PAUSED" },
        s.restart_required
    );
    println!("Checked: {}", s.updated);
    if s.devices.is_empty() {
        println!("Capture device: not connected");
    }
    for c in &s.devices {
        println!("\n{}", c.parent);
        println!(
            "Identity: {}/{} checks passed{}",
            c.checks.iter().filter(|v| v.passed).count(),
            c.checks.len(),
            if c.matches() {
                " (MATCH)"
            } else {
                " (NO CHANGE ALLOWED)"
            }
        );
        for check in c.checks.iter().filter(|v| !v.passed) {
            println!("  FAIL {}: {}", check.name, check.detail);
        }
        for a in &c.audio {
            println!(
                "  {}: {} (problem {}){}",
                a.bus_name,
                a.inf,
                a.problem,
                if a.inf.eq_ignore_ascii_case("usbmidi2.inf") {
                    " -> eligible for audio repair"
                } else {
                    " -> skipped"
                }
            );
        }
    }
    println!("Log: {}", system::log_path().display());
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let json = args.iter().any(|s| s == "--json");
    match cmd {
        "help"|"--help"|"-h"=>println!("CHD312 Windows 11 24H2+ Fix (unofficial) v{}\n\nchd312 status [--json]     Live device/monitor status\nchd312 inspect [--json]    Read-only detailed identity checks\nchd312 start               Start the hidden monitor\nchd312 stop                Stop this session's monitor\nchd312 enable              Enable automatic MIDI-to-audio repair\nchd312 disable             Pause automatic repair (keep audio working)\nchd312 apply [--force]     Repair MIDI bindings now; --force allows a pass while paused\nchd312 restore-original    Pause repair, then restore original MIDI binding (audio may fail)\nchd312 logs                Open the log folder\nchd312 uninstall           Run the installed uninstaller\n\nUSB Audio and unknown drivers are NEVER replaced by apply, even with --force.\nAll identity checks remain mandatory. Portable start may request administrator approval.",env!("CARGO_PKG_VERSION")),
        "status"=>{let response=ipc::call(&Request::Status).unwrap_or_else(|_|Response{ok:true,message:"Monitor is not running. Read-only device status:".into(),status:Some(Status{running:false,pid:0,automatic:system::settings_enabled(),restart_required:false,updated:chrono::Local::now().to_rfc3339(),devices:device::inspect().unwrap_or_default()})});display(response,json)?;},
        "inspect"=>{let captures=device::inspect()?;if json{println!("{}",serde_json::to_string_pretty(&captures)?);}else{for c in &captures{println!("{}\nMATCH: {}",c.parent,c.matches());for check in &c.checks{println!("  {} {}: {}",if check.passed{"PASS"}else{"FAIL"},check.name,check.detail);}}if captures.is_empty(){println!("No matching VID/PID device connected");}}},
        "start"=>{start()?;display(ipc::call(&Request::Status)?,json)?;},
        "stop"=>{if let Ok(r)=ipc::call(&Request::Stop){display(r,json)?;}else{println!("Monitor is already stopped");}},
        "disable"=>{system::set_enabled(false)?;if let Ok(r)=ipc::call(&Request::Disable){display(r,json)?;}else{println!("Automatic repair paused; monitor is not running");}},
        "enable"|"apply"|"restore-original"=>{
            if cmd=="restore-original"{system::set_enabled(false)?;}
            start()?;let request=match cmd{"enable"=>Request::Enable,"apply"=>Request::Apply{force:args.iter().any(|s|s=="--force")},_=>Request::RestoreOriginal};display(ipc::call(&request)?,json)?;
        },
        "logs"=>system::open_logs()?,"uninstall"=>system::uninstall()?,
        "--version"|"version"=>println!("{}",env!("CARGO_PKG_VERSION")),
        _=>bail!("Unknown command '{cmd}'. Run chd312 help."),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
