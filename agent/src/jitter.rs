//! Penghalus jaringan: meratakan paket yang datang bergerombol.

use std::collections::VecDeque;
use std::time::Instant;

/// Penghalus Wi-Fi. Paket yang datang bergerombol dijadwalkan ulang sesuai waktu sentuhan aslinya
/// di HP, ditambah jeda kecil yang mengikuti seberapa tidak rata kedatangan paket belakangan ini.
pub struct Jitter {
    epoch: Instant,
    /// (waktu tiba ms, selisih tiba - waktu HP) untuk mencari selisih minimum 5 detik terakhir.
    offsets: VecDeque<(f64, f64)>,
    base: Option<f64>,
    samples: VecDeque<(f64, f64)>,
    target: f64,
    fixed: Option<f64>,
    // statistik
    count: u32,
    /// berapa kali Android memotong sentuhan (misalnya gestur kembali dari tepi layar)
    pub cancels: u32,
    max_gap: f64,
    last_arrival: Option<f64>,
    stat_start: f64,
}

impl Jitter {
    pub fn new(epoch: Instant, fixed: Option<f64>) -> Self {
        Jitter {
            epoch,
            offsets: VecDeque::new(),
            base: None,
            samples: VecDeque::new(),
            target: fixed.unwrap_or(15.0),
            fixed,
            count: 0,
            cancels: 0,
            max_gap: 0.0,
            last_arrival: None,
            stat_start: 0.0,
        }
    }

    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    pub fn reset(&mut self) {
        self.offsets.clear();
        self.base = None;
        self.last_arrival = None;
    }

    /// Kembalikan waktu (ms sejak agen mulai) kapan paket ini sebaiknya diproses.
    pub fn schedule(&mut self, phone_t: u32) -> f64 {
        let now = self.now();
        let off = now - phone_t as f64;
        // waktu HP bisa melompat (HP restart, u32 berputar): mulai hitung ulang
        if let Some(b) = self.base {
            if (off - b).abs() > 5000.0 {
                self.reset();
            }
        }
        // antrean naik: nilai yang lebih besar dari selisih baru tidak akan pernah jadi minimum lagi
        while let Some(&(_, o)) = self.offsets.back() {
            if o >= off { self.offsets.pop_back(); } else { break; }
        }
        self.offsets.push_back((now, off));
        while let Some(&(at, _)) = self.offsets.front() {
            if now - at > 5000.0 && self.offsets.len() > 1 { self.offsets.pop_front(); } else { break; }
        }
        let base = self.offsets.front().map(|&(_, o)| o).unwrap_or(off);
        self.base = Some(base);

        let late = off - base;
        self.samples.push_back((now, late));
        while let Some(&(at, _)) = self.samples.front() {
            if now - at > 2000.0 { self.samples.pop_front(); } else { break; }
        }
        if self.fixed.is_none() {
            let mut v: Vec<f64> = self.samples.iter().map(|&(_, l)| l).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let p90 = v[(v.len() * 9 / 10).min(v.len() - 1)];
            let want = p90.clamp(4.0, 45.0);
            // naik cepat saat Wi-Fi memburuk, turun pelan saat membaik
            self.target = if want > self.target { want } else { self.target * 0.995 + want * 0.005 };
        }

        if let Some(la) = self.last_arrival {
            self.max_gap = self.max_gap.max(now - la);
        }
        self.last_arrival = Some(now);
        self.count += 1;

        phone_t as f64 + base + self.target
    }

    pub fn report(&mut self) -> Option<String> {
        let now = self.now();
        let span = now - self.stat_start;
        if span < 2000.0 {
            return None;
        }
        self.stat_start = now;
        let mut v: Vec<f64> = self.samples.iter().map(|&(_, l)| l).collect();
        let line = if self.count == 0 || v.is_empty() {
            None
        } else {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let q = |p: usize| v[(v.len() * p / 100).min(v.len() - 1)];
            Some(format!(
                "paket/detik {:>4.0}  telat p50 {:>4.0} ms  p90 {:>4.0} ms  maks {:>4.0} ms  jeda terlama {:>4.0} ms  buffer {:>3.0} ms  dipotong Android {}",
                self.count as f64 * 1000.0 / span,
                q(50),
                q(90),
                v[v.len() - 1],
                self.max_gap,
                self.target,
                self.cancels
            ))
        };
        self.count = 0;
        self.cancels = 0;
        self.max_gap = 0.0;
        line
    }
}

