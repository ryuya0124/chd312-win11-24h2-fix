use crate::{
    device,
    identity::Capture,
    ipc::{self, Message, Request, Response, Status},
    system,
};
use anyhow::{ensure, Result};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    mem::size_of,
    ptr::{null, null_mut},
    sync::mpsc::{self, Sender},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::DeviceAndDriverInstallation::*, Foundation::*, System::Threading::CreateMutexW,
};

struct Events {
    handle: HCMNOTIFICATION,
    _context: Box<Sender<Message>>,
}
impl Events {
    fn register(sender: Sender<Message>) -> Result<Self> {
        unsafe {
            let context = Box::new(sender);
            let filter = CM_NOTIFY_FILTER {
                cbSize: size_of::<CM_NOTIFY_FILTER>() as u32,
                Flags: CM_NOTIFY_FILTER_FLAG_ALL_DEVICE_INSTANCES,
                FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINSTANCE,
                ..Default::default()
            };
            let mut handle = null_mut();
            let result = CM_Register_Notification(
                &filter,
                (&*context as *const Sender<Message>).cast(),
                Some(callback),
                &mut handle,
            );
            ensure!(result == 0, "PnP registration failed: CR {result}");
            Ok(Self {
                handle,
                _context: context,
            })
        }
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        unsafe {
            CM_Unregister_Notification(self.handle);
        }
    }
}
unsafe extern "system" fn callback(
    _: HCMNOTIFICATION,
    context: *const c_void,
    action: CM_NOTIFY_ACTION,
    data: *const CM_NOTIFY_EVENT_DATA,
    size: u32,
) -> u32 {
    if context.is_null() || data.is_null() || size < 10 || !(7..=9).contains(&action) {
        return 0;
    }
    if (*data).FilterType != CM_NOTIFY_FILTER_TYPE_DEVICEINSTANCE {
        return 0;
    }
    let chars = std::slice::from_raw_parts(
        data.cast::<u8>().add(8).cast::<u16>(),
        ((size - 8) / 2) as usize,
    );
    let len = chars.iter().position(|&v| v == 0).unwrap_or(chars.len());
    let id = String::from_utf16_lossy(&chars[..len]);
    if id
        .to_ascii_uppercase()
        .starts_with("USB\\VID_04B4&PID_00F8")
    {
        let tx = &*context.cast::<Sender<Message>>();
        let _ = tx.send(Message::Device(format!("{action} {id}")));
    }
    0
}
fn snapshot(devices: Vec<Capture>, automatic: bool, restart: bool) -> Status {
    Status {
        running: true,
        pid: std::process::id(),
        automatic,
        restart_required: restart,
        updated: chrono::Local::now().to_rfc3339(),
        devices,
    }
}
fn persist(status: &Status) {
    if let Ok(bytes) = serde_json::to_vec_pretty(status) {
        let path = system::data_dir().join("status.json");
        let temp = path.with_extension("tmp");
        if std::fs::write(&temp, bytes).is_ok() {
            let _ = std::fs::rename(temp, path);
        }
    }
}
fn repair_all(captures: &[Capture], desired: device::DesiredDriver) -> Result<(u32, bool)> {
    let mut changed = 0;
    let mut restart = false;
    for c in captures.iter().filter(|c| c.matches()) {
        let (n, r) = device::apply(&c.parent, desired)?;
        changed += n;
        restart |= r;
        if r {
            break;
        }
    }
    Ok((changed, restart))
}
pub fn watch() -> Result<()> {
    ensure!(
        system::is_admin(),
        "The background monitor must run elevated (use the installed task)"
    );
    std::fs::create_dir_all(system::data_dir())?;
    let mutex_name = system::wide(&format!("Local\\CHD312Win11Fix-{}", system::sid()?));
    let singleton = device::Handle(unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) });
    ensure!(!singleton.0.is_null(), "Cannot create monitor mutex");
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let _events = Events::register(tx.clone())?;
    ipc::spawn_server(tx)?;
    let mut automatic = system::settings_enabled();
    let mut status = snapshot(vec![], automatic, false);
    let mut pending = Some(Instant::now());
    let mut burst_start = Instant::now();
    let mut retries = 4u8;
    let mut attempts: HashMap<String, Vec<Instant>> = HashMap::new();
    let mut reboot_parents = HashSet::new();
    let mut stopping = false;
    system::log("MONITOR_START (Rust, no window, PnP notifications)");
    loop {
        let message = match pending {
            Some(deadline) => {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(m) => Some(m),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(_) => break,
                }
            }
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
        };
        match message {
            Some(Message::Terminate) => break,
            Some(Message::Device(reason)) => {
                if stopping {
                    continue;
                }
                system::log(&format!("PNP {reason}"));
                if pending.is_none() {
                    burst_start = Instant::now();
                }
                pending = Some(
                    (Instant::now() + Duration::from_millis(750))
                        .min(burst_start + Duration::from_secs(3)),
                );
                retries = 4;
            }
            Some(Message::Request(request, reply)) => {
                let result = (|| -> Result<Response> {
                    match request {
                        Request::Status => {
                            status.devices = device::inspect()?;
                            status.updated = chrono::Local::now().to_rfc3339();
                        }
                        Request::Disable => {
                            automatic = false;
                            status.automatic = false;
                            system::set_enabled(false)?;
                            system::log("AUTOMATIC_DISABLED");
                        }
                        Request::Enable => {
                            automatic = true;
                            status.automatic = true;
                            system::set_enabled(true)?;
                            pending = Some(Instant::now());
                            retries = 4;
                            system::log("AUTOMATIC_ENABLED");
                        }
                        Request::Apply { force } => {
                            ensure!(automatic||force,"Automatic repair is paused. Use apply --force for one manual pass.");
                            let captures = device::inspect()?;
                            ensure!(
                                captures.iter().any(Capture::matches),
                                "No fully matching capture device; nothing changed"
                            );
                            let (changed, restart) =
                                repair_all(&captures, device::DesiredDriver::Audio)?;
                            status.restart_required |= restart;
                            status.devices = device::inspect()?;
                            status.updated = chrono::Local::now().to_rfc3339();
                            persist(&status);
                            let mut r=Response::ok(format!("Changed {changed} audio function(s). USB Audio / unknown drivers were skipped. Restart required: {restart}"));
                            r.status = Some(status.clone());
                            return Ok(r);
                        }
                        Request::RestoreOriginal => {
                            automatic = false;
                            status.automatic = false;
                            system::set_enabled(false)?;
                            let captures = device::inspect()?;
                            ensure!(
                                captures.iter().any(Capture::matches),
                                "No fully matching device; nothing changed"
                            );
                            let (changed, restart) =
                                repair_all(&captures, device::DesiredDriver::OriginalMidi)?;
                            status.restart_required |= restart;
                            status.devices = device::inspect()?;
                            persist(&status);
                            let mut r=Response::ok(format!("Restored original USB MIDI binding for {changed} function(s). Automatic repair is PAUSED; audio may stop working."));
                            r.status = Some(status.clone());
                            return Ok(r);
                        }
                        Request::Stop => {
                            stopping = true;
                            pending = None;
                            status.running = false;
                            system::log("STOP_REQUESTED");
                        }
                    }
                    status.automatic = automatic;
                    persist(&status);
                    let mut response = Response::ok("OK");
                    response.status = Some(status.clone());
                    Ok(response)
                })();
                let _ = reply.send(result.unwrap_or_else(|e| {
                    system::log(&format!("COMMAND_ERROR {e:#}"));
                    Response::error(format!("{e:#}"))
                }));
            }
            None => {
                pending = None;
                if stopping {
                    continue;
                }
                let result = (|| -> Result<bool> {
                    let captures = device::inspect()?;
                    let mut retry = captures.iter().any(|c| !c.matches());
                    reboot_parents.retain(|p| captures.iter().any(|c| &c.parent == p));
                    if automatic {
                        for c in captures.iter().filter(|c| c.needs_repair()) {
                            if reboot_parents.contains(&c.parent) {
                                continue;
                            }
                            let recent = attempts.entry(c.parent.clone()).or_default();
                            recent.retain(|t| t.elapsed() < Duration::from_secs(120));
                            if recent.len() >= 3 {
                                system::log("RATE_LIMIT: three repair attempts; waiting for manual retry or next connection");
                                continue;
                            }
                            recent.push(Instant::now());
                            match device::apply(&c.parent, device::DesiredDriver::Audio) {
                                Ok((count, restart)) => {
                                    system::log(&format!(
                                        "AUTO_REPAIR changed={count} restart={restart}"
                                    ));
                                    if restart {
                                        reboot_parents.insert(c.parent.clone());
                                        status.restart_required = true;
                                    }
                                }
                                Err(e) => {
                                    system::log(&format!("AUTO_REPAIR_ERROR {e:#}"));
                                    retry = true;
                                }
                            }
                        }
                    }
                    status = snapshot(device::inspect()?, automatic, status.restart_required);
                    persist(&status);
                    system::log(&format!(
                        "SCAN matched={} midi={} automatic={automatic}",
                        status.devices.iter().filter(|c| c.matches()).count(),
                        status.devices.iter().filter(|c| c.needs_repair()).count()
                    ));
                    Ok(retry)
                })();
                let retry = result.unwrap_or_else(|e| {
                    system::log(&format!("SCAN_ERROR {e:#}"));
                    true
                });
                if retry && retries > 0 {
                    retries -= 1;
                    pending = Some(Instant::now() + Duration::from_millis(2500));
                    burst_start = Instant::now();
                }
            }
        }
    }
    status.running = false;
    persist(&status);
    system::log("MONITOR_STOP");
    Ok(())
}
