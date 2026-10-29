//! Local, per-user IPC. Only fixed commands are accepted: never file paths or raw USB IDs.
use crate::{device::Handle, identity::Capture, system};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    ptr::null_mut,
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authorization::*, SECURITY_ATTRIBUTES},
    Storage::FileSystem::*,
    System::Pipes::*,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Request {
    Status,
    Apply { force: bool },
    Disable,
    Enable,
    RestoreOriginal,
    Stop,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub running: bool,
    pub pid: u32,
    pub automatic: bool,
    pub restart_required: bool,
    pub updated: String,
    pub devices: Vec<Capture>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    pub status: Option<Status>,
}
impl Response {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            status: None,
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            status: None,
        }
    }
}
pub enum Message {
    Device(String),
    Request(Request, Sender<Response>),
    Terminate,
}
fn pipe_name() -> Result<Vec<u16>> {
    Ok(system::wide(&format!(
        r"\\.\pipe\CHD312Win11Fix-{}",
        system::sid()?
    )))
}
pub fn call(request: &Request) -> Result<Response> {
    unsafe {
        let data = serde_json::to_vec(request)?;
        let mut output = vec![0u8; 262144];
        let mut read = 0;
        ensure!(
            CallNamedPipeW(
                pipe_name()?.as_ptr(),
                data.as_ptr().cast(),
                data.len() as u32,
                output.as_mut_ptr().cast(),
                output.len() as u32,
                &mut read,
                30_000
            ) != 0,
            "Background monitor is not available: {}",
            std::io::Error::last_os_error()
        );
        Ok(serde_json::from_slice(&output[..read as usize])?)
    }
}
pub fn spawn_server(sender: Sender<Message>) -> Result<()> {
    let sid = system::sid()?;
    let pipe = pipe_name()?;
    thread::Builder::new()
        .name("command-pipe".into())
        .spawn(move || {
            if let Err(e) = unsafe { serve(pipe, &sid, sender.clone()) } {
                system::log(&format!("IPC_FATAL {e:#}"));
                let _ = sender.send(Message::Terminate);
            }
        })?;
    Ok(())
}
unsafe fn serve(name: Vec<u16>, sid: &str, sender: Sender<Message>) -> Result<()> {
    let sddl = system::wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})S:(ML;;NW;;;ME)"));
    let mut descriptor = null_mut();
    ensure!(
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut()
        ) != 0,
        "Cannot secure command pipe"
    );
    struct SecurityDescriptor(*mut std::ffi::c_void);
    impl Drop for SecurityDescriptor {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let _descriptor = SecurityDescriptor(descriptor);
    let security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    loop {
        let pipe = Handle(CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            262144,
            4096,
            5000,
            &security,
        ));
        ensure!(
            pipe.0 != INVALID_HANDLE_VALUE,
            "Cannot create command pipe: {}",
            std::io::Error::last_os_error()
        );
        if ConnectNamedPipe(pipe.0, null_mut()) == 0 && GetLastError() != ERROR_PIPE_CONNECTED {
            continue;
        }
        // IPC runs on its own thread: a stalled client never blocks PnP detection/repair.
        let mut input = [0u8; 4096];
        let mut read = 0;
        let mut terminate = false;
        let response = if ReadFile(
            pipe.0,
            input.as_mut_ptr(),
            input.len() as u32,
            &mut read,
            null_mut(),
        ) != 0
        {
            match serde_json::from_slice::<Request>(&input[..read as usize]) {
                Ok(request) => {
                    terminate = request == Request::Stop;
                    let (tx, rx) = mpsc::channel();
                    if sender.send(Message::Request(request, tx)).is_ok() {
                        rx.recv_timeout(Duration::from_secs(25))
                            .unwrap_or_else(|_| Response::error("Command timed out; check logs"))
                    } else {
                        Response::error("Monitor is stopping")
                    }
                }
                Err(_) => Response::error("Unknown command; no action taken"),
            }
        } else {
            Response::error("Invalid command packet")
        };
        let bytes = serde_json::to_vec(&response)?;
        let mut written = 0;
        WriteFile(
            pipe.0,
            bytes.as_ptr(),
            bytes.len() as u32,
            &mut written,
            null_mut(),
        );
        // Let CallNamedPipe consume the reply before shutting down the process.
        FlushFileBuffers(pipe.0);
        DisconnectNamedPipe(pipe.0);
        if terminate {
            let _ = sender.send(Message::Terminate);
            break;
        }
    }
    Ok(())
}
