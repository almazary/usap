//! Mesin gestur: mengubah sentuhan mentah dari HP (posisi jari dalam milimeter)
//! menjadi aksi trackpad (gerak kursor, klik, scroll halus dengan inersia, gestur 3 jari).
//! Murni hitungan, tanpa API macOS, supaya bisa dites.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Button {
    Left,
    Right,
}

/// Fase scroll seperti trackpad asli (kCGScrollPhase*).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    None,
    Began,
    Changed,
    Ended,
}

/// Fase inersia setelah jari dilepas (kCGMomentumScrollPhase*).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Momentum {
    None,
    Begin,
    Continue,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shortcut {
    MissionControl,
    AppExpose,
    SpaceLeft,
    SpaceRight,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// Gerak kursor dalam point layar.
    Move { dx: f64, dy: f64 },
    Button { button: Button, down: bool, clicks: i64 },
    /// Scroll dalam piksel. dy positif = konten turun (lihat bagian atas).
    Scroll { dx: f64, dy: f64, phase: Phase, momentum: Momentum },
    Shortcut(Shortcut),
    /// Minta HP bergetar.
    Haptic,
}

#[derive(Clone, Copy, Debug)]
pub struct Touch {
    pub id: u8,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub speed: f64,
    pub scroll_speed: f64,
    pub natural: bool,
    /// Tahan satu jari diam selama ini (detik) untuk "menekan" tombol, lalu geser untuk menyeret. 0 = mati.
    pub hold_drag: f64,
    /// Ketuk lalu segera tempel dan geser = seret. Mati secara bawaan, karena mengetuk lalu
    /// langsung menggeser kursor (misalnya pindah tab) sering salah dianggap menyeret.
    pub tap_drag: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { speed: 1.0, scroll_speed: 1.0, natural: true, hold_drag: 0.0, tap_drag: false }
    }
}

const TAP_MAX_MS: u32 = 220;
const TAP_MAX_MM: f32 = 2.0;
const TAP_DRAG_GAP_MS: u32 = 300;
const DRAG_START_MM: f32 = 1.5;
const DOUBLE_CLICK_MS: u32 = 400;
const HOLD_MAX_MM: f32 = 1.2;
const SCROLL_START_MM: f64 = 1.0;
const SWIPE_MM: f64 = 15.0;
const VELOCITY_WINDOW_MS: u32 = 80;
const MOMENTUM_START: f64 = 150.0; // piksel per detik
const MOMENTUM_STOP: f64 = 40.0;
const MOMENTUM_TAU: f64 = 0.40; // detik
/// Telunjuk harus sudah diam sekian lama sebelum jari kedua datang supaya dianggap "menahan klik".
/// Saat scroll dua jari, kedua jari biasanya menempel hampir bersamaan.
const ANCHOR_STILL_MS: u32 = 250;
/// Kalau telunjuk ikut bergeser sejauh ini sebelum blok dimulai, itu scroll dua jari biasa.
const ANCHOR_SLIP_MM: f32 = 1.5;
/// Jari kedua harus bergeser sejauh ini sebelum klik ditekan (blok dimulai).
const ANCHOR_DRAG_MM: f64 = 3.5;
/// Saat klik mulai ditekan, telunjuk paling banyak boleh bergeser sebagian kecil dari gerak jari tengah.
/// Saat scroll, kedua jari bergerak hampir sama jauh.
const ANCHOR_SLIP_RATIO: f64 = 0.3;
/// Telunjuk dan jari tengah berdekatan. Jempol yang memegang HP di tepi layar biasanya jauh lebih jauh.
const ANCHOR_MAX_SPREAD_MM: f32 = 45.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Idle,
    Pointer,
    Scroll,
    Swipe,
    /// Satu jari ditahan diam (menahan klik), jari lain menggeser kursor: memblok teks.
    Anchor,
    /// Jumlah jari berubah di tengah gestur: abaikan sampai semua jari diangkat.
    Locked,
}

pub struct Engine {
    cfg: Config,
    touching: bool,
    mode: Mode,
    start_ms: u32,
    max_fingers: usize,
    start: Vec<Touch>,
    max_travel: f32,
    prev: Vec<Touch>,
    prev_t: u32,
    speed_ewma: f64,
    scroll_active: bool,
    scroll_pending: f64,
    scroll_samples: VecDeque<(u32, f64, f64)>,
    swipe: (f64, f64),
    swipe_fired: bool,
    last_tap_end: Option<u32>,
    last_click: Option<(u32, i64)>,
    drag_candidate: bool,
    dragging: bool,
    held: f64,
    momentum: Option<(f64, f64)>,
    momentum_started: bool,
    /// Posisi dan waktu terakhir satu jari mulai diam (untuk mengenali jari penahan).
    still_ref: (f32, f32),
    still_since: u32,
    anchor: Option<Anchor>,
    anchor_used: bool,
    /// Catatan diagnosa untuk stats.log (kapan dan kenapa klik tahan dimulai).
    note: Option<String>,
}

#[derive(Clone, Copy, Debug)]
struct Anchor {
    id: u8,
    x: f32,
    y: f32,
    mover_travel: f64,
    gap_ms: u32,
    spread: f32,
}

fn age(now: u32, then: u32) -> u32 {
    now.wrapping_sub(then)
}

fn centroid(ts: &[Touch]) -> (f64, f64) {
    let n = ts.len().max(1) as f64;
    let (sx, sy) = ts.iter().fold((0.0, 0.0), |(x, y), t| (x + t.x as f64, y + t.y as f64));
    (sx / n, sy / n)
}

fn same_ids(a: &[Touch], b: &[Touch]) -> bool {
    a.len() == b.len() && a.iter().all(|t| b.iter().any(|u| u.id == t.id))
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Engine {
            cfg,
            touching: false,
            mode: Mode::Idle,
            start_ms: 0,
            max_fingers: 0,
            start: Vec::new(),
            max_travel: 0.0,
            prev: Vec::new(),
            prev_t: 0,
            speed_ewma: 0.0,
            scroll_active: false,
            scroll_pending: 0.0,
            scroll_samples: VecDeque::new(),
            swipe: (0.0, 0.0),
            swipe_fired: false,
            last_tap_end: None,
            last_click: None,
            drag_candidate: false,
            dragging: false,
            held: 0.0,
            momentum: None,
            momentum_started: false,
            still_ref: (0.0, 0.0),
            still_since: 0,
            anchor: None,
            anchor_used: false,
            note: None,
        }
    }

    /// Ambil catatan diagnosa terakhir, kalau ada.
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
    }

    pub fn touching(&self) -> bool {
        self.touching
    }

    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    /// Point layar per milimeter gerak jari, naik seiring kecepatan (akselerasi kursor).
    /// Layar HP jauh lebih kecil dari trackpad Mac, jadi gerak pelan pun sudah cukup jauh.
    fn pointer_gain(&self, speed_mm_s: f64) -> f64 {
        let t = ((speed_mm_s - 10.0) / (220.0 - 10.0)).clamp(0.0, 1.0);
        (5.0 + 21.0 * t.powf(1.3)) * self.cfg.speed
    }

    fn scroll_gain(&self, speed_mm_s: f64) -> f64 {
        9.0 * (1.0 + speed_mm_s / 500.0).min(2.5) * self.cfg.scroll_speed
    }

    fn mode_for(n: usize) -> Mode {
        match n {
            1 => Mode::Pointer,
            2 => Mode::Scroll,
            3 => Mode::Swipe,
            _ => Mode::Locked,
        }
    }

    /// Satu snapshot berisi semua jari yang sedang menempel. Kosong = semua diangkat.
    pub fn touch(&mut self, t: u32, touches: &[Touch]) -> Vec<Action> {
        let mut out = Vec::new();
        let n = touches.len();

        if !self.touching {
            if n == 0 {
                return out;
            }
            self.stop_momentum(&mut out);
            self.touching = true;
            self.start_ms = t;
            self.max_fingers = n;
            self.start = touches.to_vec();
            self.max_travel = 0.0;
            self.prev = touches.to_vec();
            self.prev_t = t;
            self.speed_ewma = 0.0;
            self.scroll_pending = 0.0;
            self.swipe = (0.0, 0.0);
            self.swipe_fired = false;
            self.drag_candidate = self.cfg.tap_drag
                && n == 1
                && self.last_tap_end.map_or(false, |e| age(t, e) < TAP_DRAG_GAP_MS);
            self.mode = Self::mode_for(n);
            self.held = 0.0;
            self.still_ref = (touches[0].x, touches[0].y);
            self.still_since = t;
            self.anchor = None;
            self.anchor_used = false;
            return out;
        }

        if n == 0 {
            self.end_session(t, &mut out);
            return out;
        }

        for tc in touches {
            if !self.start.iter().any(|s| s.id == tc.id) {
                self.start.push(*tc);
            }
            if let Some(s) = self.start.iter().find(|s| s.id == tc.id) {
                let d = ((tc.x - s.x).powi(2) + (tc.y - s.y).powi(2)).sqrt();
                self.max_travel = self.max_travel.max(d);
            }
        }
        self.max_fingers = self.max_fingers.max(n);

        if !same_ids(touches, &self.prev) {
            if self.mode == Mode::Anchor {
                self.anchor_fingers_changed(touches, &mut out);
                self.prev = touches.to_vec();
                self.prev_t = t;
                return out;
            }
            // telunjuk sudah diam, lalu jari kedua menempel: mungkin mau memblok teks
            if self.mode == Mode::Pointer
                && !self.dragging
                && n == 2
                && self.prev.len() == 1
                && age(t, self.still_since) >= ANCHOR_STILL_MS
            {
                let a = touches.iter().find(|tc| tc.id == self.prev[0].id).copied();
                let m = touches.iter().find(|tc| tc.id != self.prev[0].id).copied();
                let spread = match (a, m) {
                    (Some(a), Some(m)) => ((a.x - m.x).powi(2) + (a.y - m.y).powi(2)).sqrt(),
                    _ => f32::MAX,
                };
                if let (Some(a), true) = (a, spread <= ANCHOR_MAX_SPREAD_MM) {
                    let gap_ms = age(t, self.still_since);
                    self.anchor = Some(Anchor { id: a.id, x: a.x, y: a.y, mover_travel: 0.0, gap_ms, spread });
                    self.anchor_used = true;
                    self.mode = Mode::Anchor;
                    self.prev = touches.to_vec();
                    self.prev_t = t;
                    return out;
                }
            }
            let next = match (self.mode, n) {
                (Mode::Pointer, _) if self.dragging => Mode::Pointer,
                (Mode::Pointer, 2) => Mode::Scroll,
                (Mode::Pointer | Mode::Scroll, 3) => Mode::Swipe,
                (Mode::Pointer, 1) => Mode::Pointer,
                _ => Mode::Locked,
            };
            if self.mode == Mode::Scroll && next != Mode::Scroll {
                if touches.len() < self.prev.len() {
                    // jari diangkat satu per satu: anggap scroll selesai dan beri inersia
                    self.finish_scroll(t, &mut out);
                } else {
                    // jari ketiga menempel: berganti gestur, tanpa inersia
                    self.end_scroll(&mut out);
                }
            }
            if next == Mode::Swipe && self.mode != Mode::Swipe {
                self.swipe = (0.0, 0.0);
            }
            self.mode = next;
            self.prev = touches.to_vec();
            self.prev_t = t;
            return out;
        }

        let dt_ms = age(t, self.prev_t).max(1) as f64;
        let (dx, dy) = if self.mode == Mode::Anchor {
            self.mover_delta(touches)
        } else {
            let (cx, cy) = centroid(touches);
            let (px, py) = centroid(&self.prev);
            (cx - px, cy - py)
        };
        let speed = (dx * dx + dy * dy).sqrt() / dt_ms * 1000.0;
        self.speed_ewma = 0.5 * self.speed_ewma + 0.5 * speed;

        match self.mode {
            Mode::Pointer => {
                let p = touches[0];
                let (sx, sy) = self.still_ref;
                if ((p.x - sx).powi(2) + (p.y - sy).powi(2)).sqrt() > HOLD_MAX_MM {
                    self.still_ref = (p.x, p.y);
                    self.still_since = t;
                }
                if self.drag_candidate && !self.dragging && self.max_travel > DRAG_START_MM {
                    self.note = Some("klik tahan: ketuk lalu seret (jari menempel lagi < 0,3 detik setelah ketukan)".into());
                    self.dragging = true;
                    out.push(Action::Button { button: Button::Left, down: true, clicks: 1 });
                }
                let g = self.pointer_gain(self.speed_ewma);
                if dx != 0.0 || dy != 0.0 {
                    out.push(Action::Move { dx: dx * g, dy: dy * g });
                }
            }
            Mode::Scroll => {
                let g = self.scroll_gain(self.speed_ewma);
                let sign = if self.cfg.natural { 1.0 } else { -1.0 };
                let (sx, sy) = (dx * g * sign, dy * g * sign);
                if !self.scroll_active {
                    self.scroll_pending += (dx * dx + dy * dy).sqrt();
                    if self.scroll_pending >= SCROLL_START_MM {
                        self.scroll_active = true;
                        out.push(Action::Scroll { dx: sx, dy: sy, phase: Phase::Began, momentum: Momentum::None });
                        self.scroll_samples.push_back((t, sx, sy));
                    }
                } else if sx != 0.0 || sy != 0.0 {
                    out.push(Action::Scroll { dx: sx, dy: sy, phase: Phase::Changed, momentum: Momentum::None });
                    self.scroll_samples.push_back((t, sx, sy));
                }
                while let Some(&(st, _, _)) = self.scroll_samples.front() {
                    if age(t, st) > VELOCITY_WINDOW_MS {
                        self.scroll_samples.pop_front();
                    } else {
                        break;
                    }
                }
            }
            Mode::Swipe => {
                self.swipe.0 += dx;
                self.swipe.1 += dy;
                let (ax, ay) = self.swipe;
                if !self.swipe_fired && (ax * ax + ay * ay).sqrt() >= SWIPE_MM {
                    self.swipe_fired = true;
                    let s = if ay.abs() >= ax.abs() {
                        if ay < 0.0 { Shortcut::MissionControl } else { Shortcut::AppExpose }
                    } else if ax < 0.0 {
                        // jari ke kiri = pindah ke desktop sebelah kanan, seperti trackpad Mac
                        Shortcut::SpaceRight
                    } else {
                        Shortcut::SpaceLeft
                    };
                    out.push(Action::Shortcut(s));
                    out.push(Action::Haptic);
                }
            }
            Mode::Anchor => {
                if self.dragging {
                    let g = self.pointer_gain(self.speed_ewma);
                    if dx != 0.0 || dy != 0.0 {
                        out.push(Action::Move { dx: dx * g, dy: dy * g });
                    }
                } else if let Some(mut a) = self.anchor {
                    let slip = touches
                        .iter()
                        .find(|tc| tc.id == a.id)
                        .map_or(0.0, |p| ((p.x - a.x).powi(2) + (p.y - a.y).powi(2)).sqrt());
                    let travel = a.mover_travel + (dx * dx + dy * dy).sqrt();
                    if slip > ANCHOR_SLIP_MM
                        || (travel >= ANCHOR_DRAG_MM && slip as f64 > travel * ANCHOR_SLIP_RATIO)
                    {
                        // telunjuk ikut bergerak: ternyata scroll dua jari biasa
                        self.anchor = None;
                        self.scroll_pending = 0.0;
                        self.mode = Mode::Scroll;
                    } else {
                        a.mover_travel = travel;
                        if a.mover_travel >= ANCHOR_DRAG_MM {
                            self.note = Some(format!(
                                "klik tahan: telunjuk ditahan (diam {} ms sebelum jari kedua, jarak jari {:.0} mm, telunjuk geser {:.1} mm, jari kedua geser {:.1} mm)",
                                a.gap_ms, a.spread, slip, travel
                            ));
                            self.dragging = true;
                            out.push(Action::Button { button: Button::Left, down: true, clicks: 1 });
                            out.push(Action::Haptic);
                        }
                        self.anchor = Some(a);
                    }
                }
            }
            Mode::Locked | Mode::Idle => {}
        }

        self.prev = touches.to_vec();
        self.prev_t = t;
        out
    }

    /// Gerak rata-rata jari selain jari penahan (jari yang menggeser saat memblok).
    fn mover_delta(&self, touches: &[Touch]) -> (f64, f64) {
        let id = self.anchor.map(|a| a.id);
        let now: Vec<Touch> = touches.iter().copied().filter(|tc| Some(tc.id) != id).collect();
        let before: Vec<Touch> = self.prev.iter().copied().filter(|tc| Some(tc.id) != id).collect();
        if now.is_empty() || before.is_empty() {
            return (0.0, 0.0);
        }
        let (cx, cy) = centroid(&now);
        let (px, py) = centroid(&before);
        (cx - px, cy - py)
    }

    /// Jari bertambah atau berkurang saat mode memblok.
    fn anchor_fingers_changed(&mut self, touches: &[Touch], out: &mut Vec<Action>) {
        let id = self.anchor.map(|a| a.id);
        let anchor_on = touches.iter().any(|tc| Some(tc.id) == id);
        if !anchor_on {
            // telunjuk diangkat: lepas klik, blok selesai
            if self.dragging {
                out.push(Action::Button { button: Button::Left, down: false, clicks: 1 });
                self.dragging = false;
            }
            self.anchor = None;
            self.mode = Mode::Locked;
        } else if touches.len() == 1 && !self.dragging {
            // jari kedua diangkat sebelum blok dimulai: telunjuk kembali menggerakkan kursor
            self.anchor = None;
            self.mode = Mode::Pointer;
        }
        // selain itu: jari kedua diangkat atau ditaruh lagi saat memblok, blok berlanjut
    }

    /// Paksa semua jari dianggap terangkat (misalnya paket terakhir hilang).
    pub fn release(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        self.stop_momentum(&mut out);
        if self.touching {
            let t = self.prev_t.wrapping_add(10_000);
            self.end_session(t, &mut out);
        }
        out
    }

    /// Akhiri scroll karena jari diangkat. Kalau jari masih bergerak cepat saat diangkat,
    /// halaman terus menggulir sebentar lalu melambat (inersia), seperti trackpad Mac.
    fn finish_scroll(&mut self, t: u32, out: &mut Vec<Action>) {
        let recent = self.scroll_samples.back().map_or(false, |&(st, _, _)| age(t, st) <= 60);
        let first = self.scroll_samples.front().map(|&(st, _, _)| st);
        let (sx, sy) = self.scroll_samples.iter().fold((0.0, 0.0), |(a, b), &(_, x, y)| (a + x, b + y));
        self.end_scroll(out);
        if let (true, Some(f)) = (recent, first) {
            let span = (age(t, f).max(16) as f64) / 1000.0;
            let (vx, vy) = (sx / span, sy / span);
            if (vx * vx + vy * vy).sqrt() >= MOMENTUM_START {
                self.momentum = Some((vx, vy));
                self.momentum_started = false;
            }
        }
    }

    fn end_scroll(&mut self, out: &mut Vec<Action>) {
        if self.scroll_active {
            out.push(Action::Scroll { dx: 0.0, dy: 0.0, phase: Phase::Ended, momentum: Momentum::None });
            self.scroll_active = false;
        }
        self.scroll_samples.clear();
    }

    fn end_session(&mut self, t: u32, out: &mut Vec<Action>) {
        let duration = age(t, self.start_ms);
        if self.dragging {
            out.push(Action::Button { button: Button::Left, down: false, clicks: 1 });
            self.dragging = false;
            self.last_tap_end = None;
        } else if self.scroll_active {
            self.finish_scroll(t, out);
            self.last_tap_end = None;
        } else if duration <= TAP_MAX_MS
            && self.max_travel <= TAP_MAX_MM
            && self.mode != Mode::Swipe
            && !self.anchor_used
        {
            match self.max_fingers {
                1 => {
                    let clicks = match self.last_click {
                        Some((lt, c)) if age(t, lt) <= DOUBLE_CLICK_MS => (c + 1).min(3),
                        _ => 1,
                    };
                    out.push(Action::Button { button: Button::Left, down: true, clicks });
                    out.push(Action::Button { button: Button::Left, down: false, clicks });
                    out.push(Action::Haptic);
                    self.last_click = Some((t, clicks));
                    self.last_tap_end = Some(t);
                }
                2 => {
                    out.push(Action::Button { button: Button::Right, down: true, clicks: 1 });
                    out.push(Action::Button { button: Button::Right, down: false, clicks: 1 });
                    out.push(Action::Haptic);
                    self.last_tap_end = None;
                }
                _ => self.last_tap_end = None,
            }
        } else {
            self.last_tap_end = None;
        }
        self.end_scroll(out);
        self.touching = false;
        self.mode = Mode::Idle;
        self.drag_candidate = false;
        self.anchor = None;
        self.prev.clear();
        self.start.clear();
    }

    fn stop_momentum(&mut self, out: &mut Vec<Action>) {
        if self.momentum.take().is_some() && self.momentum_started {
            out.push(Action::Scroll { dx: 0.0, dy: 0.0, phase: Phase::None, momentum: Momentum::End });
        }
        self.momentum_started = false;
    }

    /// Dipanggil terus oleh loop agen (sekitar 120 kali per detik) untuk inersia scroll
    /// dan untuk mendeteksi jari yang ditahan diam (tekan dan seret).
    pub fn tick(&mut self, dt_s: f64) -> Vec<Action> {
        let mut out = Vec::new();
        if self.touching && self.mode == Mode::Pointer && !self.dragging && self.cfg.hold_drag > 0.0 {
            if self.max_travel <= HOLD_MAX_MM {
                self.held += dt_s.clamp(0.0, 0.05);
                if self.held >= self.cfg.hold_drag {
                    self.note = Some("klik tahan: satu jari ditahan diam (hold_drag)".into());
                    self.dragging = true;
                    out.push(Action::Button { button: Button::Left, down: true, clicks: 1 });
                    out.push(Action::Haptic);
                }
            }
        }
        if let Some((vx, vy)) = self.momentum {
            let dt = dt_s.clamp(0.0, 0.05);
            let m = if self.momentum_started { Momentum::Continue } else { Momentum::Begin };
            self.momentum_started = true;
            out.push(Action::Scroll { dx: vx * dt, dy: vy * dt, phase: Phase::None, momentum: m });
            let k = (-dt / MOMENTUM_TAU).exp();
            let (nx, ny) = (vx * k, vy * k);
            if (nx * nx + ny * ny).sqrt() < MOMENTUM_STOP {
                out.push(Action::Scroll { dx: 0.0, dy: 0.0, phase: Phase::None, momentum: Momentum::End });
                self.momentum = None;
                self.momentum_started = false;
            } else {
                self.momentum = Some((nx, ny));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t1(x: f32, y: f32) -> Vec<Touch> {
        vec![Touch { id: 0, x, y }]
    }
    fn t2(x: f32, y: f32) -> Vec<Touch> {
        vec![Touch { id: 0, x, y }, Touch { id: 1, x: x + 20.0, y }]
    }
    fn has(a: &[Action], f: impl Fn(&Action) -> bool) -> bool {
        a.iter().any(f)
    }

    #[test]
    fn tap_is_left_click_then_double_click() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t1(10.0, 10.0));
        let a = e.touch(100, &[]);
        assert!(has(&a, |x| *x == Action::Button { button: Button::Left, down: true, clicks: 1 }));
        e.touch(250, &t1(10.0, 10.0));
        let a = e.touch(330, &[]);
        assert!(has(&a, |x| *x == Action::Button { button: Button::Left, down: true, clicks: 2 }));
    }

    #[test]
    fn two_finger_tap_is_right_click() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t2(10.0, 10.0));
        let a = e.touch(120, &[]);
        assert!(has(&a, |x| matches!(x, Action::Button { button: Button::Right, down: true, .. })));
    }

    #[test]
    fn slow_move_is_precise_fast_move_goes_far() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t1(0.0, 0.0));
        let slow = e.touch(100, &t1(1.0, 0.0)); // 10 mm/s
        let mut e2 = Engine::new(Config::default());
        e2.touch(0, &t1(0.0, 0.0));
        let fast = e2.touch(10, &t1(5.0, 0.0)); // 500 mm/s
        let dx = |a: &[Action]| match a[0] { Action::Move { dx, .. } => dx, _ => 0.0 };
        assert!(dx(&slow) / 1.0 < dx(&fast) / 5.0);
    }

    #[test]
    fn long_press_without_move_is_not_a_click_when_hold_drag_off() {
        let mut e = Engine::new(Config { hold_drag: 0.0, ..Config::default() });
        e.touch(0, &t1(10.0, 10.0));
        for _ in 0..60 {
            assert!(e.tick(0.01).is_empty());
        }
        let a = e.touch(600, &[]);
        assert!(a.is_empty());
    }

    #[test]
    fn hold_still_then_move_drags_with_button_pressed() {
        let mut e = Engine::new(Config { hold_drag: 0.35, ..Config::default() });
        e.touch(0, &t1(10.0, 10.0));
        let mut a = Vec::new();
        for _ in 0..40 {
            a.extend(e.tick(0.01));
        }
        assert_eq!(a, vec![Action::Button { button: Button::Left, down: true, clicks: 1 }, Action::Haptic]);
        let m = e.touch(420, &t1(14.0, 10.0));
        assert!(has(&m, |x| matches!(x, Action::Move { .. })));
        let up = e.touch(600, &[]);
        assert_eq!(up, vec![Action::Button { button: Button::Left, down: false, clicks: 1 }]);
    }

    #[test]
    fn moving_right_away_does_not_press() {
        let mut e = Engine::new(Config { hold_drag: 0.35, ..Config::default() });
        e.touch(0, &t1(10.0, 10.0));
        e.tick(0.05);
        e.touch(50, &t1(13.0, 10.0));
        for _ in 0..60 {
            assert!(e.tick(0.01).is_empty());
        }
    }

    #[test]
    fn two_finger_scroll_has_phases_and_momentum() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t2(10.0, 30.0));
        let mut all = Vec::new();
        for i in 1..=10u32 {
            all.extend(e.touch(i * 8, &t2(10.0, 30.0 - i as f32 * 2.0)));
        }
        all.extend(e.touch(88, &[]));
        assert!(has(&all, |x| matches!(x, Action::Scroll { phase: Phase::Began, .. })));
        assert!(has(&all, |x| matches!(x, Action::Scroll { phase: Phase::Ended, .. })));
        // jari ke atas dengan natural scrolling = konten ikut ke atas (dy negatif)
        assert!(has(&all, |x| matches!(x, Action::Scroll { dy, phase: Phase::Changed, .. } if *dy < 0.0)));
        let first = e.tick(0.008);
        assert!(matches!(first[0], Action::Scroll { momentum: Momentum::Begin, .. }));
        let mut ended = false;
        for _ in 0..1000 {
            if has(&e.tick(0.008), |x| matches!(x, Action::Scroll { momentum: Momentum::End, .. })) {
                ended = true;
                break;
            }
        }
        assert!(ended);
    }

    #[test]
    fn momentum_starts_when_fingers_lift_one_after_another() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t2(10.0, 30.0));
        for i in 1..=10u32 {
            e.touch(i * 8, &t2(10.0, 30.0 - i as f32 * 2.0));
        }
        // seperti di HP sungguhan: satu jari terangkat dulu, jari kedua menyusul
        let a = e.touch(88, &t1(10.0, 10.0));
        assert!(has(&a, |x| matches!(x, Action::Scroll { phase: Phase::Ended, .. })));
        e.touch(110, &[]);
        let m = e.tick(0.008);
        assert!(has(&m, |x| matches!(x, Action::Scroll { momentum: Momentum::Begin, .. })), "tidak ada inersia: {m:?}");
    }

    #[test]
    fn touching_again_stops_momentum() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &t2(10.0, 30.0));
        for i in 1..=10u32 {
            e.touch(i * 8, &t2(10.0, 30.0 - i as f32 * 2.0));
        }
        e.touch(88, &[]);
        e.tick(0.008);
        let a = e.touch(120, &t1(10.0, 10.0));
        assert!(has(&a, |x| matches!(x, Action::Scroll { momentum: Momentum::End, .. })));
        assert!(e.tick(0.008).is_empty());
    }

    #[test]
    fn tap_then_hold_and_move_drags_when_enabled() {
        let mut e = Engine::new(Config { tap_drag: true, ..Config::default() });
        e.touch(0, &t1(10.0, 10.0));
        e.touch(90, &[]);
        e.touch(200, &t1(10.0, 10.0));
        let a = e.touch(230, &t1(13.0, 10.0));
        assert!(has(&a, |x| *x == Action::Button { button: Button::Left, down: true, clicks: 1 }));
        let a = e.touch(400, &[]);
        assert!(has(&a, |x| *x == Action::Button { button: Button::Left, down: false, clicks: 1 }));
    }

    #[test]
    fn three_finger_swipe_up_opens_mission_control_once() {
        let mut e = Engine::new(Config::default());
        let t3 = |y: f32| vec![
            Touch { id: 0, x: 10.0, y },
            Touch { id: 1, x: 25.0, y },
            Touch { id: 2, x: 40.0, y },
        ];
        e.touch(0, &t3(50.0));
        let mut all = Vec::new();
        for i in 1..=10u32 {
            all.extend(e.touch(i * 10, &t3(50.0 - i as f32 * 3.0)));
        }
        let n = all.iter().filter(|x| matches!(x, Action::Shortcut(Shortcut::MissionControl))).count();
        assert_eq!(n, 1);
        assert!(e.touch(120, &[]).is_empty());
    }

    fn tt(a: (f32, f32), b: Option<(u8, f32, f32)>) -> Vec<Touch> {
        let mut v = vec![Touch { id: 0, x: a.0, y: a.1 }];
        if let Some((id, x, y)) = b {
            v.push(Touch { id, x, y });
        }
        v
    }
    fn presses(a: &[Action]) -> usize {
        a.iter().filter(|x| matches!(x, Action::Button { button: Button::Left, down: true, .. })).count()
    }
    fn releases(a: &[Action]) -> usize {
        a.iter().filter(|x| matches!(x, Action::Button { button: Button::Left, down: false, .. })).count()
    }

    #[test]
    fn hold_index_and_slide_middle_finger_selects() {
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        // arahkan kursor dulu, lalu telunjuk diam
        e.touch(0, &tt((10.0, 10.0), None));
        all.extend(e.touch(40, &tt((20.0, 10.0), None)));
        all.extend(e.touch(300, &tt((20.0, 10.0), Some((1, 40.0, 10.0)))));
        assert_eq!(presses(&all), 0, "jari tengah baru menempel, belum memblok");
        let mut drag = Vec::new();
        for i in 1..=8u32 {
            drag.extend(e.touch(300 + i * 10, &tt((20.1, 10.0), Some((1, 40.0 + i as f32, 10.0)))));
        }
        assert!(e.take_note().unwrap().contains("telunjuk ditahan"));
        assert_eq!(presses(&drag), 1);
        assert!(has(&drag, |x| *x == Action::Haptic));
        assert!(has(&drag, |x| matches!(x, Action::Move { dx, .. } if *dx > 0.0)));
        // jari tengah diangkat lalu ditaruh lagi: blok berlanjut
        let mut more = e.touch(400, &tt((20.0, 10.0), None));
        more.extend(e.touch(450, &tt((20.0, 10.0), Some((2, 30.0, 12.0)))));
        for i in 1..=4u32 {
            more.extend(e.touch(450 + i * 10, &tt((20.0, 10.0), Some((2, 30.0 + i as f32 * 2.0, 12.0)))));
        }
        assert_eq!(releases(&more), 0);
        assert!(has(&more, |x| matches!(x, Action::Move { .. })));
        // telunjuk diangkat: klik dilepas
        let up = e.touch(520, &[Touch { id: 2, x: 38.0, y: 12.0 }]);
        assert_eq!(releases(&up), 1);
        assert!(e.touch(560, &[]).is_empty());
    }

    #[test]
    fn staggered_two_finger_scroll_is_still_a_scroll() {
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &tt((10.0, 30.0), None));
        all.extend(e.touch(400, &tt((10.0, 30.0), Some((1, 30.0, 30.0)))));
        for i in 1..=10u32 {
            let y = 30.0 - i as f32 * 1.5;
            all.extend(e.touch(400 + i * 8, &tt((10.0, y), Some((1, 30.0, y)))));
        }
        all.extend(e.touch(300, &[]));
        assert_eq!(presses(&all), 0);
        assert!(has(&all, |x| matches!(x, Action::Scroll { phase: Phase::Began, .. })));
    }

    #[test]
    fn fingers_landing_together_never_select() {
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &tt((10.0, 30.0), None));
        all.extend(e.touch(40, &tt((10.0, 30.0), Some((1, 30.0, 30.0)))));
        for i in 1..=10u32 {
            all.extend(e.touch(40 + i * 8, &tt((10.0, 30.0), Some((1, 30.0 + i as f32, 30.0)))));
        }
        all.extend(e.touch(200, &[]));
        assert_eq!(presses(&all), 0);
    }

    #[test]
    fn tapping_middle_finger_while_holding_index_does_nothing() {
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &tt((10.0, 10.0), None));
        all.extend(e.touch(260, &tt((10.0, 10.0), Some((1, 30.0, 10.0)))));
        all.extend(e.touch(310, &tt((10.0, 10.0), None)));
        all.extend(e.touch(315, &[]));
        assert!(!has(&all, |x| matches!(x, Action::Button { .. })), "{all:?}");
    }

    #[test]
    fn index_moves_cursor_again_after_middle_finger_lifts() {
        let mut e = Engine::new(Config::default());
        e.touch(0, &tt((10.0, 10.0), None));
        e.touch(300, &tt((10.0, 10.0), Some((1, 30.0, 10.0))));
        e.touch(360, &tt((10.0, 10.0), None));
        let a = e.touch(380, &tt((14.0, 10.0), None));
        assert!(has(&a, |x| matches!(x, Action::Move { .. })));
    }

    #[test]
    fn resting_thumb_far_away_is_not_an_anchor() {
        // jempol tangan yang memegang HP diam di tepi, telunjuk menggeser kursor di tengah
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &tt((5.0, 60.0), None));
        all.extend(e.touch(500, &tt((5.0, 60.0), Some((1, 80.0, 30.0)))));
        for i in 1..=10u32 {
            all.extend(e.touch(500 + i * 10, &tt((5.0, 60.0), Some((1, 80.0 + i as f32 * 2.0, 30.0)))));
        }
        assert_eq!(presses(&all), 0);
    }

    #[test]
    fn second_finger_leading_a_scroll_does_not_press() {
        // scroll dua jari, tapi jari kedua bergerak lebih dulu dan telunjuk menyusul pelan
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &tt((10.0, 30.0), None));
        all.extend(e.touch(400, &tt((10.0, 30.0), Some((1, 30.0, 30.0)))));
        for i in 1..=10u32 {
            let f = i as f32;
            all.extend(e.touch(400 + i * 8, &tt((10.0, 30.0 - f * 0.5), Some((1, 30.0, 30.0 - f * 1.2)))));
        }
        assert_eq!(presses(&all), 0, "{all:?}");
    }

    #[test]
    fn tap_then_move_quickly_only_moves_cursor() {
        // klik tab, lalu langsung geser ke tab lain: tidak boleh menyeret
        let mut e = Engine::new(Config::default());
        let mut all = Vec::new();
        e.touch(0, &t1(10.0, 10.0));
        e.touch(90, &[]);
        all.extend(e.touch(200, &t1(10.0, 10.0)));
        for i in 1..=5u32 {
            all.extend(e.touch(200 + i * 10, &t1(10.0 + i as f32 * 2.0, 10.0)));
        }
        all.extend(e.touch(300, &[]));
        assert_eq!(presses(&all), 0, "{all:?}");
        assert!(has(&all, |x| matches!(x, Action::Move { .. })));
    }
}
