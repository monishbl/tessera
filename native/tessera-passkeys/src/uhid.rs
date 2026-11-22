//! The virtual security key's HID device, through /dev/uhid
//! (Documentation/hid/uhid.rst): UHID_CREATE2 with FIDO's report
//! descriptor (usage page 0xF1D0, 64-byte input and output reports, no
//! report ids), then OUTPUT events in (what a browser writes to the
//! hidraw node) and INPUT2 events out (what it reads). The kernel makes
//! the /dev/hidrawN node; systemd's 60-fido-id.rules sees the descriptor
//! and tags it a security token (uaccess), and
//! 70-tessera-passkeys.rules matches our ids too (docs/PASSKEYS.md).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;

/// The ids the key shows (pid.codes' open-source vendor id; a product id
/// it has not assigned). Kept stable: udev rules match them.
pub const VENDOR: u32 = 0x1209;
pub const PRODUCT: u32 = 0x7e55;
pub const NAME: &str = "Tessera Passkeys";
const BUS_USB: u16 = 0x03;

/// FIDO Alliance's HID report descriptor (CTAP 2.1 §11.2.8.2)
pub const REPORT_DESCRIPTOR: [u8; 34] = [
    0x06, 0xd0, 0xf1, // Usage Page (FIDO Alliance)
    0x09, 0x01, // Usage (CTAPHID)
    0xa1, 0x01, // Collection (Application)
    0x09, 0x20, //   Usage (Input Report Data)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xff, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x40, //   Report Count (64)
    0x81, 0x02, //   Input (Data, Var, Abs)
    0x09, 0x21, //   Usage (Output Report Data)
    0x15, 0x00, //   Logical Minimum (0)
    0x26, 0xff, 0x00, //   Logical Maximum (255)
    0x75, 0x08, //   Report Size (8)
    0x95, 0x40, //   Report Count (64)
    0x91, 0x02, //   Output (Data, Var, Abs)
    0xc0, // End Collection
];

// struct uhid_event: a u32 type, then the largest member (create2: 4372 bytes)
const EVENT_SIZE: usize = 4 + 4372;
const DESTROY: u32 = 1;
const START: u32 = 2;
const STOP: u32 = 3;
const OPEN: u32 = 4;
const CLOSE: u32 = 5;
const OUTPUT: u32 = 6;
const GET_REPORT: u32 = 9;
const GET_REPORT_REPLY: u32 = 10;
const CREATE2: u32 = 11;
const INPUT2: u32 = 12;
const SET_REPORT: u32 = 13;
const SET_REPORT_REPLY: u32 = 14;

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// a report the host wrote (64 bytes, the report id taken off)
    Output(Vec<u8>),
    /// the hidraw node was opened or closed, the driver started or stopped
    Open,
    Close,
    Start,
    Stop,
    /// something answered and ignored (GET/SET_REPORT), or unknown
    Other,
}

/// The CREATE2 request, as the kernel reads it
pub fn create2(name: &str, phys: &str, uniq: &str) -> Vec<u8> {
    let mut e = vec![0u8; EVENT_SIZE];
    e[0..4].copy_from_slice(&CREATE2.to_ne_bytes());
    let put = |e: &mut [u8], at: usize, max: usize, s: &str| {
        let b = s.as_bytes();
        let n = b.len().min(max - 1);
        e[at..at + n].copy_from_slice(&b[..n]);
    };
    put(&mut e, 4, 128, name);
    put(&mut e, 132, 64, phys);
    put(&mut e, 196, 64, uniq);
    e[260..262].copy_from_slice(&(REPORT_DESCRIPTOR.len() as u16).to_ne_bytes());
    e[262..264].copy_from_slice(&BUS_USB.to_ne_bytes());
    e[264..268].copy_from_slice(&VENDOR.to_ne_bytes());
    e[268..272].copy_from_slice(&PRODUCT.to_ne_bytes());
    e[272..276].copy_from_slice(&1u32.to_ne_bytes()); // version
    e[276..280].copy_from_slice(&0u32.to_ne_bytes()); // country
    e[280..280 + REPORT_DESCRIPTOR.len()].copy_from_slice(&REPORT_DESCRIPTOR);
    e
}

/// INPUT2: one report to the host
pub fn input2(report: &[u8]) -> Vec<u8> {
    let mut e = vec![0u8; EVENT_SIZE];
    e[0..4].copy_from_slice(&INPUT2.to_ne_bytes());
    let n = report.len().min(4096);
    e[4..6].copy_from_slice(&(n as u16).to_ne_bytes());
    e[6..6 + n].copy_from_slice(&report[..n]);
    e
}

/// An event read from the device: what it is, and the reply some need
pub fn parse(e: &[u8]) -> (Event, Option<Vec<u8>>) {
    if e.len() < 4 {
        return (Event::Other, None);
    }
    let kind = u32::from_ne_bytes([e[0], e[1], e[2], e[3]]);
    match kind {
        OUTPUT if e.len() >= 4103 => {
            let size = (u16::from_ne_bytes([e[4100], e[4101]]) as usize).min(4096);
            let mut data = e[4..4 + size].to_vec();
            // (hidraw passes the report number first: 0, as FIDO has no ids)
            if data.len() == 65 && data[0] == 0 {
                data.remove(0);
            }
            data.resize(64, 0);
            (Event::Output(data), None)
        }
        GET_REPORT if e.len() >= 10 => {
            let mut r = vec![0u8; EVENT_SIZE];
            r[0..4].copy_from_slice(&GET_REPORT_REPLY.to_ne_bytes());
            r[4..8].copy_from_slice(&e[4..8]);
            r[8..10].copy_from_slice(&(libc::EIO as u16).to_ne_bytes());
            (Event::Other, Some(r))
        }
        SET_REPORT if e.len() >= 8 => {
            let mut r = vec![0u8; EVENT_SIZE];
            r[0..4].copy_from_slice(&SET_REPORT_REPLY.to_ne_bytes());
            r[4..8].copy_from_slice(&e[4..8]);
            r[8..10].copy_from_slice(&(libc::EIO as u16).to_ne_bytes());
            (Event::Other, Some(r))
        }
        OPEN => (Event::Open, None),
        CLOSE => (Event::Close, None),
        START => (Event::Start, None),
        STOP => (Event::Stop, None),
        _ => (Event::Other, None),
    }
}

pub struct Device {
    file: File,
}

impl Device {
    pub const PATH: &'static str = "/dev/uhid";

    pub fn create() -> std::io::Result<Device> {
        let mut file = OpenOptions::new().read(true).write(true).custom_flags(libc::O_CLOEXEC).open(Self::PATH)?;
        let uniq = format!("tessera-{}", unsafe { libc::getuid() });
        file.write_all(&create2(NAME, "tessera-passkeys", &uniq))?;
        Ok(Device { file })
    }

    /// The next event (blocks)
    pub fn read(&self) -> std::io::Result<Event> {
        let mut buf = vec![0u8; EVENT_SIZE];
        let n = (&self.file).read(&mut buf)?;
        let (ev, reply) = parse(&buf[..n]);
        if let Some(r) = reply {
            let _ = (&self.file).write_all(&r);
        }
        Ok(ev)
    }

    /// One 64-byte report to the host (each write is one event: threads may share it)
    pub fn send(&self, report: &[u8]) -> std::io::Result<()> {
        (&self.file).write_all(&input2(report))
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        let mut e = vec![0u8; EVENT_SIZE];
        e[0..4].copy_from_slice(&DESTROY.to_ne_bytes());
        let _ = (&self.file).write_all(&e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_descriptor_is_fidos() {
        assert_eq!(&REPORT_DESCRIPTOR[..3], &[0x06, 0xd0, 0xf1]);
        assert_eq!(REPORT_DESCRIPTOR.last(), Some(&0xc0));
    }

    #[test]
    fn create2_lays_out_as_the_kernel_reads() {
        let e = create2("Tessera Passkeys", "p", "u");
        assert_eq!(e.len(), EVENT_SIZE);
        assert_eq!(u32::from_ne_bytes(e[0..4].try_into().unwrap()), CREATE2);
        assert_eq!(&e[4..20], b"Tessera Passkeys");
        assert_eq!(e[20], 0);
        assert_eq!(e[132], b'p');
        assert_eq!(e[196], b'u');
        assert_eq!(u16::from_ne_bytes([e[260], e[261]]) as usize, REPORT_DESCRIPTOR.len());
        assert_eq!(u16::from_ne_bytes([e[262], e[263]]), BUS_USB);
        assert_eq!(u32::from_ne_bytes(e[264..268].try_into().unwrap()), VENDOR);
        assert_eq!(u32::from_ne_bytes(e[268..272].try_into().unwrap()), PRODUCT);
        assert_eq!(&e[280..283], &[0x06, 0xd0, 0xf1]);
        // a long name is cut, still ending in a NUL
        let e = create2(&"x".repeat(300), "", "");
        assert_eq!(e[4 + 127], 0);
    }

    #[test]
    fn outputs_lose_their_report_number() {
        let mut e = vec![0u8; EVENT_SIZE];
        e[0..4].copy_from_slice(&OUTPUT.to_ne_bytes());
        e[4] = 0;
        e[5] = 0xaa;
        e[4100..4102].copy_from_slice(&65u16.to_ne_bytes());
        let (ev, reply) = parse(&e);
        assert!(reply.is_none());
        let Event::Output(d) = ev else { panic!() };
        assert_eq!(d.len(), 64);
        assert_eq!(d[0], 0xaa);
        // a 64-byte one stays as it is
        e[4100..4102].copy_from_slice(&64u16.to_ne_bytes());
        let Event::Output(d) = parse(&e).0 else { panic!() };
        assert_eq!(d[1], 0xaa);
    }

    #[test]
    fn report_requests_are_refused() {
        let mut e = vec![0u8; 16];
        e[0..4].copy_from_slice(&GET_REPORT.to_ne_bytes());
        e[4..8].copy_from_slice(&77u32.to_ne_bytes());
        let (_, r) = parse(&e);
        let r = r.unwrap();
        assert_eq!(u32::from_ne_bytes(r[0..4].try_into().unwrap()), GET_REPORT_REPLY);
        assert_eq!(u32::from_ne_bytes(r[4..8].try_into().unwrap()), 77);
        let i = input2(&[1u8; 64]);
        assert_eq!(u16::from_ne_bytes([i[4], i[5]]), 64);
        assert_eq!(i[6], 1);
    }
}
