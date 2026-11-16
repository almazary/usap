//! Usap: jadikan HP Android trackpad untuk Mac.
//! Agen ini menerima sentuhan dari aplikasi Usap di HP lewat Wi-Fi (UDP)
//! atau kabel USB (TCP lewat `adb reverse`) dan menggerakkan kursor macOS.

mod aoa;
mod ble;
mod engine;
mod jitter;
mod mac;
mod proto;

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use engine::{Action, Config, Engine};
use jitter::Jitter;

const DEFAULT_PORT: u16 = 50505;

struct Args {
    port: u16,
    cfg: Config,
    /// opsi yang diberikan lewat perintah, mengalahkan isi file pengaturan
    set_speed: Option<f64>,
    set_scroll: Option<f64>,
    set_natural: Option<bool>,
    new_code: bool,
    dry_run: bool,
    verbose: bool,
    stats: bool,
    no_ble: bool,
    no_aoa: bool,
    /// None = otomatis menyesuaikan Wi-Fi, Some(ms) = tetap.
    buffer_ms: Option<f64>,
}

const HELP: &str = "usap-agent: jadikan HP Android trackpad untuk Mac

Pemakaian: usap-agent [opsi]
  --speed <angka>    kecepatan kursor, bawaan 1.0 (misal 1.5 lebih cepat)
  --scroll <angka>   kecepatan scroll, bawaan 1.0
  --natural / --no-natural   arah scroll
                     (kecepatan dan arah scroll juga bisa diatur di ~/.config/usap/config;
                      perubahan di file itu langsung dipakai tanpa menjalankan ulang agen)
  --port <port>      port UDP, bawaan 50505
  --new-code         buat kode sambung baru
  --buffer <ms>      buffer penghalus Wi-Fi tetap (0 = mati); bawaan otomatis
  --no-ble           jangan cari HP lewat Bluetooth
  --no-aoa           jangan ajak HP yang tercolok masuk mode kabel tanpa mode developer
  --stats            tampilkan kualitas sambungan setiap 2 detik
                     (juga ditulis ke ~/.config/usap/stats.log)
  --dry-run          jangan gerakkan kursor, hanya tampilkan aksi
  -v                 tampilkan setiap aksi";

fn parse_args() -> Args {
    let mut a = Args { port: DEFAULT_PORT, cfg: Config::default(), set_speed: None, set_scroll: None, set_natural: None, new_code: false, dry_run: false, verbose: false, stats: false, no_ble: false, no_aoa: false, buffer_ms: None };
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        let mut num = |name: &str| -> f64 {
            it.next().and_then(|v| v.parse().ok()).unwrap_or_else(|| {
                eprintln!("{name} butuh angka");
                std::process::exit(1)
            })
        };
        match s.as_str() {
            "--speed" => a.set_speed = Some(num("--speed")),
            "--scroll" => a.set_scroll = Some(num("--scroll")),
            "--port" => a.port = num("--port") as u16,
            "--no-natural" => a.set_natural = Some(false),
            "--natural" => a.set_natural = Some(true),
            "--new-code" => a.new_code = true,
            "--dry-run" => a.dry_run = true,
            "--stats" => a.stats = true,
            "--no-ble" => a.no_ble = true,
            "--no-aoa" => a.no_aoa = true,
            "--buffer" => a.buffer_ms = Some(num("--buffer").clamp(0.0, 200.0)),
            "-v" | "--verbose" => a.verbose = true,
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0)
            }
            other => {
                eprintln!("opsi tidak dikenal: {other}\n\n{HELP}");
                std::process::exit(1)
            }
        }
    }
    a
}

fn code_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config/usap/code")
}

/// Kode 6 angka untuk menyambungkan HP. Disimpan supaya tidak perlu diketik ulang setiap kali.
fn load_code(new: bool) -> String {
    let path = code_path();
    if !new {
        if let Ok(c) = std::fs::read_to_string(&path) {
            let c = c.trim().to_string();
            if c.len() == 6 && c.chars().all(|ch| ch.is_ascii_digit()) {
                return c;
            }
        }
    }
    let mut buf = [0u8; 4];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut buf);
    }
    let code = format!("{:06}", u32::from_le_bytes(buf) % 1_000_000);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, &code).is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    code
}

const CONFIG_TEMPLATE: &str = "# Pengaturan Usap. Simpan file ini, agen langsung memakai nilai baru.
# Kecepatan kursor. 1.0 = bawaan, 1.5 = lebih jauh, 0.7 = lebih pelan.
speed = 1.0
# Kecepatan scroll.
scroll = 1.0
# Arah scroll. true = seperti trackpad Mac (natural), false = terbalik.
natural = true
# Tekan dan seret satu jari: tahan jari diam selama sekian detik sampai HP bergetar, lalu geser.
# 0 = mati (bawaan). Untuk memblok teks, tahan telunjuk lalu geser jari tengah.
hold_drag = 0
# Ketuk lalu segera tempel dan geser untuk menyeret. false = mati (bawaan).
tap_drag = false
";

fn config_path() -> PathBuf {
    code_path().with_file_name("config")
}

fn config_mtime() -> Option<std::time::SystemTime> {
    std::fs::metadata(config_path()).and_then(|m| m.modified()).ok()
}

/// Baca ~/.config/usap/config (dibuat kalau belum ada), lalu terapkan opsi dari perintah.
fn load_config(args: &Args) -> Config {
    let path = config_path();
    if !path.exists() {
        let _ = std::fs::write(&path, CONFIG_TEMPLATE);
    }
    let mut c = Config::default();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "speed" => c.speed = v.parse::<f64>().map(|x| x.clamp(0.1, 10.0)).unwrap_or(c.speed),
                "scroll" => c.scroll_speed = v.parse::<f64>().map(|x| x.clamp(0.1, 10.0)).unwrap_or(c.scroll_speed),
                "natural" => c.natural = !matches!(v, "false" | "no" | "0" | "tidak"),
                "tap_drag" => c.tap_drag = matches!(v, "true" | "yes" | "1" | "ya"),
                "hold_drag" => c.hold_drag = v.parse::<f64>().map(|x| x.clamp(0.0, 2.0)).unwrap_or(c.hold_drag),
                _ => {}
            }
        }
    }
    if let Some(x) = args.set_speed { c.speed = x; }
    if let Some(x) = args.set_scroll { c.scroll_speed = x; }
    if let Some(x) = args.set_natural { c.natural = x; }
    c
}

fn describe(c: &Config) -> String {
    format!(
        "kursor {:.2}, scroll {:.2}, arah scroll {}, tekan-seret {}, ketuk-seret {}",
        c.speed,
        c.scroll_speed,
        if c.natural { "natural" } else { "terbalik" },
        if c.hold_drag > 0.0 { format!("tahan {:.2} detik", c.hold_drag) } else { "mati".into() },
        if c.tap_drag { "nyala" } else { "mati" }
    )
}

fn command_out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !s.is_empty()).then_some(s)
}

fn computer_name() -> String {
    command_out("scutil", &["--get", "ComputerName"]).unwrap_or_else(|| "Mac".into())
}

fn local_ips() -> Vec<String> {
    ["en0", "en1", "en2"].iter().filter_map(|i| command_out("ipconfig", &["getifaddr", i])).collect()
}


/// Asal paket: Wi-Fi (UDP), kabel USB lewat adb (TCP), kabel tanpa mode developer (AOA) atau Bluetooth.
#[derive(Clone)]
enum Source {
    Aoa(u64, aoa::Handle, u8),
    Udp(SocketAddr),
    Usb(u64, Arc<Mutex<TcpStream>>),
    Ble(u64, tokio::sync::mpsc::UnboundedSender<Vec<u8>>),
}

impl PartialEq for Source {
    fn eq(&self, o: &Source) -> bool {
        match (self, o) {
            (Source::Udp(a), Source::Udp(b)) => a == b,
            (Source::Usb(a, _), Source::Usb(b, _)) => a == b,
            (Source::Ble(a, _), Source::Ble(b, _)) => a == b,
            (Source::Aoa(a, ..), Source::Aoa(b, ..)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Source::Udp(a) => write!(f, "Wi-Fi {a}"),
            Source::Usb(..) => write!(f, "kabel USB"),
            Source::Ble(..) => write!(f, "Bluetooth"),
            Source::Aoa(..) => write!(f, "kabel"),
        }
    }
}

struct Agent {
    udp: UdpSocket,
    args: Args,
    code: String,
    name: String,
    engine: Engine,
    mac: Option<mac::Mac>,
    peer: Option<Source>,
    last_seq: Option<u32>,
    last_touch_rx: Instant,
    last_rx: Instant,
    wrong: Vec<Instant>,
    locked_until: Option<Instant>,
    quiet_logged: bool,
    jitter: Jitter,
    queue: VecDeque<(f64, Queued)>,
    stats_log: Option<std::fs::File>,
}

enum Queued {
    Touch(proto::TouchPacket),
    /// Android membatalkan sentuhan: lepaskan jari tanpa dianggap ketukan.
    Cancel,
}

impl Agent {
    fn send(&self, to: &Source, b: &[u8]) {
        match to {
            Source::Udp(a) => {
                let _ = self.udp.send_to(b, a);
            }
            Source::Usb(_, s) => {
                let mut f = Vec::with_capacity(b.len() + 2);
                f.extend((b.len() as u16).to_le_bytes());
                f.extend_from_slice(b);
                if let Ok(mut s) = s.lock() {
                    let _ = s.write_all(&f);
                }
            }
            Source::Ble(_, tx) => {
                let _ = tx.send(b.to_vec());
            }
            Source::Aoa(_, h, ep) => aoa::write(h, *ep, b),
        }
    }

    fn reply(&self, to: &Source, kind: u8, extra: &[u8]) {
        let mut b = vec![kind];
        b.extend_from_slice(extra);
        self.send(to, &b);
    }

    fn apply(&mut self, actions: Vec<Action>) {
        for a in actions {
            if self.args.verbose {
                println!("  {a:?}");
            }
            match a {
                Action::Haptic => {
                    if let Some(p) = self.peer.clone() {
                        self.send(&p, &[proto::HAPTIC, 1]);
                    }
                }
                _ => {
                    if let Some(m) = self.mac.as_mut() {
                        m.apply(a);
                    }
                }
            }
        }
    }

    fn process_due(&mut self) {
        let now = self.jitter.now();
        while let Some((due, _)) = self.queue.front() {
            if *due > now {
                break;
            }
            let (_, q) = self.queue.pop_front().unwrap();
            self.run(q);
        }
    }

    fn run(&mut self, q: Queued) {
        let a = match q {
            Queued::Touch(p) => self.engine.touch(p.t, &p.touches),
            Queued::Cancel => self.engine.release(),
        };
        self.note();
        self.apply(a);
    }

    /// Tulis catatan diagnosa dari mesin gestur (misalnya kenapa klik tahan dimulai).
    fn note(&mut self) {
        if let Some(n) = self.engine.take_note() {
            if self.args.stats || self.args.verbose {
                println!("{n}");
            }
            if let Some(f) = self.stats_log.as_mut() {
                let _ = writeln!(f, "{n}");
            }
        }
    }

    fn drop_touch(&mut self) {
        // sentuhan yang masih antre tetap dijalankan dulu supaya klik terakhir tidak hilang
        while let Some((_, q)) = self.queue.pop_front() {
            self.run(q);
        }
        self.jitter.reset();
        let a = self.engine.release();
        self.apply(a);
        if let Some(m) = self.mac.as_mut() {
            m.release_all();
        }
    }

    fn on_packet(&mut self, b: &[u8], from: Source) {
        let Some(&kind) = b.first() else { return };
        let from_peer = self.peer.as_ref() == Some(&from);
        match kind {
            proto::DISCOVER => self.reply(&from, proto::HERE, self.name.as_bytes()),
            proto::HELLO => {
                let Some((code, phone)) = proto::parse_hello(b) else { return };
                let now = Instant::now();
                if self.locked_until.map_or(false, |u| now < u) {
                    self.reply(&from, proto::WELCOME, &[2]);
                    return;
                }
                // HP yang tercolok langsung ke Mac lewat kabel aksesori sudah pasti milik pemakai Mac ini
                let trusted = matches!(from, Source::Aoa(..));
                if trusted || code == self.code {
                    self.drop_touch();
                    self.wrong.clear();
                    let new_peer = !from_peer;
                    self.peer = Some(from.clone());
                    self.last_seq = None;
                    self.last_rx = now;
                    self.quiet_logged = false;
                    let mut extra = vec![1u8];
                    extra.extend_from_slice(self.name.as_bytes());
                    self.reply(&from, proto::WELCOME, &extra);
                    if new_peer {
                        println!("Tersambung: {phone} lewat {from}");
                    }
                } else {
                    self.wrong.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
                    self.wrong.push(now);
                    println!("Kode salah dari {from} ({phone})");
                    if self.wrong.len() >= 5 {
                        self.locked_until = Some(now + Duration::from_secs(60));
                        self.wrong.clear();
                        println!("Terlalu banyak kode salah. Sambungan baru dikunci 60 detik.");
                    }
                    self.reply(&from, proto::WELCOME, &[0]);
                }
            }
            proto::TOUCH if from_peer => {
                let Some(p) = proto::parse_touch(b) else { return };
                if let Some(last) = self.last_seq {
                    if (p.seq.wrapping_sub(last) as i32) <= 0 {
                        return; // paket lama atau dobel
                    }
                }
                self.last_seq = Some(p.seq);
                self.last_touch_rx = Instant::now();
                self.last_rx = self.last_touch_rx;
                let due = self.jitter.schedule(p.t);
                if self.args.buffer_ms == Some(0.0) {
                    self.run(Queued::Touch(p));
                } else {
                    self.queue.push_back((due, Queued::Touch(p)));
                    self.process_due();
                }
            }
            proto::PING if from_peer => {
                self.last_rx = Instant::now();
                self.quiet_logged = false;
                self.reply(&from, proto::PONG, &[]);
            }
            proto::NOTE if from_peer => {
                if b.get(1) == Some(&proto::NOTE_CANCEL) {
                    self.jitter.cancels += 1;
                    // dijalankan setelah sentuhan yang masih antre, sebelum paket "semua jari diangkat"
                    let due = self.queue.back().map_or(0.0, |(d, _)| *d);
                    self.queue.push_back((due, Queued::Cancel));
                    self.process_due();
                    if self.args.verbose {
                        println!("  Android memotong sentuhan (gestur sistem atau tepi layar)");
                    }
                }
            }
            proto::BYE if from_peer => {
                self.drop_touch();
                self.peer = None;
                println!("HP memutus sambungan ({from}).");
            }
            _ => {}
        }
    }
}

type Inbox = Sender<(Vec<u8>, Source)>;

fn spawn_udp(sock: UdpSocket, tx: Inbox) {
    thread::spawn(move || {
        let mut buf = [0u8; 512];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if tx.send((buf[..n].to_vec(), Source::Udp(from))).is_err() {
                        break;
                    }
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {}
                Err(e) => {
                    eprintln!("gagal menerima paket Wi-Fi: {e}");
                    thread::sleep(Duration::from_millis(200));
                }
            }
        }
    });
}

/// Kabel USB: HP tersambung ke 127.0.0.1 lewat `adb reverse`. Setiap paket diawali panjang u16.
fn spawn_usb(listener: TcpListener, tx: Inbox) {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            let _ = s.set_nodelay(true);
            let Ok(w) = s.try_clone() else { continue };
            let src = Source::Usb(NEXT.fetch_add(1, Ordering::Relaxed), Arc::new(Mutex::new(w)));
            let tx = tx.clone();
            thread::spawn(move || {
                let mut len = [0u8; 2];
                let mut buf = vec![0u8; 512];
                loop {
                    if s.read_exact(&mut len).is_err() {
                        break;
                    }
                    let n = u16::from_le_bytes(len) as usize;
                    if n == 0 || n > buf.len() || s.read_exact(&mut buf[..n]).is_err() {
                        break;
                    }
                    if tx.send((buf[..n].to_vec(), src.clone())).is_err() {
                        return;
                    }
                }
                // kabel dicabut atau aplikasi ditutup
                let _ = tx.send((vec![proto::BYE], src));
            });
        }
    });
}

fn adb_path() -> Option<&'static str> {
    ["adb", "/opt/homebrew/bin/adb", "/usr/local/bin/adb"]
        .into_iter()
        .find(|p| Command::new(p).arg("version").output().map_or(false, |o| o.status.success()))
}

/// Pasang `adb reverse` setiap kali HP dicolok, supaya aplikasi di HP bisa menyambung ke 127.0.0.1.
fn spawn_adb_reverse(port: u16) {
    let Some(adb) = adb_path() else {
        println!("  USB      : adb tidak ditemukan, mode kabel USB tidak aktif (brew install android-platform-tools)");
        return;
    };
    println!("  USB      : aktif. Colok HP (USB debugging menyala), lalu pilih Sambungkan lewat kabel USB di aplikasi.");
    thread::spawn(move || {
        let spec = format!("tcp:{port}");
        let mut attached = false;
        loop {
            let present = Command::new(adb)
                .args(["get-state"])
                .output()
                .map_or(false, |o| String::from_utf8_lossy(&o.stdout).trim() == "device");
            if present {
                let listed = Command::new(adb)
                    .args(["reverse", "--list"])
                    .output()
                    .map_or(false, |o| String::from_utf8_lossy(&o.stdout).contains(&spec));
                if !listed {
                    let ok = Command::new(adb).args(["reverse", &spec, &spec]).output().map_or(false, |o| o.status.success());
                    if ok && !attached {
                        println!("HP terdeteksi lewat kabel USB.");
                    }
                }
            } else if attached {
                println!("Kabel USB dilepas.");
            }
            attached = present;
            thread::sleep(Duration::from_secs(2));
        }
    });
}

fn main() {
    let mut args = parse_args();
    let code = load_code(args.new_code);
    args.cfg = load_config(&args);
    let mut cfg_mtime = config_mtime();
    let mut cfg_checked = Instant::now();

    let mac = if args.dry_run {
        None
    } else {
        if !mac::ensure_accessibility() {
            eprintln!(
                "Agen butuh izin Accessibility untuk menggerakkan kursor.\n\
                 Buka System Settings > Privacy & Security > Accessibility, nyalakan aplikasi terminal\n\
                 yang menjalankan agen ini (misalnya Alacritty), lalu jalankan ulang."
            );
            std::process::exit(2);
        }
        Some(mac::Mac::new())
    };

    let udp = match UdpSocket::bind(("0.0.0.0", args.port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Tidak bisa membuka port UDP {}: {e}. Apakah usap-agent sudah berjalan di jendela lain?", args.port);
            std::process::exit(1);
        }
    };
    let usb = TcpListener::bind(("127.0.0.1", args.port)).ok();

    let name = computer_name();
    let ips = local_ips();
    println!("Usap agen siap{}", if args.dry_run { " (dry-run, kursor tidak digerakkan)" } else { "" });
    println!("  Komputer : {name}");
    println!("  Alamat   : {}", if ips.is_empty() { "-".into() } else { ips.join(", ") });
    println!("  Port     : {}", args.port);
    println!("  Kode     : {code}");
    println!("  Setelan  : {} (ubah di {})", describe(&args.cfg), config_path().display());

    let (tx, rx) = mpsc::channel();
    spawn_udp(udp.try_clone().expect("udp clone"), tx.clone());
    if let Some(l) = usb {
        spawn_usb(l, tx.clone());
        spawn_adb_reverse(args.port);
    }
    if args.no_aoa {
        println!("  Kabel    : mati (--no-aoa)");
    } else {
        println!("  Kabel    : aktif. Colok HP ke Mac (tanpa mode developer), Usap terbuka sendiri.");
        let atx = tx.clone();
        aoa::spawn(move |sid, h, ep, pkt| {
            let _ = atx.send((pkt, Source::Aoa(sid, h, ep)));
        });
    }
    if args.no_ble {
        println!("  Bluetooth: mati (--no-ble)");
    } else {
        println!("  Bluetooth: aktif. Di aplikasi pilih Sambungkan lewat Bluetooth; agen akan menyambung sendiri.");
        let btx = tx.clone();
        ble::spawn(move |sid, out, pkt| {
            let _ = btx.send((pkt, Source::Ble(sid, out)));
        });
    }
    println!("Buka aplikasi Usap di HP, pilih komputer ini, lalu masukkan kodenya. Ctrl+C untuk berhenti.");

    let now = Instant::now();
    let engine = Engine::new(args.cfg);
    let stats_log = if args.stats {
        let path = code_path().with_file_name("stats.log");
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    } else {
        None
    };
    let jitter = Jitter::new(now, args.buffer_ms);
    let mut agent = Agent {
        udp,
        args,
        code,
        name,
        engine,
        mac,
        peer: None,
        last_seq: None,
        last_touch_rx: now,
        last_rx: now,
        wrong: Vec::new(),
        locked_until: None,
        quiet_logged: false,
        jitter,
        queue: VecDeque::new(),
        stats_log,
    };

    let mut last_tick = Instant::now();
    loop {
        let wait = agent
            .queue
            .front()
            .map(|(due, _)| (due - agent.jitter.now()).clamp(0.5, 4.0))
            .unwrap_or(4.0);
        match rx.recv_timeout(Duration::from_micros((wait * 1000.0) as u64)) {
            Ok((b, from)) => agent.on_packet(&b, from),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        agent.process_due();
        if cfg_checked.elapsed() >= Duration::from_secs(1) {
            cfg_checked = Instant::now();
            let m = config_mtime();
            if m != cfg_mtime {
                cfg_mtime = m;
                let c = load_config(&agent.args);
                agent.engine.set_config(c);
                println!("Setelan baru dipakai: {}", describe(&c));
            }
        }
        if agent.args.stats {
            if let Some(line) = agent.jitter.report() {
                let line = match &agent.peer {
                    Some(Source::Usb(..)) => format!("[USB]   {line}"),
                    Some(Source::Udp(_)) => format!("[Wi-Fi] {line}"),
                    Some(Source::Ble(..)) => format!("[BLE]   {line}"),
                    Some(Source::Aoa(..)) => format!("[Kabel] {line}"),
                    None => line,
                };
                println!("{line}");
                if let Some(f) = agent.stats_log.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
        }

        let now = Instant::now();
        let dt = now.duration_since(last_tick).as_secs_f64();
        if dt >= 0.004 {
            last_tick = now;
            let a = agent.engine.tick(dt);
            agent.note();
            agent.apply(a);
        }

        if agent.engine.touching() && now.duration_since(agent.last_touch_rx) > Duration::from_millis(800) {
            agent.drop_touch();
        }
        if agent.peer.is_some() && !agent.quiet_logged && now.duration_since(agent.last_rx) > Duration::from_secs(5) {
            agent.quiet_logged = true;
            println!("HP tidak terdengar lagi (layar mati atau keluar dari Wi-Fi?). Menunggu...");
        }
    }
}
