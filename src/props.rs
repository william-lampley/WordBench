//! Character "look" (font, size, colour, highlight, caps) derived from Word
//! run properties, and helpers to edit run properties. Run properties are
//! kept as the original `w:rPr` children so nothing is lost; the look is
//! what WordBench understands of them.

use crate::io::xml::{XNode, parse};
use crate::model::{Align, Fmt, ListInfo, ParaProps, TabKind, TabStop};
use crate::styles::Numbering;

/// Visible character attributes beyond bold/italic/underline/strikeout.
/// `None` means "not set here" (inherit).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharLook {
    /// Font name, or "+major"/"+minor" for theme fonts.
    pub font: Option<String>,
    /// Size in half-points.
    pub size: Option<u16>,
    /// Text colour (RGB); `Some(AUTO)` means automatic.
    pub color: Option<u32>,
    /// Background (highlight or shading), RGB; `Some(NONE)` means none.
    pub bg: Option<u32>,
    pub caps: Option<bool>,
    pub small_caps: Option<bool>,
    pub hidden: Option<bool>,
    /// Character style id (`w:rStyle`).
    pub char_style: Option<String>,
}

pub const AUTO: u32 = 0xFF00_0000;
pub const NONE: u32 = 0xFE00_0000;

impl CharLook {
    /// This look with unset attributes taken from `base`.
    pub fn over(&self, base: &CharLook) -> CharLook {
        CharLook {
            font: self.font.clone().or_else(|| base.font.clone()),
            size: self.size.or(base.size),
            color: self.color.or(base.color),
            bg: self.bg.or(base.bg),
            caps: self.caps.or(base.caps),
            small_caps: self.small_caps.or(base.small_caps),
            hidden: self.hidden.or(base.hidden),
            char_style: self.char_style.clone().or_else(|| base.char_style.clone()),
        }
    }

    pub fn rgb(&self) -> Option<u32> {
        self.color.filter(|&c| c != AUTO)
    }

    pub fn background(&self) -> Option<u32> {
        self.bg.filter(|&c| c != NONE)
    }
}

/// Word highlight colour names.
pub const HIGHLIGHTS: &[(&str, &str, u32)] = &[
    ("yellow", "Yellow", 0xFFFF00),
    ("green", "Bright green", 0x00FF00),
    ("cyan", "Turquoise", 0x00FFFF),
    ("magenta", "Pink", 0xFF00FF),
    ("blue", "Blue", 0x0000FF),
    ("red", "Red", 0xFF0000),
    ("darkBlue", "Dark blue", 0x000080),
    ("darkCyan", "Teal", 0x008080),
    ("darkGreen", "Green", 0x008000),
    ("darkMagenta", "Violet", 0x800080),
    ("darkRed", "Dark red", 0x800000),
    ("darkYellow", "Dark yellow", 0x808000),
    ("darkGray", "Gray 50%", 0x808080),
    ("lightGray", "Gray 25%", 0xC0C0C0),
    ("black", "Black", 0x000000),
    ("white", "White", 0xFFFFFF),
];

/// Font colours offered in the Font dialog (Word's standard colours).
pub const COLORS: &[(&str, u32)] = &[
    ("Black", 0x000000),
    ("Dark red", 0xC00000),
    ("Red", 0xFF0000),
    ("Orange", 0xFFC000),
    ("Yellow", 0xFFFF00),
    ("Light green", 0x92D050),
    ("Green", 0x00B050),
    ("Light blue", 0x00B0F0),
    ("Blue", 0x0070C0),
    ("Dark blue", 0x002060),
    ("Purple", 0x7030A0),
    ("Gray", 0x808080),
];

pub fn color_name(rgb: u32) -> String {
    if rgb == AUTO {
        return "Auto".into();
    }
    COLORS
        .iter()
        .find(|(_, c)| *c == rgb)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("#{rgb:06X}"))
}

pub fn highlight_name(rgb: u32) -> String {
    HIGHLIGHTS
        .iter()
        .find(|(_, _, c)| *c == rgb)
        .map(|(_, n, _)| n.to_string())
        .unwrap_or_else(|| format!("#{rgb:06X}"))
}

pub fn parse_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('#');
    (s.len() == 6)
        .then(|| u32::from_str_radix(s, 16).ok())
        .flatten()
}

fn node(xml: &str) -> Option<XNode> {
    parse(xml)
        .ok()?
        .children
        .into_iter()
        .find(|c| c.name != "#text")
}

fn on(n: &XNode) -> bool {
    !matches!(n.attr("val"), Some("0" | "false" | "off" | "none"))
}

pub fn look_from_rpr(items: &[(String, String)]) -> CharLook {
    let mut l = CharLook::default();
    for (name, xml) in items {
        let interesting = matches!(
            name.as_str(),
            "rFonts"
                | "sz"
                | "color"
                | "highlight"
                | "shd"
                | "caps"
                | "smallCaps"
                | "vanish"
                | "rStyle"
        );
        if !interesting {
            continue;
        }
        let Some(n) = node(xml) else { continue };
        match name.as_str() {
            "rFonts" => {
                l.font = n
                    .attr("ascii")
                    .or_else(|| n.attr("hAnsi"))
                    .or_else(|| n.attr("cs"))
                    .or_else(|| n.attr("eastAsia"))
                    .map(str::to_string)
                    .or_else(|| {
                        n.attr("asciiTheme")
                            .or_else(|| n.attr("hAnsiTheme"))
                            .map(|t| {
                                if t.starts_with("major") {
                                    "+major".into()
                                } else {
                                    "+minor".into()
                                }
                            })
                    })
            }
            "sz" => l.size = n.attr("val").and_then(|v| v.parse().ok()),
            "color" => {
                l.color = match n.attr("val") {
                    Some("auto") => Some(AUTO),
                    Some(v) => parse_hex(v),
                    None => None,
                }
            }
            "highlight" => {
                l.bg = match n.attr("val") {
                    Some("none") => Some(NONE),
                    Some(v) => HIGHLIGHTS
                        .iter()
                        .find(|(k, _, _)| *k == v)
                        .map(|(_, _, c)| *c),
                    None => None,
                }
            }
            "shd" => {
                if l.bg.is_none() {
                    l.bg = n.attr("fill").filter(|f| *f != "auto").and_then(parse_hex);
                }
            }
            "caps" => l.caps = Some(on(&n)),
            "smallCaps" => l.small_caps = Some(on(&n)),
            "vanish" => l.hidden = Some(on(&n)),
            "rStyle" => l.char_style = n.attr("val").map(str::to_string),
            _ => {}
        }
    }
    l
}

const TWIPS_PER_COL: f32 = 144.0;

fn num(n: &XNode, key: &str) -> Option<f32> {
    n.attr(key).and_then(|v| v.trim().parse::<f32>().ok())
}

/// Blank lines for a paragraph spacing in twips (a line is 12pt).
pub fn space_lines(tw: f32) -> u8 {
    if tw < 100.0 {
        0
    } else {
        ((tw + 120.0) / 240.0).floor().clamp(1.0, 3.0) as u8
    }
}

/// Character formatting flags from run properties (including explicit
/// "off" values such as `<w:b w:val="0"/>`).
pub fn fmt_from_rpr(items: &[(String, String)]) -> Fmt {
    let mut f = Fmt::default();
    for (name, xml) in items {
        if !matches!(
            name.as_str(),
            "b" | "i" | "u" | "strike" | "dstrike" | "vertAlign"
        ) {
            continue;
        }
        let Some(c) = node(xml) else { continue };
        match name.as_str() {
            "b" => f = f.with(Fmt::BOLD, on(&c)),
            "i" => f = f.with(Fmt::ITALIC, on(&c)),
            "u" => f = f.with(Fmt::UNDERLINE, on(&c)),
            "strike" | "dstrike" => {
                if on(&c) {
                    f = f.with(Fmt::STRIKE, true);
                } else if !f.has(Fmt::STRIKE) {
                    f = f.with(Fmt::STRIKE, false);
                }
            }
            "vertAlign" => match c.attr("val") {
                Some("superscript") => f = f.with(Fmt::SUPER, true),
                Some("subscript") => f = f.with(Fmt::SUB, true),
                _ => {}
            },
            _ => {}
        }
    }
    f
}

pub fn outline_level(items: &[(String, String)]) -> Option<u8> {
    let (_, xml) = items.iter().find(|(n, _)| n == "outlineLvl")?;
    let v: u8 = node(xml)?.attr("val")?.parse().ok()?;
    (v < 9).then_some(v)
}

/// Apply `w:pPr` children to paragraph properties (later values win; tab
/// stops accumulate as in Word).
pub fn apply_ppr(p: &mut ParaProps, items: &[(String, String)], numbering: &Numbering) {
    for (name, xml) in items {
        let interesting = matches!(
            name.as_str(),
            "jc" | "ind" | "spacing" | "contextualSpacing" | "pageBreakBefore" | "numPr" | "tabs"
        );
        if !interesting {
            continue;
        }
        let Some(c) = node(xml) else { continue };
        match name.as_str() {
            "jc" => {
                p.align = match c.attr("val").unwrap_or("") {
                    "center" => Align::Center,
                    "right" | "end" => Align::Right,
                    "both" | "distribute" | "justify" | "lowKashida" | "mediumKashida"
                    | "highKashida" => Align::Full,
                    _ => Align::Left,
                }
            }
            "ind" => {
                if let Some(l) = num(&c, "left").or_else(|| num(&c, "start")) {
                    p.indent = (l / TWIPS_PER_COL).round().clamp(0.0, 60.0) as u16;
                }
                if let Some(f) = num(&c, "firstLine") {
                    p.first_indent = (f / TWIPS_PER_COL).round().clamp(-60.0, 60.0) as i16;
                } else if let Some(h) = num(&c, "hanging") {
                    p.first_indent = -(h / TWIPS_PER_COL).round().clamp(-60.0, 60.0) as i16;
                }
            }
            "spacing" => {
                if let Some(line) = num(&c, "line") {
                    let auto = c.attr("lineRule").unwrap_or("auto") == "auto";
                    p.spacing = if auto && line >= 420.0 { 2 } else { 1 };
                }
                if let Some(b) = num(&c, "before") {
                    p.space_before = space_lines(b);
                }
                if c.attr("beforeAutospacing")
                    .is_some_and(|v| v == "1" || v == "true")
                {
                    p.space_before = 1;
                }
                if let Some(a) = num(&c, "after") {
                    p.space_after = space_lines(a);
                }
                if c.attr("afterAutospacing")
                    .is_some_and(|v| v == "1" || v == "true")
                {
                    p.space_after = 1;
                }
            }
            "contextualSpacing" => p.contextual = on(&c),
            "pageBreakBefore" => p.page_break_before = on(&c),
            "numPr" => {
                let ilvl: Option<u8> = c
                    .child("ilvl")
                    .and_then(|n| n.attr("val"))
                    .and_then(|v| v.parse().ok());
                let id: Option<u32> = c
                    .child("numId")
                    .and_then(|n| n.attr("val"))
                    .and_then(|v| v.parse().ok());
                match id {
                    Some(0) => p.list = None,
                    Some(id) => {
                        let level = ilvl.or(p.list.map(|l| l.level)).unwrap_or(0).min(8);
                        p.list = Some(ListInfo {
                            kind: numbering.kind(id, level),
                            level,
                            num: id,
                        });
                    }
                    None => {
                        if let (Some(l), Some(li)) = (ilvl, p.list.as_mut()) {
                            li.level = l.min(8);
                            li.kind = numbering.kind(li.num, li.level);
                        }
                    }
                }
            }
            "tabs" => {
                for t in c.children_named("tab") {
                    let pos = (num(t, "pos").unwrap_or(0.0) / TWIPS_PER_COL)
                        .round()
                        .clamp(0.0, 400.0) as u16;
                    p.tabs.retain(|s| s.pos != pos);
                    let kind = match t.attr("val").unwrap_or("left") {
                        "clear" => continue,
                        "right" | "end" => TabKind::Right,
                        "center" => TabKind::Center,
                        "decimal" => TabKind::Decimal,
                        _ => TabKind::Left,
                    };
                    let leader = match t.attr("leader").unwrap_or("none") {
                        "dot" | "middleDot" => '.',
                        "hyphen" => '-',
                        "underscore" | "heavy" => '_',
                        _ => ' ',
                    };
                    p.tabs.push(TabStop { pos, kind, leader });
                }
                p.tabs.sort_by_key(|t| t.pos);
            }
            _ => {}
        }
    }
}

/// Replace (or with `None` remove) a property element by local name.
pub fn set_item(items: &mut Vec<(String, String)>, name: &str, xml: Option<String>) {
    match (items.iter().position(|(n, _)| n == name), xml) {
        (Some(i), Some(x)) => items[i].1 = x,
        (Some(i), None) => {
            items.remove(i);
        }
        (None, Some(x)) => items.push((name.into(), x)),
        (None, None) => {}
    }
}

fn esc_attr(s: &str) -> String {
    crate::io::xml::esc(s)
}

/// A change made in the Font dialog; `None` fields are left alone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharEdit {
    pub font: Option<Option<String>>,
    pub size: Option<Option<u16>>,
    pub color: Option<Option<u32>>,
    /// Word highlight name, or None to remove.
    pub highlight: Option<Option<&'static str>>,
    pub caps: Option<bool>,
    pub small_caps: Option<bool>,
}

impl CharEdit {
    pub fn is_empty(&self) -> bool {
        *self == CharEdit::default()
    }

    pub fn apply(&self, items: &mut Vec<(String, String)>) {
        if let Some(f) = &self.font {
            let x = f.as_ref().map(|f| {
                let f = esc_attr(f);
                format!(r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:cs="{f}" w:eastAsia="{f}"/>"#)
            });
            set_item(items, "rFonts", x);
        }
        if let Some(s) = self.size {
            set_item(items, "sz", s.map(|h| format!(r#"<w:sz w:val="{h}"/>"#)));
            set_item(
                items,
                "szCs",
                s.map(|h| format!(r#"<w:szCs w:val="{h}"/>"#)),
            );
        }
        if let Some(c) = self.color {
            set_item(
                items,
                "color",
                c.map(|c| format!(r#"<w:color w:val="{c:06X}"/>"#)),
            );
        }
        if let Some(h) = self.highlight {
            set_item(
                items,
                "highlight",
                h.map(|h| format!(r#"<w:highlight w:val="{h}"/>"#)),
            );
        }
        if let Some(c) = self.caps {
            set_item(items, "caps", c.then(|| "<w:caps/>".into()));
        }
        if let Some(c) = self.small_caps {
            set_item(items, "smallCaps", c.then(|| "<w:smallCaps/>".into()));
        }
    }
}

/// Map symbol-font bullet characters to Unicode.
pub fn bullet_char(text: &str) -> String {
    let c = text.chars().next().unwrap_or('•');
    let mapped = match c as u32 {
        0xF0B7 | 0xB7 => '•',
        0xF0A7 | 0xA7 => '▪',
        0xF0D8 => '➢',
        0xF076 => '❖',
        0xF0FC => '✓',
        0xF0A8 => '□',
        0xF06E => '■',
        0xF0E0 => '➔',
        0xF000..=0xF0FF => '•',
        _ if c == 'o' => '◦',
        _ => c,
    };
    mapped.to_string()
}

/// Format a list number in a Word number format.
pub fn format_num(fmt: &str, n: u32) -> String {
    match fmt {
        "lowerLetter" => crate::layout::alpha(n),
        "upperLetter" => crate::layout::alpha(n).to_uppercase(),
        "lowerRoman" => crate::layout::roman(n),
        "upperRoman" => crate::layout::roman(n).to_uppercase(),
        "decimalZero" => format!("{n:02}"),
        "ordinal" => {
            let suffix = match (n % 10, n % 100) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{n}{suffix}")
        }
        "none" => String::new(),
        _ => n.to_string(),
    }
}

/// Readable version of a colour on a background (both RGB): lightens or
/// darkens it until the contrast is good enough for a terminal.
/// WCAG contrast ratio of two RGB colours (1 to 21).
#[cfg_attr(not(test), allow(dead_code))]
pub fn contrast(a: u32, b: u32) -> f32 {
    fn lum(c: u32) -> f32 {
        let ch = |v: u32| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c >> 16 & 255) + 0.7152 * ch(c >> 8 & 255) + 0.0722 * ch(c & 255)
    }
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

pub fn readable(fg: u32, bg: u32) -> u32 {
    fn lum(c: u32) -> f32 {
        let ch = |v: u32| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(c >> 16 & 255) + 0.7152 * ch(c >> 8 & 255) + 0.0722 * ch(c & 255)
    }
    let contrast = |a: u32, b: u32| {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    };
    let toward = if lum(bg) < 0.4 { 0xFFFFFF } else { 0x000000 };
    let mut c = fg;
    for step in 1..=10 {
        if contrast(c, bg) >= 3.0 {
            return c;
        }
        let t = step as f32 / 10.0;
        let mix = |s: u32| {
            let a = (fg >> s & 255) as f32;
            let b = (toward >> s & 255) as f32;
            ((a + (b - a) * t).round() as u32) << s
        };
        c = mix(16) | mix(8) | mix(0);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_look() {
        let items = vec![
            (
                "rFonts".to_string(),
                r#"<w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>"#.to_string(),
            ),
            ("sz".into(), r#"<w:sz w:val="28"/>"#.into()),
            ("color".into(), r#"<w:color w:val="C00000"/>"#.into()),
            (
                "highlight".into(),
                r#"<w:highlight w:val="yellow"/>"#.into(),
            ),
        ];
        let l = look_from_rpr(&items);
        assert_eq!(l.font.as_deref(), Some("Arial"));
        assert_eq!(l.size, Some(28));
        assert_eq!(l.rgb(), Some(0xC00000));
        assert_eq!(l.background(), Some(0xFFFF00));
    }

    #[test]
    fn edits_items() {
        let mut items = Vec::new();
        CharEdit {
            font: Some(Some("Georgia".into())),
            size: Some(Some(24)),
            ..Default::default()
        }
        .apply(&mut items);
        let l = look_from_rpr(&items);
        assert_eq!((l.font.as_deref(), l.size), (Some("Georgia"), Some(24)));
        CharEdit {
            size: Some(None),
            ..Default::default()
        }
        .apply(&mut items);
        assert_eq!(look_from_rpr(&items).size, None);
    }

    #[test]
    fn numbers_and_contrast() {
        assert_eq!(format_num("upperRoman", 4), "IV");
        assert_eq!(format_num("ordinal", 12), "12th");
        assert_eq!(bullet_char("\u{F0B7}"), "•");
        let c = readable(0x000080, 0x0000AA);
        assert_ne!(c, 0x000080);
    }
}

/// Fonts documents commonly use (Microsoft Office, macOS, web), offered
/// even when not installed: the system then shows a look-alike.
pub const COMMON_FONTS: &[&str] = &[
    "Aptos",
    "Arial",
    "Arial Black",
    "Arial Narrow",
    "Baskerville",
    "Book Antiqua",
    "Bookman Old Style",
    "Calibri",
    "Calibri Light",
    "Cambria",
    "Candara",
    "Century Gothic",
    "Century Schoolbook",
    "Comic Sans MS",
    "Consolas",
    "Constantia",
    "Corbel",
    "Courier New",
    "Didot",
    "Franklin Gothic",
    "Futura",
    "Garamond",
    "Georgia",
    "Gill Sans",
    "Helvetica",
    "Impact",
    "Lato",
    "Lucida Console",
    "Lucida Sans",
    "Montserrat",
    "Open Sans",
    "Palatino Linotype",
    "Roboto",
    "Rockwell",
    "Segoe UI",
    "Source Sans Pro",
    "Tahoma",
    "Times New Roman",
    "Trebuchet MS",
    "Verdana",
];

/// The installed font the system shows in place of each named font.
pub fn font_substitutes(names: &[&str]) -> std::collections::HashMap<String, String> {
    names
        .iter()
        .filter_map(|n| crate::platform::font_substitute(n).map(|s| (n.to_string(), s)))
        .collect()
}

/// Installed font families (from fontconfig), sorted and de-duplicated.
pub fn installed_fonts() -> Vec<String> {
    let mut v: Vec<String> = crate::platform::installed_fonts()
        .into_iter()
        .filter(|f| !f.is_empty() && !f.starts_with('.'))
        .collect();
    if v.is_empty() {
        v = COMMON_FONTS.iter().map(|s| s.to_string()).collect();
    }
    v.sort_by_key(|f| f.to_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v
}

fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb))
                .min(prev[j + 1] + 1)
                .min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// How well `name` matches what was typed, lower is better; None if it
/// doesn't match. Tolerates misspellings: "helvetika", "gergia", "tms new".
pub fn fuzzy_score(query: &str, name: &str) -> Option<usize> {
    let q: String = query
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some(0);
    }
    let n: String = name
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if n.starts_with(&q) {
        return Some(0);
    }
    if let Some(i) = n.find(&q) {
        return Some(1 + i.min(20));
    }
    // Letters in order ("tnr" -> Times New Roman).
    let mut it = n.chars();
    if q.chars().all(|c| it.any(|x| x == c)) {
        return Some(30);
    }
    // Close spelling of the start of the name or of any word in it.
    let qc: Vec<char> = q.chars().collect();
    let allowed = (qc.len() / 3).clamp(1, 3);
    let mut best = usize::MAX;
    let words = std::iter::once(n.clone())
        .chain(name.to_lowercase().split_whitespace().map(str::to_string))
        .collect::<Vec<_>>();
    for w in words {
        let wc: Vec<char> = w.chars().collect();
        for len in qc.len().saturating_sub(1)..=qc.len() + 1 {
            if len == 0 || len > wc.len() {
                continue;
            }
            best = best.min(edit_distance(&qc, &wc[..len]));
        }
    }
    (best <= allowed).then(|| 40 + best * 10)
}

/// Fonts matching a search, best first.
pub fn search_fonts<'a>(fonts: &'a [String], query: &str) -> Vec<&'a String> {
    let mut v: Vec<(usize, usize, &String)> = fonts
        .iter()
        .enumerate()
        .filter_map(|(i, f)| fuzzy_score(query, f).map(|s| (s, i, f)))
        .collect();
    v.sort_by_key(|(s, i, _)| (*s, *i));
    v.into_iter().map(|(_, _, f)| f).collect()
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn fuzzy_font_search() {
        let fonts: Vec<String> = [
            "Arial",
            "Courier New",
            "Georgia",
            "Helvetica",
            "Times New Roman",
            "DejaVu Sans",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(search_fonts(&fonts, "geo")[0], "Georgia");
        assert_eq!(search_fonts(&fonts, "gergia")[0], "Georgia");
        assert_eq!(search_fonts(&fonts, "helvetika")[0], "Helvetica");
        assert_eq!(search_fonts(&fonts, "tnr")[0], "Times New Roman");
        assert_eq!(search_fonts(&fonts, "roman")[0], "Times New Roman");
        assert_eq!(search_fonts(&fonts, "dejavu")[0], "DejaVu Sans");
        assert!(search_fonts(&fonts, "zzzz").is_empty());
    }
}
