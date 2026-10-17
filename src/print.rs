//! Page composition for print preview and PDF: body lines from the layout,
//! headers and footers with page numbers, footnotes at the page bottom.

use crate::layout::{GKind, Glyph, Layout, LineKind, PicBox, str_width};
use crate::model::*;

pub type HfParts = Vec<(String, Align)>;

/// Header/footer text is stored one line per line of text; a line may start
/// with `{center}` or `{right}` for its alignment. Templates may contain
/// `{page}`, `{pages}` and `{date}`.
pub fn hf_lines(s: &str) -> HfParts {
    let mut v: HfParts = s
        .split('\n')
        .map(|l| {
            if let Some(t) = l.strip_prefix("{center}") {
                (t.to_string(), Align::Center)
            } else if let Some(t) = l.strip_prefix("{right}") {
                (t.to_string(), Align::Right)
            } else {
                (l.to_string(), Align::Left)
            }
        })
        .collect();
    while v.last().is_some_and(|(t, _)| t.trim().is_empty()) {
        v.pop();
    }
    v
}

pub fn hf_join(lines: &[(String, Align)]) -> String {
    let mut v: Vec<String> = lines
        .iter()
        .map(|(t, a)| match a {
            Align::Center => format!("{{center}}{t}"),
            Align::Right => format!("{{right}}{t}"),
            _ => t.clone(),
        })
        .collect();
    while v.last().is_some_and(|l| hf_lines(l).is_empty()) {
        v.pop();
    }
    v.join("\n")
}

/// Header and footer as rows of (template, alignment) parts; the page
/// number setting adds a part to the first header row or last footer row.
pub fn header_footer_rows(doc: &Document) -> (Vec<HfParts>, Vec<HfParts>) {
    let mut h: Vec<HfParts> = hf_lines(&doc.header).into_iter().map(|p| vec![p]).collect();
    let mut f: Vec<HfParts> = hf_lines(&doc.footer).into_iter().map(|p| vec![p]).collect();
    let add = |rows: &mut Vec<HfParts>, first: bool, part: (String, Align)| {
        if rows.is_empty() {
            rows.push(Vec::new());
        }
        let i = if first { 0 } else { rows.len() - 1 };
        rows[i].push(part);
    };
    match doc.page.page_numbers {
        PageNumPos::None => {}
        PageNumPos::TopRight => add(&mut h, true, ("{page}".into(), Align::Right)),
        PageNumPos::BottomCenter => add(&mut f, false, ("{page}".into(), Align::Center)),
        PageNumPos::BottomRight => add(&mut f, false, ("{page}".into(), Align::Right)),
    }
    (h, f)
}

/// Header and footer content as a flat list of (template, alignment) lines.
pub fn header_footer_parts(doc: &Document) -> (HfParts, HfParts) {
    let (h, f) = header_footer_rows(doc);
    (
        h.into_iter().flatten().collect(),
        f.into_iter().flatten().collect(),
    )
}

/// Lines a header (or footer) takes on the page, with the blank line that
/// separates it from the text.
pub fn hf_height(rows: &[HfParts]) -> usize {
    if rows.is_empty() { 0 } else { rows.len() + 1 }
}

pub fn expand(template: &str, page: u32, pages: u32) -> String {
    let mut s = template
        .replace("{pages}", &pages.to_string())
        .replace("{page}", &page.to_string());
    if s.contains("{date}") {
        s = s.replace("{date}", &crate::app::today());
    }
    s
}

fn hf_glyphs(parts: &HfParts, page: u32, pages: u32, width: u16) -> Vec<Glyph> {
    let mut out = Vec::new();
    for (t, a) in parts {
        let s = expand(t, page, pages);
        let w = str_width(&s).min(width);
        let mut x = match a {
            Align::Center => (width - w) / 2,
            Align::Right => width - w,
            _ => 0,
        };
        for ch in s.chars() {
            if x >= width {
                break;
            }
            out.retain(|g: &Glyph| g.x != x);
            out.push(Glyph::plain(x, ch, GKind::Text));
            x += 1;
        }
    }
    out
}

pub struct PrintPage {
    /// One entry per line between the top and bottom margins.
    pub rows: Vec<Vec<Glyph>>,
    /// Pictures: the box's top row and the layout's picture.
    pub pics: Vec<(usize, PicBox)>,
}

pub fn compose(doc: &Document, layout: &Layout) -> Vec<PrintPage> {
    let total = doc.page.body_lines();
    let (h, f) = header_footer_rows(doc);
    let width = layout.width;
    let pages = layout.pages.max(1);
    let body_start = hf_height(&h).min(total);
    let body_end = total.saturating_sub(hf_height(&f));
    let mut out: Vec<PrintPage> = (1..=pages)
        .map(|pg| {
            let mut rows = vec![Vec::new(); total];
            for (i, parts) in h.iter().enumerate().take(total) {
                rows[i] = hf_glyphs(parts, pg, pages, width);
            }
            let fstart = total.saturating_sub(f.len());
            for (i, parts) in f.iter().enumerate() {
                if fstart + i < total {
                    rows[fstart + i] = hf_glyphs(parts, pg, pages, width);
                }
            }
            PrintPage {
                rows,
                pics: Vec::new(),
            }
        })
        .collect();
    let mut row = vec![body_start; pages as usize];
    let mut notes: Vec<Vec<Vec<Glyph>>> = vec![Vec::new(); pages as usize];
    let mut pics = layout.pics.iter().peekable();
    for (li, l) in layout.lines.iter().enumerate() {
        let pi = (l.page.max(1) - 1) as usize;
        if pi >= out.len() {
            continue;
        }
        match l.kind {
            LineKind::Text
            | LineKind::Blank
            | LineKind::Rule
            | LineKind::Endnote
            | LineKind::Picture => {
                if row[pi] < body_end {
                    out[pi].rows[row[pi]] = layout.glyphs(li, doc);
                    while let Some(pb) = pics.next_if(|p| p.line <= li) {
                        if pb.line == li {
                            let top = (row[pi] + 1)
                                .saturating_sub(pb.rows as usize)
                                .max(body_start);
                            out[pi].pics.push((top, *pb));
                        }
                    }
                }
                row[pi] += 1;
            }
            LineKind::Note | LineKind::NoteSep => notes[pi].push(layout.glyphs(li, doc)),
            LineKind::PageBreak { .. } | LineKind::SectionMark => {}
        }
    }
    for (pi, page_notes) in notes.into_iter().enumerate() {
        let start = body_end
            .saturating_sub(page_notes.len())
            .max(row[pi].min(body_end));
        for (k, g) in page_notes.into_iter().enumerate() {
            if start + k < body_end {
                out[pi].rows[start + k] = g;
            }
        }
    }
    out
}
