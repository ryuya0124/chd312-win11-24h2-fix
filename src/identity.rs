use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const PREFIX: &str = "USB\\VID_04B4&PID_00F8\\";
pub const CONFIG_HASH: &str = "E235F83B29245CF7272F1F733000F2B36A76E48B88A1160F8F42F1CDB1ED33A7";
pub const DEVICE: [u8; 18] = [18, 1, 0, 3, 239, 2, 1, 9, 180, 4, 248, 0, 0, 0, 1, 2, 0, 1];
pub const BOS: [u8; 22] = [
    5, 15, 22, 0, 2, 7, 16, 2, 0, 0, 0, 0, 10, 16, 3, 0, 14, 0, 3, 0, 0, 0,
];
pub const NAMES: [&str; 5] = [
    "Cypress Semiconductor",
    "USB3.0 Capture Video",
    "USB3.0 Capture Analog Audio",
    "USB Controller",
    "USB3.0 Capture Digital Audio",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub parent: String,
    pub bus_name: String,
    pub class: String,
    pub hardware_ids: Vec<String>,
    pub compatible_ids: Vec<String>,
    pub inf: String,
    pub problem: u32,
    pub devinst: u32,
    pub port: u32,
}
#[derive(Clone, Debug)]
pub struct UsbIdentity {
    pub device: Vec<u8>,
    pub config: Vec<u8>,
    pub bos: Vec<u8>,
    pub strings: BTreeMap<u8, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub parent: String,
    pub checks: Vec<Check>,
    pub audio: Vec<Node>,
}
impl Capture {
    pub fn matches(&self) -> bool {
        self.checks.len() == 14 && self.checks.iter().all(|c| c.passed)
    }
    pub fn needs_repair(&self) -> bool {
        self.matches()
            && self
                .audio
                .iter()
                .any(|n| n.inf.eq_ignore_ascii_case("usbmidi2.inf"))
    }
    pub fn failed(parent: &str, err: String) -> Self {
        Self {
            parent: parent.into(),
            checks: vec![Check {
                name: "USB read".into(),
                passed: false,
                detail: err,
            }],
            audio: vec![],
        }
    }
}
pub fn matches_prefix(id: &str) -> bool {
    id.to_ascii_uppercase().starts_with(PREFIX)
}
pub fn match_capture(parent: &Node, nodes: &[Node], usb: &UsbIdentity) -> Capture {
    let mut checks = vec![];
    let mut add = |name: &str, passed: bool, detail: String| {
        checks.push(Check {
            name: name.into(),
            passed,
            detail,
        })
    };
    add("VID/PID", matches_prefix(&parent.id), "04B4:00F8".into());
    add(
        "Hardware revision",
        parent
            .hardware_ids
            .iter()
            .any(|s| s.eq_ignore_ascii_case("USB\\VID_04B4&PID_00F8&REV_0000")),
        "REV_0000".into(),
    );
    add(
        "Device descriptor",
        usb.device == DEVICE,
        "Known 18 bytes; no USB serial".into(),
    );
    add(
        "Configuration length",
        usb.config.len() == 1436,
        format!("{}/1436 bytes", usb.config.len()),
    );
    let hash = format!("{:X}", Sha256::digest(&usb.config));
    add("Configuration SHA-256", hash == CONFIG_HASH, hash);
    add("BOS descriptor", usb.bos == BOS, "Known 22 bytes".into());
    add(
        "USB strings",
        NAMES
            .iter()
            .enumerate()
            .all(|(i, n)| usb.strings.get(&((i + 1) as u8)).map(String::as_str) == Some(*n)),
        "All 5 strings".into(),
    );
    let children: Vec<_> = nodes
        .iter()
        .filter(|n| n.parent.eq_ignore_ascii_case(&parent.id))
        .collect();
    add(
        "Common parent / children",
        children.len() == 4,
        format!("{}/4 direct USB functions", children.len()),
    );
    let mut audio = vec![];
    for (mi, name, class) in [
        ("00", NAMES[1], "Camera"),
        ("02", NAMES[2], "MEDIA"),
        ("04", NAMES[3], "HIDClass"),
        ("05", NAMES[4], "MEDIA"),
    ] {
        let prefix = format!("USB\\VID_04B4&PID_00F8&MI_{mi}\\");
        let found: Vec<_> = children
            .iter()
            .filter(|n| n.id.to_ascii_uppercase().starts_with(&prefix))
            .collect();
        let ok = found.len() == 1
            && found[0].bus_name == name
            && found[0].class.eq_ignore_ascii_case(class);
        add(
            &format!("MI_{mi} / {name}"),
            ok,
            if found.len() == 1 {
                format!("{} / {}", found[0].bus_name, found[0].class)
            } else {
                "Missing or duplicated function".into()
            },
        );
        if ok && (mi == "02" || mi == "05") {
            audio.push((*found[0]).clone());
        }
    }
    add(
        "Audio IAD classification",
        usb.config.get(1187..1195) == Some(&[8, 11, 2, 2, 1, 3, 0, 3][..])
            && usb.config.get(1331..1339) == Some(&[8, 11, 5, 2, 1, 3, 0, 5][..]),
        "Known Audio/SubClass 03 declarations".into(),
    );
    add(
        "Audio compatible IDs",
        audio.len() == 2
            && audio.iter().all(|n| {
                n.compatible_ids
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("USB\\Class_01&SubClass_03&Prot_00"))
            }),
        "Both audio functions match".into(),
    );
    Capture {
        parent: parent.id.clone(),
        checks,
        audio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Node, Vec<Node>, UsbIdentity) {
        let parent = Node {
            id: format!("{PREFIX}new-instance"),
            parent: "HUB".into(),
            bus_name: "".into(),
            class: "USB".into(),
            hardware_ids: vec!["USB\\VID_04B4&PID_00F8&REV_0000".into()],
            compatible_ids: vec![],
            inf: "usb.inf".into(),
            problem: 0,
            devinst: 1,
            port: 2,
        };
        let nodes = [
            ("00", NAMES[1], "Camera"),
            ("02", NAMES[2], "MEDIA"),
            ("04", NAMES[3], "HIDClass"),
            ("05", NAMES[4], "MEDIA"),
        ]
        .into_iter()
        .map(|(mi, name, class)| Node {
            id: format!("USB\\VID_04B4&PID_00F8&MI_{mi}\\changed-child"),
            parent: parent.id.clone(),
            bus_name: name.into(),
            class: class.into(),
            hardware_ids: vec![],
            compatible_ids: vec!["USB\\Class_01&SubClass_03&Prot_00".into()],
            inf: "wdma_usb.inf".into(),
            problem: 0,
            devinst: 2,
            port: 0,
        })
        .collect();
        let usb = UsbIdentity {
            device: DEVICE.to_vec(),
            config: include_bytes!("../tests/fixtures/usb-config.bin").to_vec(),
            bos: BOS.to_vec(),
            strings: NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| ((i + 1) as u8, n.to_string()))
                .collect(),
        };
        (parent, nodes, usb)
    }
    #[test]
    fn known_descriptor_accepts_changed_instance() {
        let (p, n, u) = fixture();
        let m = match_capture(&p, &n, &u);
        assert!(m.matches());
        assert!(!m.needs_repair());
    }
    #[test]
    fn mismatched_identity_is_rejected() {
        for i in 0..12 {
            let (mut p, mut n, mut u) = fixture();
            match i {
                0 => p.id = "USB\\VID_9999&PID_00F8\\x".into(),
                1 => p.hardware_ids.clear(),
                2 => u.device[16] = 1,
                3 => {
                    u.config.pop();
                }
                4 => u.config[100] ^= 1,
                5 => u.bos[20] ^= 1,
                6 => {
                    u.strings.insert(5, "Other Audio".into());
                }
                7 => {
                    n.pop();
                }
                8 => n[0].parent = "another-parent".into(),
                9 => n[2].bus_name = "Other HID".into(),
                10 => n[3].class = "USB".into(),
                11 => n[1].compatible_ids.clear(),
                _ => unreachable!(),
            }
            assert!(!match_capture(&p, &n, &u).matches(), "case {i}");
        }
    }
    #[test]
    fn driver_problem_does_not_weaken_identity() {
        let (p, mut n, u) = fixture();
        n[1].inf = "usbmidi2.inf".into();
        n[1].problem = 10;
        let m = match_capture(&p, &n, &u);
        assert!(m.matches());
        assert!(m.needs_repair());
    }
    #[test]
    fn only_midi_binding_is_repaired() {
        let (p, mut n, u) = fixture();
        n[1].problem = 10;
        assert!(!match_capture(&p, &n, &u).needs_repair());
        n[1].inf = "other.inf".into();
        assert!(!match_capture(&p, &n, &u).needs_repair());
        n[1].inf = "usbmidi2.inf".into();
        n[1].problem = 0;
        assert!(match_capture(&p, &n, &u).needs_repair());
    }
    #[test]
    fn extra_or_mixed_parent_rejected() {
        let (p, mut n, u) = fixture();
        n.push(n[1].clone());
        assert!(!match_capture(&p, &n, &u).matches());
    }
    #[test]
    fn empty_descriptor_does_not_panic() {
        let (p, n, mut u) = fixture();
        u.config.clear();
        assert!(!match_capture(&p, &n, &u).matches());
    }
}
