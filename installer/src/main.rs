#![windows_subsystem = "windows"]
use anyhow::{ensure, Context, Result};
use chd312_win11_fix::{
    device::Handle,
    ipc::{self, Request},
    system,
};
use std::{
    fs,
    os::windows::{fs::MetadataExt, process::CommandExt},
    path::Path,
    process::Command,
};
use windows_sys::Win32::{System::Threading::*, UI::WindowsAndMessaging::*};
const CLI: &[u8] = include_bytes!("../../target/release/chd312.exe");
const BACKGROUND: &[u8] = include_bytes!("../../target/release/chd312-background.exe");
const README: &str = include_str!("../../README.md");
fn powershell(script: &str) -> Result<()> {
    let output = system::hidden_command("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "Windows registration failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
fn stop_monitor() {
    if let Ok(status) = ipc::call(&Request::Status) {
        let pid = status.status.map(|s| s.pid);
        let _ = ipc::call(&Request::Stop);
        if let Some(pid) = pid {
            unsafe {
                let h = Handle(OpenProcess(0x00100000, 0, pid));
                if !h.0.is_null() {
                    WaitForSingleObject(h.0, 10_000);
                }
            }
        }
    }
}
fn validate_dir(path: &Path) -> Result<()> {
    ensure!(
        path == system::installed_dir()?,
        "Unexpected installation directory"
    );
    if path.exists() {
        ensure!(
            fs::symlink_metadata(path)?.file_attributes() & 0x400 == 0,
            "Installation directory is a reparse point; refusing changes"
        );
    }
    Ok(())
}
fn install() -> Result<()> {
    let dir = system::installed_dir()?;
    validate_dir(&dir)?;
    stop_monitor();
    fs::create_dir_all(&dir)?;
    for (name, bytes) in [("chd312.exe", CLI), ("chd312-background.exe", BACKGROUND)] {
        let path = dir.join(name);
        if path.exists() {
            ensure!(
                fs::symlink_metadata(&path)?.file_attributes() & 0x400 == 0,
                "Unexpected reparse-point file"
            );
        }
        fs::write(path, bytes)?;
    }
    let uninstaller = dir.join("Uninstall.exe");
    let self_exe = std::env::current_exe()?;
    if self_exe != uninstaller {
        fs::copy(self_exe, &uninstaller)?;
    }
    fs::write(dir.join("README.txt"), README)?;
    powershell(include_str!("../install.ps1"))?;
    Ok(())
}
fn uninstall() -> Result<()> {
    let dir = system::installed_dir()?;
    validate_dir(&dir)?;
    stop_monitor();
    powershell(include_str!("../uninstall.ps1"))?;
    // Defer only this fixed set of files until our own process has exited. No recursive file deletion.
    let literal = dir.to_string_lossy().replace('\'', "''");
    let pid = std::process::id();
    let cleanup = format!(
        r#"$ErrorActionPreference='SilentlyContinue'; Wait-Process -Id {pid} -ErrorAction SilentlyContinue; $dir='{literal}'; for($i=0;$i -lt 40;$i++){{ foreach($name in @('chd312.exe','chd312-background.exe','Uninstall.exe','README.txt')){{ $p=Join-Path $dir $name; if(Test-Path -LiteralPath $p){{Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue}} }}; if((Test-Path -LiteralPath $dir) -and @(Get-ChildItem -LiteralPath $dir -Force).Count -eq 0){{Remove-Item -LiteralPath $dir -Force}}; if(-not (Test-Path -LiteralPath $dir)){{break}}; Start-Sleep -Milliseconds 500 }}"#
    );
    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &cleanup,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let remove = args.iter().any(|s| s == "--uninstall");
    let silent = args.iter().any(|s| s == "--silent");
    let result = (|| -> Result<()> {
        ensure!(system::is_admin(), "Administrator privileges required");
        if remove {
            uninstall().context("Uninstall")
        } else {
            install().context("Install")
        }
    })();
    let (code, message) = match result {
        Ok(()) => (
            0,
            if remove {
                "アンインストールしました。音声ドライバーとログは保持しています。".into()
            } else {
                "インストールが完了しました。画面を表示せずに接続を監視しています。\n新しく開いたターミナルで chd312 status を実行できます。".into()
            },
        ),
        Err(e) => (1, format!("処理に失敗しました: {e:#}")),
    };
    system::log(&format!("SETUP remove={remove} result={code} {message}"));
    if !silent {
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                system::wide(&message).as_ptr(),
                system::wide("CHD312 Windows 11 24H2+ Fix").as_ptr(),
                MB_OK
                    | if code == 0 {
                        MB_ICONINFORMATION
                    } else {
                        MB_ICONERROR
                    },
            );
        }
    }
    std::process::exit(code);
}
