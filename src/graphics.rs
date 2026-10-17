//! Pictures on screen with the kitty graphics protocol or sixel, for
//! terminals that support one. The text layout is unchanged: a picture
//! covers the cells its placeholder reserves, and is drawn after each frame.

use crate::model::{Document, InlineObj};
use crate::pictures::{self, Raster};
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Kitty,
    Sixel,
}

/// The protocol the terminal speaks, judged from the environment.
/// `WORDBENCH_GRAPHICS` (kitty, sixel or none) overrides the guess.
pub fn detect() -> Option<Proto> {
    detect_from(|k| std::env::var(k).ok())
}

fn detect_from(var: impl Fn(&str) -> Option<String>) -> Option<Proto> {
    match var("WORDBENCH_GRAPHICS")
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("kitty") => return Some(Proto::Kitty),
        Some("sixel") => return Some(Proto::Sixel),
        Some("none" | "off" | "0") => return None,
        _ => {}
    }
    // Multiplexers would need their own passthrough escapes.
    if var("TMUX").is_some() || var("STY").is_some() {
        return None;
    }
    let term = var("TERM").unwrap_or_default();
    let program = var("TERM_PROGRAM").unwrap_or_default();
    if var("KITTY_WINDOW_ID").is_some()
        || term == "xterm-kitty"
        || term == "xterm-ghostty"
        || matches!(program.as_str(), "ghostty" | "WezTerm")
    {
        return Some(Proto::Kitty);
    }
    if term.starts_with("foot")
        || term.starts_with("mlterm")
        || term.contains("sixel")
        || matches!(program.as_str(), "iTerm.app" | "contour")
    {
        return Some(Proto::Sixel);
    }
    None
}

/// A picture to show, in screen cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub key: String,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
}

pub struct Graphics {
    pub proto: Option<Proto>,
    /// Size of a character cell in pixels.
    pub cell_px: (u16, u16),
    /// Placements of the frame just drawn (not yet sent).
    pub pending: Vec<Placement>,
    shown: Vec<Placement>,
    rasters: HashMap<String, Option<Rc<Raster>>>,
    kitty_ids: HashMap<String, u32>,
    sixels: HashMap<(String, u16, u16), Rc<String>>,
}

impl Default for Graphics {
    fn default() -> Self {
        Graphics::new(None)
    }
}

/// Largest picture sent to the terminal, in pixels per side.
const MAX_SIDE: u32 = 1600;

impl Graphics {
    pub fn new(proto: Option<Proto>) -> Graphics {
        Graphics {
            proto,
            cell_px: (8, 16),
            pending: Vec::new(),
            shown: Vec::new(),
            rasters: HashMap::new(),
            kitty_ids: HashMap::new(),
            sixels: HashMap::new(),
        }
    }

    /// Learn the cell size from the terminal (when it reports pixels).
    pub fn query_cell_size(&mut self) {
        if let Ok(ws) = ratatui::crossterm::terminal::window_size()
            && ws.columns > 0
            && ws.rows > 0
            && ws.width >= ws.columns
            && ws.height >= ws.rows
        {
            self.cell_px = (ws.width / ws.columns, ws.height / ws.rows);
        }
    }

    /// Decoded picture of an inline object, cached. The key names the part
    /// and a fingerprint of its bytes, so a different document's picture of
    /// the same name is not mistaken for it.
    pub fn picture(&mut self, doc: &Document, o: &InlineObj) -> Option<String> {
        let (part, bytes) = pictures::source(doc, o)?;
        let key = format!("{part}#{}#{:016x}", bytes.len(), fingerprint(bytes));
        let r = self.rasters.entry(key.clone()).or_insert_with(|| {
            let r = pictures::decode(bytes)?;
            let s = (MAX_SIDE as f32 / r.w.max(r.h) as f32).min(1.0);
            Some(Rc::new(if s < 1.0 {
                r.scaled((r.w as f32 * s) as u32, (r.h as f32 * s) as u32)
            } else {
                r
            }))
        });
        r.is_some().then_some(key)
    }

    /// After a frame is drawn: sixel pictures that moved or went away need
    /// the screen redrawn from scratch first to wipe their pixels.
    pub fn needs_clear(&self) -> bool {
        self.proto == Some(Proto::Sixel) && !self.shown.is_empty() && self.pending != self.shown
    }

    /// Send the frame's pictures to the terminal.
    pub fn flush(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        let want = std::mem::take(&mut self.pending);
        let Some(proto) = self.proto else {
            return Ok(());
        };
        let mut buf: Vec<u8> = Vec::new();
        match proto {
            Proto::Kitty => {
                if want != self.shown {
                    if !self.shown.is_empty() {
                        buf.extend_from_slice(b"\x1b_Ga=d,d=a,q=2\x1b\\");
                    }
                    for p in &want {
                        let Some(r) = self.rasters.get(&p.key).cloned().flatten() else {
                            continue;
                        };
                        let next = self.kitty_ids.len() as u32 + 1;
                        let id = *self.kitty_ids.entry(p.key.clone()).or_insert_with(|| {
                            buf.extend_from_slice(kitty_transmit(next, &r).as_bytes());
                            next
                        });
                        let (cols, rows) = fit_cells(&r, p.cols, p.rows, self.cell_px);
                        let (dx, dy) = (0, p.rows - rows);
                        buf.extend_from_slice(
                            format!("\x1b7\x1b[{};{}H\x1b_Ga=p,i={id},c={cols},r={rows},C=1,q=2\x1b\\\x1b8", p.y + dy + 1, p.x + dx + 1)
                                .as_bytes(),
                        );
                    }
                }
            }
            Proto::Sixel => {
                // Redrawn every frame: the text drawn around a picture can
                // overwrite its pixels.
                for p in &want {
                    let Some(r) = self.rasters.get(&p.key).cloned().flatten() else {
                        continue;
                    };
                    let (cw, ch) = self.cell_px;
                    let six = self
                        .sixels
                        .entry((p.key.clone(), p.cols, p.rows))
                        .or_insert_with(|| {
                            let (w, h) =
                                fit_px(&r, p.cols as u32 * cw as u32, p.rows as u32 * ch as u32);
                            Rc::new(sixel(&r.scaled(w, h)))
                        })
                        .clone();
                    // Sit the picture on the bottom of its box, like the text.
                    let used =
                        r.h.max(1) as f32 / r.w.max(1) as f32 * (p.cols as u32 * cw as u32) as f32;
                    let rows = ((used / ch as f32).ceil() as u16).clamp(1, p.rows);
                    buf.extend_from_slice(
                        format!("\x1b7\x1b[{};{}H", p.y + (p.rows - rows) + 1, p.x + 1).as_bytes(),
                    );
                    buf.extend_from_slice(six.as_bytes());
                    buf.extend_from_slice(b"\x1b8");
                }
            }
        }
        self.shown = want;
        if !buf.is_empty() {
            out.write_all(&buf)?;
            out.flush()?;
        }
        Ok(())
    }

    /// Remove every picture (on exit).
    pub fn clear_all(&mut self, out: &mut impl Write) -> std::io::Result<()> {
        if self.proto == Some(Proto::Kitty) && !self.kitty_ids.is_empty() {
            out.write_all(b"\x1b_Ga=d,d=A,q=2\x1b\\")?;
            out.flush()?;
        }
        self.kitty_ids.clear();
        self.shown.clear();
        Ok(())
    }
}

fn fingerprint(b: &[u8]) -> u64 {
    // FNV-1a over the start and end of the data.
    let mut h: u64 = 0xcbf29ce484222325;
    let tail = b.len().saturating_sub(4096).max(4096.min(b.len()));
    for &x in b[..4096.min(b.len())].iter().chain(&b[tail..]) {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Size in pixels that fits `w` x `h`, keeping the picture's proportions.
fn fit_px(r: &Raster, w: u32, h: u32) -> (u32, u32) {
    let (rw, rh) = (r.w.max(1) as f32, r.h.max(1) as f32);
    let s = (w as f32 / rw).min(h as f32 / rh);
    (
        ((rw * s).round() as u32).max(1),
        ((rh * s).round() as u32).max(1),
    )
}

/// Cells that fit in `cols` x `rows`, keeping the picture's proportions.
fn fit_cells(r: &Raster, cols: u16, rows: u16, cell: (u16, u16)) -> (u16, u16) {
    let (w, h) = fit_px(r, cols as u32 * cell.0 as u32, rows as u32 * cell.1 as u32);
    let c = ((w as f32 / cell.0.max(1) as f32).round() as u16).clamp(1, cols);
    let rr = ((h as f32 / cell.1.max(1) as f32).round() as u16).clamp(1, rows);
    (c, rr)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

/// Kitty: send RGBA pixels (zlib-compressed, base64, in 4096-byte chunks)
/// under image id `id`, without showing them yet.
fn kitty_transmit(id: u32, r: &Raster) -> String {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    let _ = e.write_all(&r.rgba);
    let data = base64(&e.finish().unwrap_or_default());
    let chunks: Vec<&str> = data
        .as_bytes()
        .chunks(4096)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let mut s = String::new();
    for (k, c) in chunks.iter().enumerate() {
        let more = u8::from(k + 1 < chunks.len());
        if k == 0 {
            s.push_str(&format!(
                "\x1b_Ga=t,f=32,o=z,s={},v={},i={id},q=2,m={more};{c}\x1b\\",
                r.w, r.h
            ));
        } else {
            s.push_str(&format!("\x1b_Gm={more};{c}\x1b\\"));
        }
    }
    s
}

/// Sixel: the picture on a 6x6x6 colour cube (transparency blended with
/// white paper).
fn sixel(r: &Raster) -> String {
    let (w, h) = (r.w as usize, r.h as usize);
    let level = |v: u32| ((v * 5 + 127) / 255) as usize;
    let idx: Vec<u8> = r
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as u32;
            let c = |v: u8| (v as u32 * a + 255 * (255 - a)) / 255;
            (level(c(p[0])) * 36 + level(c(p[1])) * 6 + level(c(p[2]))) as u8
        })
        .collect();
    let mut used = [false; 216];
    for &i in &idx {
        used[i as usize] = true;
    }
    let mut s = format!("\x1bP0;1;0q\"1;1;{w};{h}");
    for (i, _) in used.iter().enumerate().filter(|(_, u)| **u) {
        let pct = |l: usize| l * 100 / 5;
        s.push_str(&format!(
            "#{i};2;{};{};{}",
            pct(i / 36),
            pct(i / 6 % 6),
            pct(i % 6)
        ));
    }
    let mut row = vec![0u8; w];
    for band in (0..h).step_by(6) {
        let mut colors: Vec<u8> = idx[band * w..(band + 6).min(h) * w].to_vec();
        colors.sort_unstable();
        colors.dedup();
        for (n, &c) in colors.iter().enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                *cell = (0..6)
                    .filter(|dy| band + dy < h && idx[(band + dy) * w + x] == c)
                    .map(|dy| 1 << dy)
                    .sum();
            }
            if n > 0 {
                s.push('$');
            }
            s.push_str(&format!("#{c}"));
            let mut x = 0;
            while x < w {
                let v = row[x];
                let run = row[x..].iter().take_while(|&&b| b == v).count();
                let ch = (63 + v) as char;
                if run > 3 {
                    s.push_str(&format!("!{run}{ch}"));
                } else {
                    s.extend(std::iter::repeat_n(ch, run));
                }
                x += run;
            }
        }
        s.push('-');
    }
    s.push_str("\x1b\\");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(w: u32, h: u32, px: [u8; 4]) -> Raster {
        Raster {
            w,
            h,
            rgba: px.repeat((w * h) as usize),
        }
    }

    #[test]
    fn detects_terminals() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            detect_from(env(&[("TERM", "xterm-kitty")])),
            Some(Proto::Kitty)
        );
        assert_eq!(detect_from(env(&[("TERM", "foot")])), Some(Proto::Sixel));
        assert_eq!(detect_from(env(&[("TERM", "xterm-256color")])), None);
        assert_eq!(
            detect_from(env(&[("TERM", "xterm-kitty"), ("TMUX", "1")])),
            None
        );
        assert_eq!(
            detect_from(env(&[("TERM", "xterm"), ("WORDBENCH_GRAPHICS", "sixel")])),
            Some(Proto::Sixel)
        );
        assert_eq!(
            detect_from(env(&[
                ("TERM", "xterm-kitty"),
                ("WORDBENCH_GRAPHICS", "none")
            ])),
            None
        );
    }

    #[test]
    fn encodes_sixel() {
        // 3x7 red: one full band (all six bits) and one with the top bit.
        let s = sixel(&raster(3, 7, [255, 0, 0, 255]));
        assert!(
            s.starts_with("\x1bP0;1;0q\"1;1;3;7#180;2;100;0;0#180"),
            "{s:?}"
        );
        assert!(s.contains("#180~~~-#180@@@-"), "{s:?}");
        assert!(s.ends_with("\x1b\\"));
        // Long runs are compressed; transparent pixels become white.
        let s = sixel(&raster(10, 1, [0, 0, 0, 0]));
        assert!(s.contains("#215!10@-"), "{s:?}");
    }

    #[test]
    fn kitty_chunks() {
        let s = kitty_transmit(3, &raster(64, 64, [1, 2, 3, 255]));
        assert!(s.starts_with("\x1b_Ga=t,f=32,o=z,s=64,v=64,i=3,q=2,m="));
        assert!(s.ends_with("\x1b\\"));
        let mut g = Graphics::new(Some(Proto::Kitty));
        g.rasters
            .insert("k".into(), Some(Rc::new(raster(20, 10, [0, 0, 0, 255]))));
        g.pending = vec![Placement {
            key: "k".into(),
            x: 4,
            y: 2,
            cols: 10,
            rows: 3,
        }];
        let mut out = Vec::new();
        g.flush(&mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("\x1b[3;5H\x1b_Ga=p,i=1,c=10,r=3,C=1,q=2"),
            "{text:?}"
        );
        // Unchanged placements are not sent again; removed ones are deleted.
        g.pending = g.shown.clone();
        let mut out = Vec::new();
        g.flush(&mut out).unwrap();
        assert!(out.is_empty());
        let mut out = Vec::new();
        g.flush(&mut out).unwrap();
        assert_eq!(out, b"\x1b_Ga=d,d=a,q=2\x1b\\");
    }

    #[test]
    fn base64_matches() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
