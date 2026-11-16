//! Protokol UDP Usap v1 (little-endian). Port bawaan 50505.
//!
//! HP -> agen:
//!   0x01 HELLO    [kode: 6 byte ASCII][panjang nama: u8][nama HP]
//!   0x02 TOUCH    [seq: u32][waktu ms: u32][n: u8] lalu n x [id: u8][x mm: f32][y mm: f32]
//!   0x03 PING
//!   0x04 DISCOVER (broadcast, mencari agen di jaringan)
//!   0x05 BYE
//!   0x06 NOTE     [jenis: u8] 1 = Android membatalkan sentuhan (gestur sistem)
//!   0x07 BATCH    lewat Bluetooth: beberapa paket sekaligus, masing-masing [panjang: u8][paket]
//! Lewat kabel USB (TCP), setiap paket di atas diawali panjangnya sebagai u16.
//! agen -> HP:
//!   0x81 WELCOME  [ok: u8][nama komputer]
//!   0x82 HAPTIC   [jenis: u8]
//!   0x83 PONG
//!   0x84 HERE     [nama komputer]

use crate::engine::Touch;

pub const HELLO: u8 = 0x01;
pub const TOUCH: u8 = 0x02;
pub const PING: u8 = 0x03;
pub const DISCOVER: u8 = 0x04;
pub const BYE: u8 = 0x05;
pub const NOTE: u8 = 0x06;
pub const NOTE_CANCEL: u8 = 1;
pub const BATCH: u8 = 0x07;
pub const WELCOME: u8 = 0x81;
pub const HAPTIC: u8 = 0x82;
pub const PONG: u8 = 0x83;
pub const HERE: u8 = 0x84;

pub struct TouchPacket {
    pub seq: u32,
    pub t: u32,
    pub touches: Vec<Touch>,
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn f32_at(b: &[u8], i: usize) -> Option<f32> {
    u32_at(b, i).map(f32::from_bits)
}

pub fn parse_touch(b: &[u8]) -> Option<TouchPacket> {
    let seq = u32_at(b, 1)?;
    let t = u32_at(b, 5)?;
    let n = *b.get(9)? as usize;
    if n > 10 {
        return None;
    }
    let mut touches = Vec::with_capacity(n);
    for k in 0..n {
        let o = 10 + k * 9;
        let id = *b.get(o)?;
        let x = f32_at(b, o + 1)?;
        let y = f32_at(b, o + 5)?;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        touches.push(Touch { id, x, y });
    }
    Some(TouchPacket { seq, t, touches })
}

pub fn parse_hello(b: &[u8]) -> Option<(String, String)> {
    let code = std::str::from_utf8(b.get(1..7)?).ok()?.to_string();
    let len = *b.get(7)? as usize;
    let name = b.get(8..8 + len).map(|s| String::from_utf8_lossy(s).into_owned()).unwrap_or_default();
    Some((code, name))
}

/// Pecah BATCH menjadi paket-paket. Paket biasa dikembalikan apa adanya.
pub fn unbatch(b: &[u8]) -> Vec<Vec<u8>> {
    if b.first() != Some(&BATCH) {
        return if b.is_empty() { vec![] } else { vec![b.to_vec()] };
    }
    let mut out = Vec::new();
    let mut i = 1;
    while i < b.len() {
        let n = b[i] as usize;
        let Some(p) = b.get(i + 1..i + 1 + n) else { break };
        if n > 0 {
            out.push(p.to_vec());
        }
        i += 1 + n;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbatches() {
        assert_eq!(unbatch(&[PING]), vec![vec![PING]]);
        assert_eq!(unbatch(&[BATCH, 1, PING, 2, NOTE, 1]), vec![vec![PING], vec![NOTE, 1]]);
        assert_eq!(unbatch(&[BATCH, 5, PING]), Vec::<Vec<u8>>::new());
    }

    #[test]
    fn parses_touch_packet() {
        let mut b = vec![TOUCH];
        b.extend(7u32.to_le_bytes());
        b.extend(1234u32.to_le_bytes());
        b.push(1);
        b.push(3);
        b.extend(12.5f32.to_le_bytes());
        b.extend(40.0f32.to_le_bytes());
        let p = parse_touch(&b).unwrap();
        assert_eq!((p.seq, p.t, p.touches.len(), p.touches[0].id), (7, 1234, 1, 3));
        assert_eq!(p.touches[0].x, 12.5);
        assert!(parse_touch(&b[..b.len() - 1]).is_none());
    }

    #[test]
    fn parses_hello() {
        let mut b = vec![HELLO];
        b.extend(b"123456");
        b.push(5);
        b.extend(b"Tecno");
        assert_eq!(parse_hello(&b), Some(("123456".into(), "Tecno".into())));
    }
}
