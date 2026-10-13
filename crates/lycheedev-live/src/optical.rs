//! The color signal is freshness/readiness evidence; it never supplies runtime identity.
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug)]
pub struct Signal {
    pub state: &'static str,
    pub heartbeat: bool,
    pub frame_ticks: i64,
    pub edge_ticks: i64,
}
#[derive(Default)]
pub struct Tracker {
    current: Option<Signal>,
    highwater: i64,
    after: i64,
}
pub fn decode(bytes: &[u8]) -> Result<(&'static str, bool)> {
    if bytes.len() != 48 {
        return Err("live.input_signal_extent".into());
    }
    let mut cells = [b'?'; 3];
    for (cell, out) in cells.iter_mut().enumerate() {
        for y in 0..2 {
            for x in cell * 2..cell * 2 + 2 {
                let p = &bytes[(y * 6 + x) * 4..(y * 6 + x + 1) * 4];
                let color = match (p[2], p[1], p[0], p[3]) {
                    (_, _, _, 0..=249) => b'?',
                    (0..=31, 0..=31, 0..=31, _) => b'k',
                    (224..=255, 224..=255, 224..=255, _) => b'w',
                    (224..=255, 0..=31, 0..=31, _) => b'r',
                    (0..=31, 224..=255, 0..=31, _) => b'g',
                    (0..=31, 0..=31, 224..=255, _) => b'b',
                    _ => b'?',
                };
                if color == b'?' || (y != 0 || x != cell * 2) && color != *out {
                    return Err("live.input_signal_mixed_or_invalid".into());
                }
                *out = color;
            }
        }
    }
    let state = match &cells[..2] {
        b"gw" => "ready",
        b"rb" => "input_keyboard_focus",
        b"br" => "input_combat_lockdown",
        b"wk" => "unknown",
        _ => return Err("live.input_signal_code".into()),
    };
    if !b"kw".contains(&cells[2]) {
        return Err("live.input_signal_heartbeat".into());
    }
    Ok((state, cells[2] == b'w'))
}
impl Tracker {
    pub fn accept(&mut self, bytes: &[u8], ticks: i64, now: i64) -> Result<()> {
        if ticks <= 0 || ticks > now || now - ticks > 5_000_000 || ticks <= self.highwater {
            self.current = None;
            return Err("live.input_signal_stale".into());
        }
        self.highwater = ticks;
        let (state, heartbeat) = match decode(bytes) {
            Ok(v) => v,
            Err(e) => {
                self.current = None;
                return Err(e);
            }
        };
        let edge = match &self.current {
            Some(old) if ticks - old.frame_ticks <= 15_000_000 => {
                if heartbeat != old.heartbeat {
                    ticks
                } else {
                    old.edge_ticks
                }
            }
            _ => 0,
        };
        self.current = Some(Signal {
            state,
            heartbeat,
            frame_ticks: ticks,
            edge_ticks: edge,
        });
        Ok(())
    }
    pub fn evidence(&mut self, now: i64) -> Result<Signal> {
        let s = self
            .current
            .as_ref()
            .ok_or("live.input_signal_unavailable")?;
        if now < s.frame_ticks
            || now - s.frame_ticks > 5_000_000
            || s.edge_ticks == 0
            || now < s.edge_ticks
            || now - s.edge_ticks > 15_000_000
            || s.edge_ticks < self.after
        {
            return Err("live.input_signal_waiting".into());
        }
        if !["ready", "input_keyboard_focus"].contains(&s.state) {
            return Err("live.input_signal_blocked".into());
        }
        Ok(s.clone())
    }
    pub fn after_input(&mut self, ticks: i64) {
        self.after = ticks + 1_000_000;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pixels(beat: bool) -> Vec<u8> {
        let mut b = vec![0; 48];
        for y in 0..2 {
            for x in 0..6 {
                let v = if x < 2 {
                    [0, 255, 0, 255]
                } else if x < 4 || beat {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                };
                b[(y * 6 + x) * 4..(y * 6 + x + 1) * 4].copy_from_slice(&v)
            }
        }
        b
    }
    #[test]
    fn heartbeat_not_compositor_freshness() {
        let mut t = Tracker::default();
        t.accept(&pixels(false), 100, 100).unwrap();
        assert!(t.evidence(100).is_err());
        t.accept(&pixels(true), 200, 200).unwrap();
        assert_eq!(t.evidence(200).unwrap().state, "ready");
        t.accept(&pixels(true), 16_000_201, 16_000_201).unwrap();
        assert!(t.evidence(16_000_201).is_err());
    }
    #[test]
    fn mixed_pixel_rejected() {
        let mut b = pixels(true);
        b[0] = 120;
        assert!(decode(&b).is_err());
    }
}
