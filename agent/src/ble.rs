//! Jalur Bluetooth Low Energy. HP memancarkan layanan Usap (peripheral), agen mencari dan
//! menyambung ke HP (central). Paket sama dengan jalur Wi-Fi; HP mengirim lewat notifikasi
//! karakteristik TX (beberapa paket sekaligus dalam satu BATCH), agen membalas lewat tulis ke RX.

use crate::proto;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use std::error::Error;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use uuid::Uuid;

pub const SERVICE: Uuid = Uuid::from_u128(0x7a1c0001_5e2a_4c6b_9a57_2f0d8b6c3e10);
/// HP -> agen (notify)
pub const TX: Uuid = Uuid::from_u128(0x7a1c0002_5e2a_4c6b_9a57_2f0d8b6c3e10);
/// agen -> HP (write without response)
pub const RX: Uuid = Uuid::from_u128(0x7a1c0003_5e2a_4c6b_9a57_2f0d8b6c3e10);

type Res<T> = Result<T, Box<dyn Error>>;

static NEXT: AtomicU64 = AtomicU64::new(1_000_000);

/// Jalankan pencarian dan sambungan Bluetooth di thread sendiri.
/// `deliver(id sesi, saluran balasan, paket)` dipanggil untuk setiap paket dari HP.
pub fn spawn<F>(deliver: F)
where
    F: Fn(u64, UnboundedSender<Vec<u8>>, Vec<u8>) + Send + 'static,
{
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Bluetooth tidak bisa dijalankan: {e}");
                return;
            }
        };
        rt.block_on(async move {
            loop {
                if let Err(e) = run(&deliver).await {
                    eprintln!("Bluetooth: {e}");
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
    });
}

async fn run<F>(deliver: &F) -> Res<()>
where
    F: Fn(u64, UnboundedSender<Vec<u8>>, Vec<u8>),
{
    let manager = Manager::new().await?;
    let adapter = manager.adapters().await?.into_iter().next().ok_or("tidak ada adapter Bluetooth")?;
    let mut events = adapter.events().await?;
    let filter = || ScanFilter { services: vec![SERVICE] };
    adapter.start_scan(filter()).await?;
    while let Some(ev) = events.next().await {
        let id = match ev {
            CentralEvent::DeviceDiscovered(id)
            | CentralEvent::DeviceUpdated(id)
            | CentralEvent::ServicesAdvertisement { id, .. } => id,
            _ => continue,
        };
        let p = adapter.peripheral(&id).await?;
        let advertises = p.properties().await?.map_or(false, |pr| pr.services.contains(&SERVICE));
        if !advertises || p.is_connected().await.unwrap_or(false) {
            continue;
        }
        adapter.stop_scan().await.ok();
        let sid = NEXT.fetch_add(1, Ordering::Relaxed);
        match session(&p, sid, deliver).await {
            Ok(()) => println!("Bluetooth: HP terputus."),
            Err(e) => println!("Bluetooth: sambungan gagal ({e}). Mencoba lagi..."),
        }
        let _ = p.disconnect().await;
        // buang kejadian lama yang menumpuk selama sesi berjalan
        while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(1), events.next()).await {}
        tokio::time::sleep(Duration::from_millis(500)).await;
        adapter.start_scan(filter()).await?;
    }
    Ok(())
}

async fn session<F>(p: &Peripheral, sid: u64, deliver: &F) -> Res<()>
where
    F: Fn(u64, UnboundedSender<Vec<u8>>, Vec<u8>),
{
    tokio::time::timeout(Duration::from_secs(10), p.connect()).await.map_err(|_| "waktu habis")??;
    p.discover_services().await?;
    let chars = p.characteristics();
    let tx = chars.iter().find(|c| c.uuid == TX).cloned().ok_or("karakteristik TX tidak ada")?;
    let rx = chars.iter().find(|c| c.uuid == RX).cloned().ok_or("karakteristik RX tidak ada")?;
    let mut notes = p.notifications().await?;
    p.subscribe(&tx).await?;
    let name = p
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|pr| pr.local_name)
        .unwrap_or_else(|| "HP".into());
    println!("Bluetooth: tersambung ke {name}, menunggu kode dari aplikasi...");

    let (out_tx, mut out_rx) = unbounded_channel::<Vec<u8>>();
    let mut check = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! {
            n = notes.next() => match n {
                Some(n) if n.uuid == TX => {
                    for pkt in proto::unbatch(&n.value) {
                        deliver(sid, out_tx.clone(), pkt);
                    }
                }
                Some(_) => {}
                None => break,
            },
            out = out_rx.recv() => {
                if let Some(b) = out {
                    p.write(&rx, &b, WriteType::WithoutResponse).await?;
                }
            }
            _ = check.tick() => {
                if !p.is_connected().await.unwrap_or(false) {
                    break;
                }
            }
        }
    }
    deliver(sid, out_tx.clone(), vec![proto::BYE]);
    Ok(())
}
