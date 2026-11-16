//! Keluaran ke macOS lewat CoreGraphics: gerak kursor, klik, scroll kontinu
//! (dengan fase seperti trackpad asli) dan shortcut Mission Control.

use crate::engine::{Action, Button, Momentum, Phase, Shortcut};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::{CGPoint, CGRect};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// Cek izin Accessibility. Kalau belum ada, macOS menampilkan dialog untuk mengizinkan.
pub fn ensure_accessibility() -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef()) != 0
    }
}

const SCROLL_PHASE: u32 = 99; // kCGScrollWheelEventScrollPhase
const MOMENTUM_PHASE: u32 = 123; // kCGScrollWheelEventMomentumPhase

const KEY_CONTROL: u16 = 59;
const KEY_LEFT: u16 = 123;
const KEY_RIGHT: u16 = 124;
const KEY_DOWN: u16 = 125;
const KEY_UP: u16 = 126;

pub struct Mac {
    src: CGEventSource,
    left_down: bool,
    right_down: bool,
    rem: (f64, f64),
}

fn inside(r: &CGRect, p: &CGPoint) -> bool {
    p.x >= r.origin.x && p.x < r.origin.x + r.size.width && p.y >= r.origin.y && p.y < r.origin.y + r.size.height
}

impl Mac {
    pub fn new() -> Self {
        let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).expect("CGEventSource");
        Mac { src, left_down: false, right_down: false, rem: (0.0, 0.0) }
    }

    fn cursor(&self) -> CGPoint {
        CGEvent::new(self.src.clone()).map(|e| e.location()).unwrap_or(CGPoint::new(0.0, 0.0))
    }

    /// Jaga kursor tetap di dalam layar, termasuk kalau ada beberapa monitor.
    fn clamp(old: CGPoint, new: CGPoint) -> CGPoint {
        let rects: Vec<CGRect> = CGDisplay::active_displays()
            .unwrap_or_default()
            .into_iter()
            .map(|id| CGDisplay::new(id).bounds())
            .collect();
        if rects.is_empty() || rects.iter().any(|r| inside(r, &new)) {
            return new;
        }
        let r = rects.iter().find(|r| inside(r, &old)).unwrap_or(&rects[0]);
        CGPoint::new(
            new.x.clamp(r.origin.x, r.origin.x + r.size.width - 1.0),
            new.y.clamp(r.origin.y, r.origin.y + r.size.height - 1.0),
        )
    }

    fn mouse_move(&mut self, dx: f64, dy: f64) {
        let old = self.cursor();
        let p = Self::clamp(old, CGPoint::new(old.x + dx, old.y + dy));
        let (ty, btn) = if self.left_down {
            (CGEventType::LeftMouseDragged, CGMouseButton::Left)
        } else if self.right_down {
            (CGEventType::RightMouseDragged, CGMouseButton::Right)
        } else {
            (CGEventType::MouseMoved, CGMouseButton::Left)
        };
        if let Ok(e) = CGEvent::new_mouse_event(self.src.clone(), ty, p, btn) {
            e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx.round() as i64);
            e.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy.round() as i64);
            e.post(CGEventTapLocation::HID);
        }
    }

    fn button(&mut self, b: Button, down: bool, clicks: i64) {
        let (ty, btn) = match (b, down) {
            (Button::Left, true) => (CGEventType::LeftMouseDown, CGMouseButton::Left),
            (Button::Left, false) => (CGEventType::LeftMouseUp, CGMouseButton::Left),
            (Button::Right, true) => (CGEventType::RightMouseDown, CGMouseButton::Right),
            (Button::Right, false) => (CGEventType::RightMouseUp, CGMouseButton::Right),
        };
        match b {
            Button::Left => self.left_down = down,
            Button::Right => self.right_down = down,
        }
        if let Ok(e) = CGEvent::new_mouse_event(self.src.clone(), ty, self.cursor(), btn) {
            e.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, clicks);
            e.post(CGEventTapLocation::HID);
        }
    }

    fn scroll(&mut self, dx: f64, dy: f64, phase: Phase, momentum: Momentum) {
        let tx = dx + self.rem.0;
        let ty = dy + self.rem.1;
        let (ix, iy) = (tx.trunc(), ty.trunc());
        self.rem = (tx - ix, ty - iy);
        let boundary = !matches!(phase, Phase::Changed | Phase::None) || !matches!(momentum, Momentum::Continue | Momentum::None);
        if ix == 0.0 && iy == 0.0 && !boundary {
            return;
        }
        if matches!(phase, Phase::Ended) || matches!(momentum, Momentum::End) {
            self.rem = (0.0, 0.0);
        }
        let ph = match phase {
            Phase::None => 0,
            Phase::Began => 1,
            Phase::Changed => 2,
            Phase::Ended => 4,
        };
        let mo = match momentum {
            Momentum::None => 0,
            Momentum::Begin => 1,
            Momentum::Continue => 2,
            Momentum::End => 3,
        };
        if let Ok(e) = CGEvent::new_scroll_event(self.src.clone(), ScrollEventUnit::PIXEL, 2, iy as i32, ix as i32, 0) {
            e.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS, 1);
            e.set_integer_value_field(SCROLL_PHASE, ph);
            e.set_integer_value_field(MOMENTUM_PHASE, mo);
            e.post(CGEventTapLocation::HID);
        }
    }

    fn key(&self, code: u16, down: bool, flags: CGEventFlags) {
        if let Ok(e) = CGEvent::new_keyboard_event(self.src.clone(), code, down) {
            e.set_flags(flags);
            e.post(CGEventTapLocation::HID);
        }
    }

    fn shortcut(&self, s: Shortcut) {
        let arrow = match s {
            Shortcut::MissionControl => KEY_UP,
            Shortcut::AppExpose => KEY_DOWN,
            Shortcut::SpaceLeft => KEY_LEFT,
            Shortcut::SpaceRight => KEY_RIGHT,
        };
        let ctrl = CGEventFlags::CGEventFlagControl;
        self.key(KEY_CONTROL, true, ctrl);
        self.key(arrow, true, ctrl);
        self.key(arrow, false, ctrl);
        self.key(KEY_CONTROL, false, CGEventFlags::CGEventFlagNull);
    }

    pub fn apply(&mut self, a: Action) {
        match a {
            Action::Move { dx, dy } => self.mouse_move(dx, dy),
            Action::Button { button, down, clicks } => self.button(button, down, clicks),
            Action::Scroll { dx, dy, phase, momentum } => self.scroll(dx, dy, phase, momentum),
            Action::Shortcut(s) => self.shortcut(s),
            Action::Haptic => {}
        }
    }

    /// Lepas tombol yang masih tertekan (misalnya saat HP terputus).
    pub fn release_all(&mut self) {
        if self.left_down {
            self.button(Button::Left, false, 1);
        }
        if self.right_down {
            self.button(Button::Right, false, 1);
        }
    }
}
