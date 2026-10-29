//! Read-only USB discovery plus the narrowly scoped SetupAPI driver switch.
use crate::{identity::*, system};
use anyhow::{anyhow, bail, ensure, Context, Result};
use std::{
    collections::BTreeMap,
    mem::size_of,
    ptr::{null, null_mut},
    thread,
    time::Duration,
};
use windows_sys::{
    core::GUID,
    Win32::{
        Devices::DeviceAndDriverInstallation::*,
        Foundation::*,
        Storage::FileSystem::*,
        System::{Registry::*, IO::DeviceIoControl},
    },
};

pub struct InfoSet(HDEVINFO);
impl Drop for InfoSet {
    fn drop(&mut self) {
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}
pub struct Handle(pub HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn win(ok: i32) -> Result<()> {
    if ok == 0 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
fn text(bytes: &[u8]) -> String {
    String::from_utf16_lossy(
        &bytes
            .chunks_exact(2)
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect::<Vec<_>>(),
    )
    .trim_end_matches('\0')
    .into()
}
fn wide_text(chars: &[u16]) -> String {
    String::from_utf16_lossy(&chars[..chars.iter().position(|&v| v == 0).unwrap_or(chars.len())])
}
fn new_dev() -> SP_DEVINFO_DATA {
    SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    }
}
unsafe fn prop(set: HDEVINFO, dev: &SP_DEVINFO_DATA, property: u32) -> Vec<u8> {
    let mut b = vec![0u8; 16384];
    let mut len = 0;
    if SetupDiGetDeviceRegistryPropertyW(
        set,
        dev,
        property,
        null_mut(),
        b.as_mut_ptr(),
        b.len() as u32,
        &mut len,
    ) == 0
    {
        return vec![];
    }
    b.truncate(len as usize);
    b
}
unsafe fn parent(dev: u32) -> String {
    let mut p = 0;
    let mut b = [0u16; 512];
    if CM_Get_Parent(&mut p, dev, 0) != 0 || CM_Get_Device_IDW(p, b.as_mut_ptr(), 512, 0) != 0 {
        String::new()
    } else {
        wide_text(&b)
    }
}
unsafe fn inf_path(set: HDEVINFO, dev: &SP_DEVINFO_DATA) -> String {
    let key = text(&prop(set, dev, SPDRP_DRIVER));
    if key.is_empty() {
        return String::new();
    }
    let path = system::wide(&format!("SYSTEM\\CurrentControlSet\\Control\\Class\\{key}"));
    let value = system::wide("InfPath");
    let mut b = [0u8; 2048];
    let mut len = b.len() as u32;
    if RegGetValueW(
        HKEY_LOCAL_MACHINE,
        path.as_ptr(),
        value.as_ptr(),
        RRF_RT_REG_SZ,
        null_mut(),
        b.as_mut_ptr().cast(),
        &mut len,
    ) != 0
    {
        String::new()
    } else {
        text(&b[..len as usize])
    }
}
pub fn enumerate() -> Result<Vec<Node>> {
    unsafe {
        let set = InfoSet(SetupDiGetClassDevsW(
            null(),
            system::wide("USB").as_ptr(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        ));
        ensure!(
            set.0 != -1,
            "USB device enumeration: {}",
            std::io::Error::last_os_error()
        );
        let mut result = vec![];
        let mut index = 0;
        loop {
            let mut d = new_dev();
            if SetupDiEnumDeviceInfo(set.0, index, &mut d) == 0 {
                if GetLastError() == ERROR_NO_MORE_ITEMS {
                    break;
                }
                win(0)?;
            }
            index += 1;
            let mut id = [0u16; 512];
            win(SetupDiGetDeviceInstanceIdW(
                set.0,
                &d,
                id.as_mut_ptr(),
                512,
                null_mut(),
            ))?;
            let id = wide_text(&id);
            if !id
                .to_ascii_uppercase()
                .starts_with("USB\\VID_04B4&PID_00F8")
            {
                continue;
            }
            let bus_key = DEVPROPKEY {
                fmtid: GUID::from_u128(0x540b947e_8b40_45bc_a8a2_6a0b894cbda2),
                pid: 4,
            };
            let mut b = [0u8; 2048];
            let mut len = 0;
            let mut ty = 0;
            let name = if SetupDiGetDevicePropertyW(
                set.0,
                &d,
                &bus_key,
                &mut ty,
                b.as_mut_ptr(),
                2048,
                &mut len,
                0,
            ) == 0
            {
                String::new()
            } else {
                text(&b[..len as usize])
            };
            let mut problem = 0;
            let mut status = 0;
            if CM_Get_DevNode_Status(&mut status, &mut problem, d.DevInst, 0) != 0 {
                problem = u32::MAX;
            }
            let address = prop(set.0, &d, SPDRP_ADDRESS);
            result.push(Node {
                id,
                parent: parent(d.DevInst),
                bus_name: name,
                class: text(&prop(set.0, &d, SPDRP_CLASS)),
                hardware_ids: text(&prop(set.0, &d, SPDRP_HARDWAREID))
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                compatible_ids: text(&prop(set.0, &d, SPDRP_COMPATIBLEIDS))
                    .split('\0')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
                inf: inf_path(set.0, &d),
                problem,
                devinst: d.DevInst,
                port: if address.len() >= 4 {
                    u32::from_le_bytes(address[..4].try_into()?)
                } else {
                    0
                },
            });
        }
        Ok(result)
    }
}
unsafe fn hub_path(parent_id: &str) -> Result<String> {
    let guid = GUID::from_u128(0xf18a0e88_c30c_11d0_8815_00a0c906bed8);
    let set = InfoSet(SetupDiGetClassDevsW(
        &guid,
        null(),
        null_mut(),
        DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
    ));
    ensure!(set.0 != -1, "USB hub discovery failed");
    let mut index = 0;
    loop {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..Default::default()
        };
        if SetupDiEnumDeviceInterfaces(set.0, null(), &guid, index, &mut interface) == 0 {
            if GetLastError() == ERROR_NO_MORE_ITEMS {
                break;
            }
            win(0)?;
        }
        index += 1;
        let mut d = new_dev();
        let mut required = 0;
        SetupDiGetDeviceInterfaceDetailW(set.0, &interface, null_mut(), 0, &mut required, &mut d);
        ensure!(
            (8..65536).contains(&required),
            "Unexpected USB interface detail length"
        );
        let mut buffer = vec![0u64; (required as usize).div_ceil(8)];
        let detail = buffer
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        win(SetupDiGetDeviceInterfaceDetailW(
            set.0,
            &interface,
            detail,
            required,
            null_mut(),
            &mut d,
        ))?;
        let mut id = [0u16; 512];
        win(SetupDiGetDeviceInstanceIdW(
            set.0,
            &d,
            id.as_mut_ptr(),
            512,
            null_mut(),
        ))?;
        if wide_text(&id).eq_ignore_ascii_case(parent_id) {
            let path = std::slice::from_raw_parts(
                (detail.cast::<u8>().add(4)).cast::<u16>(),
                (required as usize - 4) / 2,
            );
            return Ok(wide_text(path));
        }
    }
    bail!("Parent USB hub is not present")
}
unsafe fn ioctl(h: HANDLE, code: u32, mut data: Vec<u8>) -> Result<Vec<u8>> {
    let mut returned = 0;
    let n = data.len() as u32;
    let p = data.as_mut_ptr();
    win(DeviceIoControl(
        h,
        code,
        p.cast(),
        n,
        p.cast(),
        n,
        &mut returned,
        null_mut(),
    ))?;
    ensure!(returned <= n, "USB response exceeds buffer");
    data.truncate(returned as usize);
    Ok(data)
}
unsafe fn descriptor(
    h: HANDLE,
    port: u32,
    kind: u8,
    index: u8,
    lang: u16,
    len: u16,
) -> Result<Vec<u8>> {
    let mut request = vec![0u8; 12 + len as usize];
    request[..4].copy_from_slice(&port.to_le_bytes());
    request[4..8].copy_from_slice(&[0x80, 6, index, kind]);
    request[8..10].copy_from_slice(&lang.to_le_bytes());
    request[10..12].copy_from_slice(&len.to_le_bytes());
    let response = ioctl(h, 0x220410, request)?;
    ensure!(response.len() >= 14, "Incomplete USB descriptor");
    Ok(response[12..].to_vec())
}
pub fn read_usb(p: &Node) -> Result<UsbIdentity> {
    unsafe {
        let path = system::wide(&hub_path(&p.parent)?);
        let h = Handle(CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        ));
        ensure!(
            h.0 != INVALID_HANDLE_VALUE,
            "Cannot open USB hub: {}",
            std::io::Error::last_os_error()
        );
        let mut q = vec![0u8; 4096];
        q[..4].copy_from_slice(&p.port.to_le_bytes());
        let node = ioctl(h.0, 0x220448, q)?;
        ensure!(
            node.len() >= 22 && node[12..16] == [0xb4, 4, 0xf8, 0],
            "USB connection changed; refusing to continue"
        );
        let device = node[4..22].to_vec();
        let head = descriptor(h.0, p.port, 2, 0, 0, 9)?;
        ensure!(head.len() >= 9, "Short configuration header");
        let len = u16::from_le_bytes([head[2], head[3]]);
        ensure!((9..=16384).contains(&len), "Invalid configuration length");
        let config = descriptor(h.0, p.port, 2, 0, 0, len)?;
        let head = descriptor(h.0, p.port, 15, 0, 0, 5)?;
        ensure!(head.len() >= 5, "Short BOS header");
        let len = u16::from_le_bytes([head[2], head[3]]);
        ensure!((5..=4096).contains(&len), "Invalid BOS length");
        let bos = descriptor(h.0, p.port, 15, 0, 0, len)?;
        let mut strings = BTreeMap::new();
        for index in 1..=5 {
            let s = descriptor(h.0, p.port, 3, index, 0x0409, 255)?;
            ensure!(
                s.len() >= 2 && s[1] == 3 && s[0] as usize == s.len() && s.len() % 2 == 0,
                "Invalid USB string"
            );
            strings.insert(index, text(&s[2..]));
        }
        Ok(UsbIdentity {
            device,
            config,
            bos,
            strings,
        })
    }
}
pub fn inspect() -> Result<Vec<Capture>> {
    let nodes = enumerate()?;
    Ok(nodes
        .iter()
        .filter(|p| matches_prefix(&p.id))
        .map(|p| match read_usb(p) {
            Ok(u) => match_capture(p, &nodes, &u),
            Err(e) => Capture::failed(&p.id, format!("{e:#}")),
        })
        .collect())
}

// Diagnostic hints from previously validated locations, never an installation target.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UsbLocation {
    pub hub: String,
    pub port: u32,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct UsbPortObservation {
    pub location: UsbLocation,
    pub connection_status: Option<u32>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub error: Option<String>,
}
impl UsbPortObservation {
    pub fn description(&self) -> &'static str {
        match self.connection_status {
            Some(1) if self.vid == Some(0x04b4) && self.pid == Some(0x00f8) =>
                "USB hub sees 04B4:00F8; Windows capture functions are not ready or do not fully match",
            Some(1) => "USB hub reports a connected device; full identity is not confirmed",
            Some(9) => "USB hub reports DeviceEnumerating (initialization in progress)",
            Some(10) => "USB hub reports DeviceReset (reset in progress)",
            Some(0) => "USB hub reports NoDeviceConnected at this previously validated port",
            Some(_) => "USB hub reports a connection error or another connection state",
            None => "USB hub state could not be read",
        }
    }
}
pub fn remember_locations(captures: &[Capture], known: &mut Vec<UsbLocation>) -> Result<()> {
    if !captures.iter().any(Capture::matches) {
        return Ok(());
    }
    for node in enumerate()? {
        if node.port == 0
            || node.parent.is_empty()
            || !captures
                .iter()
                .any(|c| c.matches() && c.parent.eq_ignore_ascii_case(&node.id))
        {
            continue;
        }
        let location = UsbLocation {
            hub: node.parent,
            port: node.port,
        };
        if !known.contains(&location) {
            known.push(location);
            if known.len() > 16 {
                known.remove(0);
            }
        }
    }
    Ok(())
}
fn parse_port(location: UsbLocation, node: &[u8]) -> Result<UsbPortObservation> {
    // USB_NODE_CONNECTION_INFORMATION_EX uses the SDK's packed layout.
    ensure!(node.len() >= 35, "Incomplete USB connection information");
    Ok(UsbPortObservation {
        location,
        connection_status: Some(u32::from_le_bytes(node[31..35].try_into()?)),
        vid: Some(u16::from_le_bytes(node[12..14].try_into()?)),
        pid: Some(u16::from_le_bytes(node[14..16].try_into()?)),
        error: None,
    })
}
pub fn probe_port(location: &UsbLocation) -> UsbPortObservation {
    let result = (|| -> Result<_> {
        unsafe {
            let path = system::wide(&hub_path(&location.hub)?);
            let h = Handle(CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            ));
            ensure!(
                h.0 != INVALID_HANDLE_VALUE,
                "Cannot open USB hub: {}",
                std::io::Error::last_os_error()
            );
            let mut request = vec![0u8; 4096];
            request[..4].copy_from_slice(&location.port.to_le_bytes());
            parse_port(location.clone(), &ioctl(h.0, 0x220448, request)?)
        }
    })();
    result.unwrap_or_else(|e| UsbPortObservation {
        location: location.clone(),
        connection_status: None,
        vid: None,
        pid: None,
        error: Some(format!("{e:#}")),
    })
}

#[cfg(test)]
mod port_tests {
    use super::*;
    #[test]
    fn packed_connection_state_and_identity_are_separate() {
        let location = UsbLocation {
            hub: "test hub".into(),
            port: 2,
        };
        let mut node = vec![0u8; 35];
        node[12..16].copy_from_slice(&[0xb4, 4, 0xf8, 0]);
        node[31..35].copy_from_slice(&1u32.to_le_bytes());
        let connected = parse_port(location.clone(), &node).unwrap();
        assert_eq!(connected.vid, Some(0x04b4));
        assert!(connected.description().contains("not ready"));
        node[31..35].copy_from_slice(&9u32.to_le_bytes());
        assert!(parse_port(location.clone(), &node)
            .unwrap()
            .description()
            .contains("DeviceEnumerating"));
        assert!(parse_port(location, &node[..34]).is_err());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesiredDriver {
    Audio,
    OriginalMidi,
}
pub fn apply(parent_id: &str, desired: DesiredDriver) -> Result<(u32, bool)> {
    ensure!(system::is_admin(), "Administrator privileges required");
    let mut changed = 0;
    let mut restart = false;
    for mi in ["02", "05"] {
        // No wildcard installation and no fallback: re-read every identity check before EACH change.
        let captures = inspect()?;
        let c = captures
            .iter()
            .find(|c| c.parent.eq_ignore_ascii_case(parent_id))
            .context("Device was disconnected")?;
        ensure!(
            c.matches(),
            "Identity changed before repair; refusing installation"
        );
        let prefix = format!("USB\\VID_04B4&PID_00F8&MI_{mi}\\");
        let n = c
            .audio
            .iter()
            .find(|n| n.id.to_ascii_uppercase().starts_with(&prefix))
            .context("Audio child disappeared")?;
        let inf = if desired == DesiredDriver::Audio {
            "wdma_usb.inf"
        } else {
            "usbmidi2.inf"
        };
        // Even a forced/manual request cannot replace an already-correct or unknown driver.
        let source = if desired == DesiredDriver::Audio {
            "usbmidi2.inf"
        } else {
            "wdma_usb.inf"
        };
        if !n.inf.eq_ignore_ascii_case(source) {
            system::log(&format!(
                "SKIP instance={} current={} expected_source={source}",
                n.id, n.inf
            ));
            continue;
        }
        system::log(&format!("BEFORE {}", serde_json::to_string(n)?));
        let Some(reboot) = (unsafe { install_driver(&n.id, inf, source)? }) else {
            system::log(&format!("SKIP_CHANGED_BINDING instance={}", n.id));
            continue;
        };
        restart |= reboot;
        changed += 1;
        system::log(&format!(
            "INSTALLED instance={} inf={inf} reboot={reboot}",
            n.id
        ));
        if reboot {
            break;
        }
        if desired == DesiredDriver::Audio {
            let mut healthy = false;
            for _ in 0..20 {
                thread::sleep(Duration::from_millis(250));
                if enumerate()?.iter().any(|v| {
                    v.id.eq_ignore_ascii_case(&n.id)
                        && v.problem == 0
                        && v.inf.eq_ignore_ascii_case(inf)
                }) {
                    healthy = true;
                    break;
                }
            }
            ensure!(healthy, "Driver switch did not become healthy: {}", n.id);
        }
    }
    Ok((changed, restart))
}
unsafe fn install_driver(id: &str, desired: &str, expected_source: &str) -> Result<Option<bool>> {
    let set = InfoSet(SetupDiCreateDeviceInfoList(null(), null_mut()));
    ensure!(set.0 != -1, "Cannot create SetupAPI list");
    let mut d = new_dev();
    win(SetupDiOpenDeviceInfoW(
        set.0,
        system::wide(id).as_ptr(),
        null_mut(),
        0,
        &mut d,
    ))?;
    win(SetupDiBuildDriverInfoList(
        set.0,
        &mut d,
        SPDIT_COMPATDRIVER,
    ))?;
    let mut selected = None;
    let mut index = 0;
    loop {
        let mut driver = SP_DRVINFO_DATA_V2_W {
            cbSize: size_of::<SP_DRVINFO_DATA_V2_W>() as u32,
            ..Default::default()
        };
        if SetupDiEnumDriverInfoW(set.0, &d, SPDIT_COMPATDRIVER, index, &mut driver) == 0 {
            if GetLastError() == ERROR_NO_MORE_ITEMS {
                break;
            }
            win(0)?;
        }
        index += 1;
        let mut required = 0;
        SetupDiGetDriverInfoDetailW(set.0, &d, &driver, null_mut(), 0, &mut required);
        let len = (required as usize).max(size_of::<SP_DRVINFO_DETAIL_DATA_W>());
        ensure!(len < 1_048_576, "Unexpected driver detail size");
        let mut storage = vec![0u64; len.div_ceil(8)];
        let detail = storage.as_mut_ptr().cast::<SP_DRVINFO_DETAIL_DATA_W>();
        (*detail).cbSize = size_of::<SP_DRVINFO_DETAIL_DATA_W>() as u32;
        win(SetupDiGetDriverInfoDetailW(
            set.0,
            &d,
            &driver,
            detail,
            len as u32,
            null_mut(),
        ))?;
        let inf = wide_text(&(*detail).InfFileName);
        let section = wide_text(&(*detail).SectionName);
        let provider = wide_text(&driver.ProviderName);
        if inf
            .rsplit('\\')
            .next()
            .is_some_and(|f| f.eq_ignore_ascii_case(desired))
            && provider == "Microsoft"
            && (desired != "wdma_usb.inf" || section == "USBAudio")
        {
            selected = Some(driver);
            break;
        }
    }
    let driver = selected.ok_or_else(|| {
        anyhow!("Compatible Microsoft {desired} not found; no fallback installed")
    })?;
    if !inf_path(set.0, &d).eq_ignore_ascii_case(expected_source) {
        return Ok(None);
    }
    let mut reboot = 0;
    win(DiInstallDevice(
        null_mut(),
        set.0,
        &d,
        &driver,
        0,
        &mut reboot,
    ))?;
    Ok(Some(reboot != 0))
}
