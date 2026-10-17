//! The desktop clipboard: Windows, macOS, Wayland or X11 (through arboard),
//! falling back to KDE's Klipper over D-Bus on Linux. Copies carry both plain text and HTML, so
//! formatting survives pasting into browsers, mail and office programs.

use std::process::Command;

pub struct SysClip {
    cb: Option<arboard::Clipboard>,
    tried: bool,
    /// The text we last put on the clipboard, to tell our own copies from
    /// something copied in another program.
    last_text: Option<String>,
}

fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace("\r\n", "\n").trim().to_string();
    norm(a) == norm(b)
}

fn klipper(args: &[&str]) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let out = Command::new("qdbus6")
        .args(["org.kde.klipper", "/klipper"])
        .args(args)
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

impl Default for SysClip {
    fn default() -> Self {
        SysClip::new()
    }
}

impl SysClip {
    pub fn new() -> SysClip {
        SysClip {
            cb: None,
            tried: false,
            last_text: None,
        }
    }

    fn cb(&mut self) -> Option<&mut arboard::Clipboard> {
        if !self.tried {
            self.tried = true;
            self.cb = arboard::Clipboard::new().ok();
        }
        self.cb.as_mut()
    }

    /// Put text (and its HTML version) on the clipboard.
    pub fn set(&mut self, text: &str, html: &str) -> bool {
        let ok = match self.cb() {
            Some(cb) => cb.set_html(html, Some(text)).is_ok() || cb.set_text(text).is_ok(),
            None => false,
        } || klipper(&["setClipboardContents", text]).is_some();
        if ok {
            self.last_text = Some(text.to_string());
        }
        ok
    }

    /// The clipboard's text, and its HTML version if it has one.
    pub fn get(&mut self) -> Option<(String, Option<String>)> {
        if let Some(cb) = self.cb() {
            let html = cb.get().html().ok().filter(|h| !h.trim().is_empty());
            if let Ok(text) = cb.get().text() {
                return Some((text, html));
            }
            if let Some(h) = html {
                return Some((String::new(), Some(h)));
            }
        }
        klipper(&["getClipboardContents"]).map(|t| (t, None))
    }

    /// Whether `text` is what we last copied (so our richer internal copy
    /// can be used instead).
    pub fn is_ours(&self, text: &str) -> bool {
        self.last_text.as_deref().is_some_and(|t| same(t, text))
    }
}
