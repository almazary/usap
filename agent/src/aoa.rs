//! Kabel USB tanpa mode developer, lewat Android Open Accessory (AOA).
//!
//! Agen memeriksa perangkat USB yang tercolok. HP Android yang mendukung AOA diminta masuk ke
//! mode aksesori "Usap"; HP lalu tersambung ulang sebagai 18d1:2d00 (atau 2d01 kalau USB debugging
//! menyala) dan membuka aplikasi Usap. Data mengalir lewat dua endpoint bulk dengan format sama
//! seperti mode USB lewat adb: setiap paket diawali panjang u16.

use rusb::{Context, Device, DeviceHandle, Direction, TransferType, UsbContext};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const GOOGLE: u16 = 0x18d1;
const ACCESSORY_PIDS: [u16; 6] = [0x2d00, 0x2d01, 0x2d02, 0x2d03, 0x2d04, 0x2d05];
const APPLE: u16 = 0x05ac;

const GET_PROTOCOL: u8 = 51;
const SEND_STRING: u8 = 52;
const START: u8 = 53;

// Harus sama dengan res/xml/accessory_filter.xml di aplikasi Android.
const STRINGS: [&str; 6] = ["Usap", "Usap Agent", "Trackpad Usap", "1", "", "usap-1"];

const T: Duration = Duration::from_millis(500);

static NEXT: AtomicU64 = AtomicU64::new(2_000_000);

pub type Handle = Arc<DeviceHandle<Context>>;

/// Kirim satu paket ke HP lewat kabel (dengan awalan panjang u16).
pub fn write(h: &Handle, ep: u8, b: &[u8]) {
    let mut f = Vec::with_capacity(b.len() + 2);
    f.extend((b.len() as u16).to_le_bytes());
    f.extend_from_slice(b);
    let _ = h.write_bulk(ep, &f, Duration::from_millis(100));
}

fn is_accessory(vid: u16, pid: u16) -> bool {
    vid == GOOGLE && ACCESSORY_PIDS.contains(&pid)
}

/// Hanya tanyai perangkat yang mungkin HP: bukan hub, bukan buatan Apple, dan tidak semua
/// antarmukanya keyboard/mouse, audio, video, atau penyimpanan.
fn maybe_phone(dev: &Device<Context>) -> bool {
    let Ok(d) = dev.device_descriptor() else { return false };
    if d.class_code() == 0x09 || d.vendor_id() == APPLE || is_accessory(d.vendor_id(), d.product_id()) {
        return false;
    }
    let Ok(cfg) = dev.active_config_descriptor() else { return true };
    let mut classes = Vec::new();
    for i in cfg.interfaces() {
        for alt in i.descriptors() {
            classes.push(alt.class_code());
        }
    }
    classes.is_empty() || classes.iter().any(|c| !matches!(c, 0x01 | 0x03 | 0x08 | 0x09 | 0x0e | 0x0b))
}

/// Minta HP masuk mode aksesori. Kembalikan true kalau HP menerima.
fn switch(dev: &Device<Context>) -> bool {
    let Ok(h) = dev.open() else { return false };
    let mut v = [0u8; 2];
    match h.read_control(0xc0, GET_PROTOCOL, 0, 0, &mut v, T) {
        Ok(2) if u16::from_le_bytes(v) >= 1 => {}
        _ => return false,
    }
    for (i, s) in STRINGS.iter().enumerate() {
        let mut b = s.as_bytes().to_vec();
        b.push(0);
        if h.write_control(0x40, SEND_STRING, 0, i as u16, &b, T).is_err() {
            return false;
        }
    }
    h.write_control(0x40, START, 0, 0, &[], T).is_ok()
}

fn bulk_endpoints(dev: &Device<Context>) -> Option<(u8, u8, u8)> {
    let cfg = dev.active_config_descriptor().ok()?;
    let iface = cfg.interfaces().next()?;
    let alt = iface.descriptors().next()?;
    let mut ep_in = None;
    let mut ep_out = None;
    for ep in alt.endpoint_descriptors() {
        if ep.transfer_type() != TransferType::Bulk {
            continue;
        }
        match ep.direction() {
            Direction::In => ep_in = Some(ep.address()),
            Direction::Out => ep_out = Some(ep.address()),
        }
    }
    Some((alt.interface_number(), ep_in?, ep_out?))
}

/// Jalankan di thread sendiri. `deliver(id sesi, handle, endpoint keluar, paket)` untuk setiap paket.
pub fn spawn<F>(deliver: F)
where
    F: Fn(u64, Handle, u8, Vec<u8>) + Send + 'static,
{
    std::thread::spawn(move || {
        let ctx = match Context::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("USB tanpa mode developer tidak aktif: {e}");
                return;
            }
        };
        // jangan menanyai perangkat yang sama berulang-ulang
        let mut tried: HashMap<(u8, u8), Instant> = HashMap::new();
        loop {
            let devices = match ctx.devices() {
                Ok(d) => d,
                Err(_) => {
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                }
            };
            let mut ran = false;
            for dev in devices.iter() {
                let Ok(d) = dev.device_descriptor() else { continue };
                if is_accessory(d.vendor_id(), d.product_id()) {
                    run_session(&dev, &deliver);
                    ran = true;
                    break;
                }
            }
            if !ran {
                for dev in devices.iter() {
                    let key = (dev.bus_number(), dev.address());
                    if tried.get(&key).map_or(false, |t| t.elapsed() < Duration::from_secs(30)) {
                        continue;
                    }
                    if !maybe_phone(&dev) {
                        continue;
                    }
                    tried.insert(key, Instant::now());
                    if switch(&dev) {
                        println!("Kabel: HP terdeteksi, meminta HP membuka Usap (mode aksesori)...");
                    }
                }
                tried.retain(|_, t| t.elapsed() < Duration::from_secs(120));
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    });
}

fn run_session<F>(dev: &Device<Context>, deliver: &F)
where
    F: Fn(u64, Handle, u8, Vec<u8>),
{
    let Some((iface, ep_in, ep_out)) = bulk_endpoints(dev) else { return };
    let h = match dev.open() {
        Ok(h) => Arc::new(h),
        Err(_) => return,
    };
    let _ = h.set_auto_detach_kernel_driver(true);
    if h.claim_interface(iface).is_err() {
        return;
    }
    let sid = NEXT.fetch_add(1, Ordering::Relaxed);
    println!("Kabel: HP dalam mode aksesori, menunggu aplikasi Usap...");
    let mut buf = vec![0u8; 16384];
    let mut acc: Vec<u8> = Vec::new();
    let mut got_any = false;
    loop {
        match h.read_bulk(ep_in, &mut buf, Duration::from_millis(200)) {
            Ok(n) => {
                acc.extend_from_slice(&buf[..n]);
                while acc.len() >= 2 {
                    let len = u16::from_le_bytes([acc[0], acc[1]]) as usize;
                    if len == 0 || len > 512 {
                        acc.clear();
                        break;
                    }
                    if acc.len() < 2 + len {
                        break;
                    }
                    let pkt = acc[2..2 + len].to_vec();
                    acc.drain(..2 + len);
                    got_any = true;
                    deliver(sid, h.clone(), ep_out, pkt);
                }
            }
            Err(rusb::Error::Timeout) => {}
            Err(_) => break, // kabel dicabut atau aplikasi ditutup
        }
    }
    if got_any {
        deliver(sid, h.clone(), ep_out, vec![crate::proto::BYE]);
    }
    println!("Kabel: HP dilepas.");
    let _ = h.release_interface(iface);
}
