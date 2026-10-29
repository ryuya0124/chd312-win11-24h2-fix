use crate::device::Handle;
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs, io::Write, os::windows::process::CommandExt, path::PathBuf, process::Command, sync::Mutex,
};
use windows_sys::Win32::{
    Foundation::*, Security::Authorization::*, Security::*, System::Threading::*, UI::Shell::*,
};
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn is_admin() -> bool {
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let token = Handle(token);
        let mut info = TOKEN_ELEVATION::default();
        let mut len = 0;
        GetTokenInformation(
            token.0,
            TokenElevation,
            (&mut info as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0
            && info.TokenIsElevated != 0
    }
}
pub fn sid() -> Result<String> {
    unsafe {
        let mut token = std::ptr::null_mut();
        ensure!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
            "Cannot open process token"
        );
        let token = Handle(token);
        let mut required = 0;
        GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut required);
        let mut storage = vec![0u64; (required as usize).div_ceil(8)];
        ensure!(
            GetTokenInformation(
                token.0,
                TokenUser,
                storage.as_mut_ptr().cast(),
                required,
                &mut required
            ) != 0,
            "Cannot read user SID"
        );
        let user = &*storage.as_ptr().cast::<TOKEN_USER>();
        let mut result = std::ptr::null_mut();
        ensure!(
            ConvertSidToStringSidW(user.User.Sid, &mut result) != 0,
            "Cannot format SID"
        );
        let mut len = 0;
        while *result.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(result, len));
        LocalFree(result.cast());
        Ok(text)
    }
}
pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| std::env::temp_dir().into()))
        .join("CHD312Win11Fix")
}
pub fn log_path() -> PathBuf {
    data_dir().join("fix.log")
}
static LOG_LOCK: Mutex<()> = Mutex::new(());
pub fn log(message: &str) {
    let Ok(_lock) = LOG_LOCK.lock() else {
        return;
    };
    let dir = data_dir();
    let _ = fs::create_dir_all(&dir);
    let path = log_path();
    if fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
        for i in (1..=3).rev() {
            let source = if i == 1 {
                path.clone()
            } else {
                dir.join(format!("fix.log.{}", i - 1))
            };
            let dest = dir.join(format!("fix.log.{i}"));
            let _ = fs::remove_file(&dest);
            let _ = fs::rename(source, dest);
        }
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{} {message}", chrono::Local::now().to_rfc3339());
    }
}
pub fn task_name() -> Result<String> {
    Ok(format!("CHD312Win11Fix-{}", sid()?))
}
pub fn hidden_command(exe: &str) -> Command {
    let mut c = Command::new(exe);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}
pub fn start_task() -> Result<()> {
    let output = hidden_command("schtasks.exe")
        .args(["/Run", "/TN", &task_name()?])
        .output()?;
    ensure!(output.status.success(),"Automatic repair task is not installed. Run the installer, or start the portable background worker.");
    Ok(())
}
pub fn installed_dir() -> Result<PathBuf> {
    unsafe {
        let mut path = std::ptr::null_mut();
        let id = windows_sys::core::GUID::from_u128(0x6d809377_6af0_444b_8957_a3773f02200e);
        ensure!(
            SHGetKnownFolderPath(&id, 0, std::ptr::null_mut(), &mut path) >= 0,
            "Cannot locate Program Files"
        );
        let mut len = 0;
        while *path.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        windows_sys::Win32::System::Com::CoTaskMemFree(path.cast());
        Ok(PathBuf::from(text).join("CHD312 Windows 11 Fix"))
    }
}
pub fn run_elevated(exe: &std::path::Path, args: &str, wait: bool) -> Result<u32> {
    unsafe {
        let exe = wide(&exe.to_string_lossy());
        let params = wide(args);
        let verb = wide("runas");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: verb.as_ptr(),
            lpFile: exe.as_ptr(),
            lpParameters: params.as_ptr(),
            nShow: 0,
            ..Default::default()
        };
        ensure!(
            ShellExecuteExW(&mut info) != 0,
            "Elevation was cancelled or failed: {}",
            std::io::Error::last_os_error()
        );
        let h = Handle(info.hProcess);
        if !wait {
            return Ok(0);
        }
        ensure!(
            WaitForSingleObject(h.0, 180_000) == WAIT_OBJECT_0,
            "Elevated operation timed out"
        );
        let mut code = 1;
        GetExitCodeProcess(h.0, &mut code);
        Ok(code)
    }
}
pub fn settings_enabled() -> bool {
    fs::read_to_string(data_dir().join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["automatic"].as_bool())
        .unwrap_or(true)
}
pub fn set_enabled(enabled: bool) -> Result<()> {
    fs::create_dir_all(data_dir())?;
    fs::write(
        data_dir().join("settings.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"automatic":enabled}))?,
    )?;
    Ok(())
}
pub fn adjacent_worker() -> Result<PathBuf> {
    let path = std::env::current_exe()?.with_file_name("chd312-background.exe");
    ensure!(
        path.exists(),
        "Background module not found; use the official portable EXE or installer"
    );
    Ok(path)
}
pub fn start_worker(worker: &std::path::Path) -> Result<()> {
    if is_admin() {
        Command::new(worker)
            .arg("watch")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
    } else {
        run_elevated(worker, "watch", false)?;
    }
    Ok(())
}
pub fn open_logs() -> Result<()> {
    fs::create_dir_all(data_dir())?;
    hidden_command("explorer.exe").arg(data_dir()).spawn()?;
    Ok(())
}
pub fn uninstall() -> Result<()> {
    let exe = installed_dir()?.join("Uninstall.exe");
    ensure!(
        exe.is_file(),
        "Not installed. For the portable edition, run 'chd312 stop' and delete its EXE."
    );
    let code = run_elevated(&exe, "--uninstall", true).context("Uninstall")?;
    if code != 0 {
        bail!("Uninstaller returned {code}");
    }
    Ok(())
}
