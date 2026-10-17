//! Layout engine: turns the document into visual lines on a fixed character
//! grid (10 cpi, 6 lpi), with word wrap, justification, list markers,
//! tables, pagination and footnotes. The same layout drives the screen,
//! Reveal Codes, print preview and PDF export, so what you see on screen is
//! what gets printed.

use crate::model::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const TAB: u16 = 5;

static FULL_SIZE_PICTURES: AtomicBool = AtomicBool::new(true);

/// Show pictures at their real size (reserving lines for their height) or
/// as one-line placeholders. A user setting: it applies to every layout, so
/// the screen, print preview and PDF always paginate alike.
pub fn set_full_size_pictures(on: bool) {
    FULL_SIZE_PICTURES.store(on, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutOpts {
    pub full_size_pictures: bool,
}

impl LayoutOpts {
    pub fn current() -> LayoutOpts {
        LayoutOpts {
            full_size_pictures: FULL_SIZE_PICTURES.load(Ordering::Relaxed),
        }
    }
}

/// Columns and lines of a full-size picture in an area `max_cols` wide
/// (scaled down, keeping its proportions, to fit).
pub fn picture_box(o: &InlineObj, max_cols: u16) -> Option<(u16, u16)> {
    let (c, r) = crate::pictures::grid_size(o)?;
    let max_cols = max_cols.max(1);
    if c <= max_cols {
        return Some((c, r));
    }
    let r = ((r as u32 * max_cols as u32 + c as u32 / 2) / c as u32).max(1) as u16;
    Some((max_cols, r))
}

/// A picture's place in the layout: on line `line` (its bottom line) from
/// column `x`, covering `cols` columns and `rows` lines up to and including
/// that line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PicBox {
    pub line: usize,
    pub x: u16,
    pub cols: u16,
    pub rows: u16,
    pub obj: char,
    pub src: Pos,
}

/// What width calculations need besides the text.
struct Ctx<'a> {
    fn_num: &'a HashMap<u32, String>,
    st: &'a Store,
    /// Full-size pictures (otherwise placeholders as wide as their label).
    pictures: bool,
}

/// A run of one paragraph's characters on one visual line.
#[derive(Clone, Debug)]
pub struct Seg {
    pub pr: ParaRef,
    pub start: usize,
    pub end: usize,
    /// Column of each character in `start..end`.
    pub xs: Vec<u16>,
    /// Display width of each character (tabs and justified spaces are wide).
    pub ws: Vec<u16>,
    /// Column just after the last character.
    pub end_x: u16,
    /// Last line of the paragraph.
    pub last: bool,
    /// Filler for a table cell that has fewer lines than its row.
    pub pad: bool,
    /// Formatting implied by the paragraph style, and the style's look.
    pub style: Fmt,
    pub style_cp: u32,
    /// Leader characters for tabs: (cell index, fill character).
    pub leaders: Vec<(usize, char)>,
    /// Horizontal extent of the area the paragraph lives in.
    pub lo: u16,
    pub hi: u16,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineKind {
    Text,
    Blank,
    Rule,
    NoteSep,
    Note,
    PageBreak {
        forced: bool,
        section: bool,
    },
    /// A continuous section break (shown, not printed).
    SectionMark,
    /// Endnotes, printed after the last paragraph.
    Endnote,
    /// Space for a picture taller than one line (above the line it is on).
    Picture,
}

impl LineKind {
    /// Whether the line takes up body space on the printed page.
    pub fn is_body(self) -> bool {
        matches!(
            self,
            LineKind::Text
                | LineKind::Blank
                | LineKind::Rule
                | LineKind::Endnote
                | LineKind::Picture
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecoKind {
    Marker,
    Border,
    Note,
}

#[derive(Clone, Debug)]
pub struct Deco {
    pub x: u16,
    pub text: String,
    pub kind: DecoKind,
}

#[derive(Clone, Debug)]
pub struct VisLine {
    pub kind: LineKind,
    pub segs: Vec<Seg>,
    pub deco: Vec<Deco>,
    pub page: u32,
}

impl VisLine {
    fn new(kind: LineKind) -> VisLine {
        VisLine {
            kind,
            segs: Vec::new(),
            deco: Vec::new(),
            page: 0,
        }
    }
    fn deco_line(kind: LineKind, x: u16, text: String, dk: DecoKind) -> VisLine {
        VisLine {
            kind,
            segs: Vec::new(),
            deco: vec![Deco { x, text, kind: dk }],
            page: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GKind {
    Text,
    FnRef,
    Marker,
    Border,
    Note,
    /// Placeholder for a picture or other object a terminal can't show.
    Object,
}

/// A single displayed character cell.
#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub x: u16,
    pub ch: char,
    pub fmt: Fmt,
    pub kind: GKind,
    pub src: Option<Pos>,
    /// Text and background colour (RGB) from the document, if any.
    pub color: Option<u32>,
    pub bg: Option<u32>,
}

impl Glyph {
    pub fn plain(x: u16, ch: char, kind: GKind) -> Glyph {
        Glyph {
            x,
            ch,
            fmt: Fmt::default(),
            kind,
            src: None,
            color: None,
            bg: None,
        }
    }
}

pub struct Layout {
    pub lines: Vec<VisLine>,
    pub width: u16,
    /// Footnote/endnote reference labels (1, 2... and i, ii...).
    pub fn_num: HashMap<u32, String>,
    pub pages: u32,
    /// Pictures, in line order.
    pub pics: Vec<PicBox>,
    pub opts: LayoutOpts,
    by_pr: HashMap<ParaRef, Vec<usize>>,
}

pub fn str_width(s: &str) -> u16 {
    UnicodeWidthStr::width(s) as u16
}

fn char_width(ch: char, x_rel: u16, area: u16, cx: &Ctx) -> u16 {
    if ch == '\t' {
        return TAB - (x_rel % TAB);
    }
    if let Some(id) = fn_id(ch) {
        return cx.fn_num.get(&id).map(|l| str_width(l)).unwrap_or(1).max(1);
    }
    if let Some(o) = cx.st.obj(ch) {
        if let Some((cols, _)) = picture_box(o, area).filter(|_| cx.pictures) {
            return cols;
        }
        return if o.kind.width_label() {
            str_width(&o.label).max(1)
        } else {
            0
        };
    }
    if obj_id(ch).is_some() {
        return 0;
    }
    UnicodeWidthChar::width(ch).unwrap_or(1).max(1) as u16
}

struct WLine {
    start: usize,
    end: usize,
    xs: Vec<u16>,
    ws: Vec<u16>,
    end_x: u16,
    /// Ended by a line/page/paragraph break object.
    hard: bool,
    leaders: Vec<(usize, char)>,
}

/// Width of a tab at `rel` columns from the area's left edge: up to the next
/// tab stop (aligning the following text for right/centre/decimal stops),
/// or the next default stop every half inch.
fn tab_width(
    rel: u16,
    tabs: &[TabStop],
    cells: &[Cell],
    next: usize,
    area: u16,
    cx: &Ctx,
) -> (u16, char) {
    let Some(t) = tabs.iter().find(|t| t.pos > rel) else {
        return (TAB - (rel % TAB), ' ');
    };
    let follow = |stop_at_point: bool| -> u16 {
        let mut w = 0u16;
        for c in cells.iter().skip(next) {
            if c.ch == '\t'
                || cx.st.obj(c.ch).is_some_and(|o| o.kind.is_break())
                || (stop_at_point && (c.ch == '.' || c.ch == ','))
            {
                break;
            }
            w = w.saturating_add(char_width(c.ch, 0, area, cx));
            if w > 400 {
                break;
            }
        }
        w
    };
    let target = match t.kind {
        TabKind::Left => t.pos,
        TabKind::Right => t.pos.saturating_sub(follow(false)),
        TabKind::Center => t.pos.saturating_sub(follow(false) / 2),
        TabKind::Decimal => t.pos.saturating_sub(follow(true)),
    };
    (target.saturating_sub(rel).max(1), t.leader)
}

/// Greedy word wrap of one paragraph. `x0..x0+width` is the area; the first
/// line starts at `first_x`, later lines at `rest_x` (absolute columns).
fn wrap(
    cells: &[Cell],
    x0: u16,
    width: u16,
    first_x: u16,
    rest_x: u16,
    align: Align,
    tabs: &[TabStop],
    cx: &Ctx,
) -> Vec<WLine> {
    let limit = x0 + width;
    let n = cells.len();
    let mut lines: Vec<WLine> = Vec::new();
    if n == 0 {
        lines.push(WLine {
            start: 0,
            end: 0,
            xs: vec![],
            ws: vec![],
            end_x: first_x,
            hard: false,
            leaders: vec![],
        });
    }
    let mut i = 0;
    while i < n {
        let line_x = if lines.is_empty() { first_x } else { rest_x };
        let mut x = line_x;
        let (mut xs, mut ws) = (Vec::new(), Vec::new());
        let mut j = i;
        let mut brk: Option<usize> = None;
        let mut hard = false;
        let mut leaders = Vec::new();
        while j < n {
            let ch = cells[j].ch;
            if cx.st.obj(ch).is_some_and(|o| o.kind.is_break()) {
                xs.push(x);
                ws.push(0);
                j += 1;
                hard = true;
                break;
            }
            let w = if ch == '\t' {
                let (w, leader) = tab_width(x.saturating_sub(x0), tabs, cells, j + 1, width, cx);
                if leader != ' ' {
                    leaders.push((j, leader));
                }
                w
            } else {
                char_width(ch, x.saturating_sub(x0), width, cx)
            };
            if ch == ' ' {
                // Spaces may hang past the right margin.
                xs.push(x);
                ws.push(1);
                x += 1;
                j += 1;
                brk = Some(j);
                continue;
            }
            if x + w > limit && j > i {
                if let Some(b) = brk {
                    xs.truncate(b - i);
                    ws.truncate(b - i);
                    j = b;
                }
                break;
            }
            xs.push(x);
            ws.push(w);
            x += w;
            j += 1;
        }
        let end_x = match (xs.last(), ws.last()) {
            (Some(&a), Some(&b)) => a + b,
            _ => line_x,
        };
        leaders.retain(|(k, _)| *k < j);
        lines.push(WLine {
            start: i,
            end: j,
            xs,
            ws,
            end_x,
            hard,
            leaders,
        });
        i = j;
        if hard && i == n {
            // A break at the very end starts an empty last line.
            lines.push(WLine {
                start: n,
                end: n,
                xs: vec![],
                ws: vec![],
                end_x: rest_x,
                hard: false,
                leaders: vec![],
            });
        }
    }
    let nl = lines.len();
    for (k, l) in lines.iter_mut().enumerate() {
        let last = k + 1 == nl || l.hard;
        align_line(l, cells, limit, align, last);
    }
    lines
}

fn align_line(l: &mut WLine, cells: &[Cell], limit: u16, align: Align, last: bool) {
    if align == Align::Left {
        return;
    }
    let line_x = l.xs.first().copied().unwrap_or(l.end_x);
    let ve = (0..l.xs.len())
        .rev()
        .find(|&k| cells[l.start + k].ch != ' ');
    let vis_end = match ve {
        Some(k) => l.xs[k] + l.ws[k],
        None => line_x,
    };
    if vis_end >= limit {
        return;
    }
    let free = limit - vis_end;
    let shift = match align {
        Align::Center => free / 2,
        Align::Right => free,
        Align::Full => {
            let Some(ve) = ve else { return };
            if last {
                return;
            }
            let first_non = (0..=ve)
                .find(|&k| cells[l.start + k].ch != ' ')
                .unwrap_or(0);
            let spaces: Vec<usize> = (first_non..ve)
                .filter(|&k| cells[l.start + k].ch == ' ')
                .collect();
            if spaces.is_empty() {
                return;
            }
            let n = spaces.len() as u16;
            let (per, rem) = (free / n, free % n);
            let mut extra = 0u16;
            let mut si = 0usize;
            for k in 0..l.xs.len() {
                l.xs[k] += extra;
                if si < spaces.len() && spaces[si] == k {
                    let add = per + u16::from((si as u16) < rem);
                    l.ws[k] += add;
                    extra += add;
                    si += 1;
                }
            }
            l.end_x += extra;
            return;
        }
        Align::Left => 0,
    };
    for x in l.xs.iter_mut() {
        *x += shift;
    }
    l.end_x += shift;
}

pub fn alpha(n: u32) -> String {
    let mut n = n.max(1);
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

pub fn roman(n: u32) -> String {
    let table = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n.max(1);
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

pub fn list_marker(li: ListInfo, n: u32) -> String {
    match li.kind {
        ListKind::Bullet => ["•", "◦", "▪"][li.level as usize % 3].to_string(),
        ListKind::Number => match li.level % 3 {
            0 => format!("{n}."),
            1 => format!("{}.", alpha(n)),
            _ => format!("{}.", roman(n)),
        },
    }
}

/// Simple word wrap for plain strings (footnotes, headers).
pub fn wrap_plain(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    let mut lw = 0usize;
    for word in text.split_whitespace() {
        let ww = UnicodeWidthStr::width(word);
        if lw > 0 && lw + 1 + ww > width {
            out.push(std::mem::take(&mut line));
            lw = 0;
        }
        if lw > 0 {
            line.push(' ');
            lw += 1;
        }
        if ww > width {
            for ch in word.chars() {
                if lw >= width {
                    out.push(std::mem::take(&mut line));
                    lw = 0;
                }
                line.push(ch);
                lw += 1;
            }
        } else {
            line.push_str(word);
            lw += ww;
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    out
}

/// How a paragraph is drawn: list marker (with the Word list level's indent
/// and hanging indent, if any) and the style's formatting.
#[derive(Clone, Default)]
struct ParaLook {
    marker: Option<String>,
    list_geom: Option<(u16, u16)>,
    style: Fmt,
    style_cp: u32,
}

fn para_segs(
    pr: ParaRef,
    p: &Paragraph,
    x0: u16,
    width: u16,
    look: ParaLook,
    cx: &Ctx,
) -> (Vec<Seg>, Option<Deco>) {
    let props = &p.props;
    let indent = props.indent.min(width.saturating_sub(10));
    let marker = look.marker.filter(|m| !m.is_empty());
    let (first_x, rest_x, deco) = match (marker, props.list, look.list_geom) {
        (Some(m), Some(_), Some((left, hang))) => {
            let (left, hang) = if indent > 0 {
                (
                    indent,
                    if props.first_indent < 0 {
                        (-props.first_indent) as u16
                    } else {
                        hang
                    },
                )
            } else {
                (left, hang)
            };
            let left = left.min(width.saturating_sub(12));
            let mx = left.saturating_sub(hang);
            let mw = str_width(&m) + 1;
            let fx = if mx + mw > left { mx + mw } else { left };
            (
                x0 + fx,
                x0 + left,
                Some(Deco {
                    x: x0 + mx,
                    text: m,
                    kind: DecoKind::Marker,
                }),
            )
        }
        (Some(m), Some(li), None) => {
            let base = x0 + (indent + li.level as u16 * 4).min(width.saturating_sub(12));
            let mw = (str_width(&m) + 1).max(3);
            (
                base + mw,
                base + mw,
                Some(Deco {
                    x: base,
                    text: m,
                    kind: DecoKind::Marker,
                }),
            )
        }
        _ => {
            let maxf = (width as i32 - 10).max(0);
            let fx = (indent as i32 + props.first_indent as i32).clamp(0, maxf) as u16;
            (x0 + fx, x0 + indent, None)
        }
    };
    let wl = wrap(
        &p.cells,
        x0,
        width,
        first_x,
        rest_x,
        props.align,
        &props.tabs,
        cx,
    );
    let n = wl.len();
    let (style, style_cp) = (look.style, look.style_cp);
    let segs = wl
        .into_iter()
        .enumerate()
        .map(|(k, l)| Seg {
            pr,
            start: l.start,
            end: l.end,
            xs: l.xs,
            ws: l.ws,
            end_x: l.end_x,
            last: k + 1 == n,
            pad: false,
            style,
            style_cp,
            leaders: l.leaders,
            lo: x0,
            hi: x0 + width,
        })
        .collect();
    (segs, deco)
}

struct Builder<'a> {
    doc: &'a Document,
    st: &'a Store,
    endnotes: Vec<u32>,
    /// Space after the previous paragraph, and its style and contextual flag.
    pending_after: u8,
    prev_style: Option<(String, bool)>,
    fn_num: HashMap<u32, String>,
    width: u16,
    cap: usize,
    lines: Vec<VisLine>,
    used: usize,
    notes_used: usize,
    notes: Vec<u32>,
    page: u32,
    pictures: bool,
}

impl Builder<'_> {
    fn ctx(&self, pictures: bool) -> Ctx<'_> {
        Ctx {
            fn_num: &self.fn_num,
            st: self.st,
            pictures: pictures && self.pictures,
        }
    }

    /// Lines the tallest full-size picture on a line needs.
    fn picture_rows(&self, p: &Paragraph, seg: &Seg) -> u16 {
        if !self.pictures {
            return 1;
        }
        p.cells[seg.start..seg.end]
            .iter()
            .filter_map(|c| picture_box(self.st.obj(c.ch)?, self.width))
            .map(|(_, r)| r)
            .max()
            .unwrap_or(1)
            .clamp(1, self.cap.max(1) as u16)
    }

    fn note_lines(&self, id: u32) -> Vec<String> {
        let num = self.fn_num.get(&id).cloned().unwrap_or_default();
        let text = self
            .doc
            .footnotes
            .get(&id)
            .map(String::as_str)
            .unwrap_or("");
        wrap_plain(&format!("{num}. {text}"), self.width as usize)
    }

    fn add(&mut self, mut line: VisLine) {
        let mut new_notes: Vec<u32> = Vec::new();
        for s in &line.segs {
            if s.pad {
                continue;
            }
            let p = self.doc.para(s.pr);
            for c in &p.cells[s.start..s.end] {
                if let Some(id) = fn_id(c.ch) {
                    if self.doc.note_meta.get(&id).is_some_and(|m| m.endnote) {
                        if !self.endnotes.contains(&id) {
                            self.endnotes.push(id);
                        }
                    } else if !self.notes.contains(&id) && !new_notes.contains(&id) {
                        new_notes.push(id);
                    }
                }
            }
        }
        // Space the new footnotes need at the bottom of the page (plus the
        // separator line if they are the page's first notes).
        let need = |b: &Self| -> usize {
            new_notes
                .iter()
                .map(|&id| b.note_lines(id).len())
                .sum::<usize>()
                + usize::from(!new_notes.is_empty() && b.notes.is_empty())
        };
        let mut note_lines = need(self);
        let body = line.kind.is_body();
        if body && self.used > 0 && self.used + self.notes_used + 1 + note_lines > self.cap {
            self.break_page(false);
            if line.kind == LineKind::Blank {
                return; // don't start a page with a blank line
            }
            note_lines = need(self);
        }
        line.page = self.page;
        self.lines.push(line);
        if body {
            self.used += 1;
        }
        self.notes.extend(new_notes);
        self.notes_used += note_lines;
    }

    fn flush_notes(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let page = self.page;
        let mut sep = VisLine::deco_line(LineKind::NoteSep, 0, "─".repeat(20), DecoKind::Note);
        sep.page = page;
        self.lines.push(sep);
        for id in std::mem::take(&mut self.notes) {
            for s in self.note_lines(id) {
                let mut l = VisLine::deco_line(LineKind::Note, 0, s, DecoKind::Note);
                l.page = page;
                self.lines.push(l);
            }
        }
        self.notes_used = 0;
    }

    fn break_page(&mut self, forced: bool) {
        self.break_page_kind(forced, false);
    }

    fn break_page_kind(&mut self, forced: bool, section: bool) {
        self.flush_notes();
        self.page += 1;
        let mut l = VisLine::new(LineKind::PageBreak { forced, section });
        l.page = self.page;
        self.lines.push(l);
        self.used = 0;
    }

    fn para_look(&self, p: &Paragraph, marker: Option<String>) -> ParaLook {
        let sd = self.doc.styles.get(&p.props.style);
        let list_geom = p.props.list.filter(|l| l.num != 0).and_then(|l| {
            let def = self.doc.numbering.level(l.num, l.level)?;
            Some((def.left?, def.hanging.unwrap_or(0)))
        });
        ParaLook {
            marker,
            list_geom,
            style: sd.map(|s| s.fmt).unwrap_or_default(),
            style_cp: sd.map(|s| s.cp).unwrap_or(0),
        }
    }

    fn para(&mut self, pr: ParaRef, p: &Paragraph, marker: Option<String>) {
        // Space between paragraphs: after the previous plus before this one,
        // except between paragraphs of one style with contextual spacing.
        let same = self
            .prev_style
            .as_ref()
            .map(|(s, _)| *s == p.props.style)
            .unwrap_or(false);
        let prev_ctx = self.prev_style.as_ref().is_some_and(|(_, c)| *c);
        let after = if same && prev_ctx {
            0
        } else {
            self.pending_after
        };
        let before = if same && p.props.contextual || self.lines.is_empty() {
            0
        } else {
            p.props.space_before
        };
        for _ in 0..after.saturating_add(before).min(4) {
            self.add(VisLine::new(LineKind::Blank));
        }
        self.pending_after = p.props.space_after;
        self.prev_style = Some((p.props.style.clone(), p.props.contextual));
        let look = self.para_look(p, marker);
        let (segs, mut deco) = para_segs(pr, p, 0, self.width, look, &self.ctx(true));
        for seg in segs {
            // A tall picture: keep its lines on one page, above the line
            // it sits on (like Word, the line's text aligns with its foot).
            let rows = self.picture_rows(p, &seg) as usize;
            if rows > 1 {
                if self.used > 0 && self.used + self.notes_used + rows > self.cap {
                    self.break_page(false);
                }
                for _ in 1..rows {
                    self.add(VisLine::new(LineKind::Picture));
                }
            }
            let page_break = seg.end > seg.start
                && self
                    .st
                    .obj(p.cells[seg.end - 1].ch)
                    .is_some_and(|o| o.kind == ObjKind::PageBreak);
            let mut l = VisLine::new(LineKind::Text);
            l.deco.extend(deco.take());
            l.segs.push(seg);
            self.add(l);
            if page_break {
                self.break_page(true);
                continue;
            }
            if p.props.spacing >= 2 {
                self.add(VisLine::new(LineKind::Blank));
            }
        }
        // A section ends with this paragraph.
        if let Some((_, xml)) = p
            .extra
            .as_ref()
            .and_then(|x| x.ppr.iter().find(|(n, _)| n == "sectPr"))
        {
            let continuous = xml.contains("w:val=\"continuous\"");
            if continuous {
                let mut l = VisLine::new(LineKind::SectionMark);
                l.page = self.page;
                self.lines.push(l);
            } else {
                self.break_page_kind(true, true);
            }
        }
    }

    fn flush_endnotes(&mut self) {
        if self.endnotes.is_empty() {
            return;
        }
        self.add(VisLine::new(LineKind::Blank));
        self.add(VisLine::deco_line(
            LineKind::Endnote,
            0,
            "─".repeat(20),
            DecoKind::Note,
        ));
        for id in std::mem::take(&mut self.endnotes) {
            for s in self.note_lines(id) {
                self.add(VisLine::deco_line(LineKind::Endnote, 0, s, DecoKind::Note));
            }
        }
    }

    fn table(&mut self, bi: usize, t: &Table) {
        for _ in 0..std::mem::take(&mut self.pending_after) {
            self.add(VisLine::new(LineKind::Blank));
        }
        self.prev_style = None;
        let cols = t.fitted_cols(self.width);
        let mut bx = Vec::with_capacity(cols.len());
        let mut acc = 0u16;
        for &c in &cols {
            bx.push(acc);
            acc += c + 3;
        }
        let right = acc;
        let n = cols.len();
        // Horizontally merged cells: a covered cell belongs to the cell on
        // its left, which spans it (no border between them).
        let owners: Vec<Vec<usize>> = t
            .rows
            .iter()
            .map(|row| {
                let mut o: Vec<usize> = Vec::with_capacity(n);
                for (ci, p) in row.iter().enumerate() {
                    let covered = ci > 0 && p.extra.as_ref().is_some_and(|x| x.covered);
                    o.push(if covered { o[ci - 1] } else { ci });
                }
                o
            })
            .collect();
        let has_border = |ri: usize, k: usize| owners[ri][k] == k;
        let rule = |above: Option<usize>, below: Option<usize>| -> String {
            let mut s = String::new();
            for (k, &c) in cols.iter().enumerate() {
                let up = above.is_some_and(|r| has_border(r, k));
                let down = below.is_some_and(|r| has_border(r, k));
                s.push(match (k == 0, above.is_some(), below.is_some(), up, down) {
                    (true, false, _, _, _) => '┌',
                    (true, _, false, _, _) => '└',
                    (true, _, _, _, _) => '├',
                    (_, _, _, true, true) => '┼',
                    (_, _, _, true, false) => '┴',
                    (_, _, _, false, true) => '┬',
                    _ => '─',
                });
                s.extend(std::iter::repeat_n('─', c as usize + 2));
            }
            s.push(match (above.is_some(), below.is_some()) {
                (false, _) => '┐',
                (_, false) => '┘',
                _ => '┤',
            });
            s
        };
        let rule_line = |s: String| VisLine::deco_line(LineKind::Rule, 0, s, DecoKind::Border);
        self.add(rule_line(rule(None, Some(0))));
        for (ri, row) in t.rows.iter().enumerate() {
            let own = &owners[ri];
            let span_width = |ci: usize| {
                let k = own.iter().filter(|&&o| o == ci).count().max(1);
                cols[ci..ci + k].iter().sum::<u16>() + 3 * (k as u16 - 1)
            };
            let cell_segs: Vec<Option<Vec<Seg>>> = row
                .iter()
                .enumerate()
                .map(|(ci, p)| {
                    (own[ci] == ci).then(|| {
                        let look = self.para_look(p, None);
                        para_segs(
                            ParaRef::cell(bi, ri, ci),
                            p,
                            bx[ci] + 2,
                            span_width(ci),
                            look,
                            &self.ctx(false),
                        )
                        .0
                    })
                })
                .collect();
            let h = cell_segs
                .iter()
                .flatten()
                .map(Vec::len)
                .max()
                .unwrap_or(1)
                .max(1);
            for li in 0..h {
                let mut l = VisLine::new(LineKind::Text);
                for (ci, segs) in cell_segs.iter().enumerate() {
                    let Some(segs) = segs else { continue };
                    match segs.get(li) {
                        Some(s) => l.segs.push(s.clone()),
                        None => {
                            let plen = row[ci].len();
                            l.segs.push(Seg {
                                pr: ParaRef::cell(bi, ri, ci),
                                start: plen,
                                end: plen,
                                xs: vec![],
                                ws: vec![],
                                end_x: bx[ci] + 2,
                                last: true,
                                pad: true,
                                style: Fmt::default(),
                                style_cp: 0,
                                leaders: Vec::new(),
                                lo: bx[ci] + 2,
                                hi: bx[ci] + 2 + span_width(ci),
                            })
                        }
                    }
                }
                for (k, &x) in bx.iter().enumerate() {
                    if has_border(ri, k) {
                        l.deco.push(Deco {
                            x,
                            text: "│".into(),
                            kind: DecoKind::Border,
                        });
                    }
                }
                l.deco.push(Deco {
                    x: right,
                    text: "│".into(),
                    kind: DecoKind::Border,
                });
                self.add(l);
            }
            if ri + 1 < t.rows.len() {
                self.add(rule_line(rule(Some(ri), Some(ri + 1))));
            }
        }
        self.add(rule_line(rule(Some(t.rows.len() - 1), None)));
    }
}

impl Layout {
    pub fn build(doc: &Document) -> Layout {
        Layout::build_with(doc, LayoutOpts::current())
    }

    pub fn build_with(doc: &Document, opts: LayoutOpts) -> Layout {
        let width = doc.page.text_cols();
        let mut fn_num: HashMap<u32, String> = HashMap::new();
        let (mut nf, mut ne) = (0u32, 0u32);
        for id in doc.footnote_numbers() {
            let label = if doc.note_meta.get(&id).is_some_and(|m| m.endnote) {
                ne += 1;
                roman(ne)
            } else {
                nf += 1;
                nf.to_string()
            };
            fn_num.insert(id, label);
        }
        let cap = doc
            .page
            .body_lines()
            .saturating_sub(reserved_lines(doc))
            .max(5);
        let st = doc.store.borrow();
        let mut b = Builder {
            doc,
            st: &st,
            endnotes: Vec::new(),
            pending_after: 0,
            prev_style: None,
            fn_num,
            width,
            cap,
            lines: Vec::new(),
            used: 0,
            notes_used: 0,
            notes: Vec::new(),
            page: 1,
            pictures: opts.full_size_pictures,
        };
        let mut counters = [0u32; 9];
        let mut word_counters: HashMap<u32, [u32; 9]> = HashMap::new();
        for (bi, block) in doc.blocks.iter().enumerate() {
            match block {
                Block::Para(p) => {
                    let marker = match p.props.list {
                        // Word lists count per numbering instance, across
                        // other paragraphs, using the file's definitions.
                        Some(li) if li.num != 0 && doc.numbering.nums.contains_key(&li.num) => {
                            let lv = (li.level as usize).min(8);
                            let c = word_counters.entry(li.num).or_insert([0; 9]);
                            c[lv] = if c[lv] == 0 {
                                doc.numbering.start(li.num, lv as u8)
                            } else {
                                c[lv] + 1
                            };
                            for k in c[lv + 1..].iter_mut() {
                                *k = 0;
                            }
                            let c = *c;
                            Some(
                                doc.numbering
                                    .marker(li.num, lv as u8, &c)
                                    .unwrap_or_else(|| list_marker(li, c[lv])),
                            )
                        }
                        Some(li) => {
                            let lv = (li.level as usize).min(8);
                            if li.kind == ListKind::Number {
                                counters[lv] += 1;
                            } else {
                                counters[lv] = 0;
                            }
                            for c in counters[lv + 1..].iter_mut() {
                                *c = 0;
                            }
                            Some(list_marker(li, counters[lv]))
                        }
                        None => {
                            counters = [0; 9];
                            None
                        }
                    };
                    let after_break = matches!(
                        b.lines.last().map(|l| l.kind),
                        Some(LineKind::PageBreak { .. })
                    );
                    if p.props.page_break_before && !b.lines.is_empty() && !after_break {
                        b.break_page(true);
                    }
                    b.para(ParaRef::top(bi), p, marker);
                }
                Block::Table(t) => {
                    counters = [0; 9];
                    b.table(bi, t);
                }
            }
        }
        b.flush_endnotes();
        b.flush_notes();
        let mut by_pr: HashMap<ParaRef, Vec<usize>> = HashMap::new();
        let mut pics = Vec::new();
        for (li, l) in b.lines.iter().enumerate() {
            // Lines reserved above for full-size pictures.
            let above = b.lines[..li]
                .iter()
                .rev()
                .take_while(|x| x.kind == LineKind::Picture)
                .count() as u16;
            for s in &l.segs {
                if s.pad {
                    continue;
                }
                by_pr.entry(s.pr).or_default().push(li);
                let p = doc.para(s.pr);
                for k in s.start..s.end {
                    let ch = p.cells[k].ch;
                    let Some(o) = st.obj(ch).filter(|o| crate::pictures::is_picture(o)) else {
                        continue;
                    };
                    let full = picture_box(o, width)
                        .filter(|_| opts.full_size_pictures && s.pr.cell.is_none());
                    let rows = full.map(|(_, r)| r.min(above + 1)).unwrap_or(1);
                    let (x, cols) = (s.xs[k - s.start], s.ws[k - s.start]);
                    pics.push(PicBox {
                        line: li,
                        x,
                        cols,
                        rows,
                        obj: ch,
                        src: Pos::new(s.pr, k),
                    });
                }
            }
        }
        let (lines, fn_num, pages) = (b.lines, b.fn_num, b.page);
        Layout {
            lines,
            width,
            fn_num,
            pages,
            pics,
            opts,
            by_pr,
        }
    }

    /// Screen line and column of a cursor position.
    pub fn locate(&self, pos: Pos) -> (usize, u16) {
        if let Some(ids) = self.by_pr.get(&pos.pr) {
            for &li in ids {
                for s in &self.lines[li].segs {
                    if s.pr != pos.pr || s.pad {
                        continue;
                    }
                    if pos.off >= s.start && (pos.off < s.end || (s.last && pos.off == s.end)) {
                        let x = if pos.off < s.end {
                            s.xs[pos.off - s.start]
                        } else {
                            s.end_x
                        };
                        return (li, x);
                    }
                }
            }
            if let Some(&li) = ids.last() {
                let x = self.lines[li]
                    .segs
                    .iter()
                    .find(|s| s.pr == pos.pr)
                    .map(|s| s.end_x)
                    .unwrap_or(0);
                return (li, x);
            }
        }
        (0, 0)
    }

    pub fn seg_at(&self, pos: Pos) -> Option<&Seg> {
        let (li, _) = self.locate(pos);
        self.lines
            .get(li)?
            .segs
            .iter()
            .find(|s| s.pr == pos.pr && !s.pad)
    }

    fn choose_seg(line: &VisLine, x: u16) -> Option<&Seg> {
        line.segs
            .iter()
            .find(|s| x >= s.lo && x < s.hi)
            .or_else(|| {
                line.segs.iter().min_by_key(|s| {
                    if x < s.lo {
                        s.lo - x
                    } else {
                        x.saturating_sub(s.hi)
                    }
                })
            })
    }

    fn seg_offset(s: &Seg, x: u16) -> usize {
        if s.pad {
            return s.start;
        }
        for k in s.start..s.end {
            let next = if k + 1 < s.end {
                s.xs[k + 1 - s.start]
            } else {
                s.end_x
            };
            if x < next {
                return k;
            }
        }
        if s.last {
            s.end
        } else {
            s.end.saturating_sub(1).max(s.start)
        }
    }

    /// Document position at a screen line/column (mouse clicks).
    pub fn hit(&self, li: usize, x: u16) -> Option<Pos> {
        let line = self.lines.get(li)?;
        let s = Self::choose_seg(line, x)?;
        Some(Pos::new(s.pr, Self::seg_offset(s, x)))
    }

    /// Move one visual line up (`dir` < 0) or down, keeping `goal_x`.
    pub fn vertical(&self, pos: Pos, goal_x: u16, dir: i32) -> Option<Pos> {
        let (li, _) = self.locate(pos);
        let mut j = li as i64;
        loop {
            j += dir as i64;
            if j < 0 || j >= self.lines.len() as i64 {
                return None;
            }
            let line = &self.lines[j as usize];
            let Some(s) = Self::choose_seg(line, goal_x) else {
                continue;
            };
            if s.pad {
                continue;
            }
            return Some(Pos::new(s.pr, Self::seg_offset(s, goal_x)));
        }
    }

    /// Positions of the start and end of the visual line containing `pos`.
    pub fn line_bounds(&self, pos: Pos) -> (usize, usize) {
        match self.seg_at(pos) {
            Some(s) => {
                let end = if s.last {
                    s.end
                } else {
                    s.end.saturating_sub(1).max(s.start)
                };
                (s.start, end)
            }
            None => (0, 0),
        }
    }

    /// Visible character cells of a line.
    pub fn glyphs(&self, li: usize, doc: &Document) -> Vec<Glyph> {
        let line = &self.lines[li];
        let mut out = Vec::new();
        for d in &line.deco {
            let kind = match d.kind {
                DecoKind::Marker => GKind::Marker,
                DecoKind::Border => GKind::Border,
                DecoKind::Note => GKind::Note,
            };
            let mut x = d.x;
            for ch in d.text.chars() {
                out.push(Glyph::plain(x, ch, kind));
                x += UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
            }
        }
        let st = doc.store.borrow();
        // Effective formatting, colours and caps for a cell's run properties.
        let mut memo: HashMap<(u32, u32, Fmt), (Fmt, Option<u32>, Option<u32>, bool)> =
            HashMap::new();
        let mut resolve = |cp: u32, s: &Seg, own: Fmt| -> (Fmt, Option<u32>, Option<u32>, bool) {
            *memo.entry((cp, s.style_cp, own)).or_insert_with(|| {
                let run = st.look(cp);
                let cs = run.char_style.as_deref().and_then(|id| doc.styles.get(id));
                let mut look = run.clone();
                if let Some(cs) = cs {
                    look = look.over(st.look(cs.cp));
                }
                look = look
                    .over(st.look(s.style_cp))
                    .over(st.look(doc.styles.base_cp));
                let sfmt = s.style.union(cs.map(|c| c.fmt).unwrap_or_default());
                let fmt = Fmt::effective(sfmt, own);
                let mut color = if look.hidden == Some(true) {
                    Some(0x808080)
                } else {
                    look.rgb()
                };
                let mut fmt = fmt;
                // Hyperlinks and tracked insertions, as Word shows them.
                for &w in &st.cprops(cp).wraps {
                    match st.wraps.get(w as usize).map(|d| d.kind) {
                        Some(WrapKind::Link) => {
                            fmt = fmt.with(Fmt::UNDERLINE, true);
                            color = color.or(Some(0x0563C1));
                        }
                        Some(WrapKind::Insert) => {
                            fmt = fmt.with(Fmt::UNDERLINE, true);
                            color = Some(0x2E9E3E);
                        }
                        _ => {}
                    }
                }
                let caps = look.caps == Some(true) || look.small_caps == Some(true);
                (fmt, color, look.background(), caps)
            })
        };
        for s in &line.segs {
            if s.pad {
                continue;
            }
            let p = doc.para(s.pr);
            for k in s.start..s.end {
                let c = p.cells[k];
                let (x, w) = (s.xs[k - s.start], s.ws[k - s.start]);
                let src = Some(Pos::new(s.pr, k));
                let (fmt, color, bg, caps) = resolve(c.cp, s, c.fmt);
                let g = |x: u16, ch: char, fmt: Fmt, kind: GKind| Glyph {
                    x,
                    ch,
                    fmt,
                    kind,
                    src,
                    color,
                    bg,
                };
                if obj_id(c.ch).is_some() {
                    if let Some(o) = st.obj(c.ch).filter(|o| o.kind.width_label()) {
                        let mut gx = x;
                        let deleted = crate::review::is_deletion(o);
                        for ch in fit_label(&o.label, w).chars() {
                            let mut gl = g(gx, ch, fmt, GKind::Object);
                            if deleted {
                                gl.color = Some(0xC00000);
                                gl.kind = GKind::Text;
                            }
                            out.push(gl);
                            gx += UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                        }
                    }
                } else if let Some(id) = fn_id(c.ch) {
                    let num = self.fn_num.get(&id).cloned().unwrap_or_default();
                    for (d, ch) in num.chars().enumerate() {
                        out.push(g(x + d as u16, ch, Fmt(Fmt::SUPER), GKind::FnRef));
                    }
                } else if c.ch == ' ' || c.ch == '\t' {
                    let leader = s.leaders.iter().find(|(i, _)| *i == k).map(|(_, l)| *l);
                    for d in 0..w {
                        let ch = match leader {
                            Some(l) if d + 1 < w && d > 0 => l,
                            _ => ' ',
                        };
                        out.push(g(x + d, ch, fmt, GKind::Text));
                    }
                } else {
                    let ch = if c.ch.is_control() {
                        '?'
                    } else if c.ch == '\u{2011}' {
                        '-'
                    } else if caps {
                        c.ch.to_uppercase().next().unwrap_or(c.ch)
                    } else {
                        c.ch
                    };
                    out.push(g(x, ch, fmt, GKind::Text));
                }
            }
        }
        out
    }

    /// Page number and line-within-page of a layout line.
    pub fn page_line(&self, li: usize) -> (u32, usize) {
        let Some(line) = self.lines.get(li) else {
            return (1, 0);
        };
        let page = line.page;
        let mut n = 0;
        let mut k = li;
        while k > 0 {
            k -= 1;
            let l = &self.lines[k];
            if l.page != page || matches!(l.kind, LineKind::PageBreak { .. }) {
                break;
            }
            if l.kind.is_body() {
                n += 1;
            }
        }
        (page.max(1), n)
    }

    /// First layout line of a page.
    pub fn page_start(&self, page: u32) -> usize {
        self.lines.iter().position(|l| l.page >= page).unwrap_or(0)
    }
}

/// A placeholder label cut to `w` columns (a full-size picture may be
/// narrower than its label).
fn fit_label(label: &str, w: u16) -> std::borrow::Cow<'_, str> {
    if str_width(label) <= w.max(1) {
        return label.into();
    }
    if w <= 1 {
        return "▣".into();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in label.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
        if used + cw + 1 > w {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out.into()
}

/// Lines of the page body taken by header and footer (text + blank each).
pub fn reserved_lines(doc: &Document) -> usize {
    let (h, f) = crate::print::header_footer_rows(doc);
    crate::print::hf_height(&h) + crate::print::hf_height(&f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(text: &str, align: Align) -> Document {
        let mut d = Document::new();
        let mut p = Paragraph::from_text(text, Fmt::default());
        p.props.align = align;
        d.blocks = vec![Block::Para(p)];
        d
    }

    #[test]
    fn wraps_at_width() {
        let d = doc_with(&"word ".repeat(40), Align::Left);
        let l = Layout::build(&d);
        assert!(l.lines.len() > 2);
        for line in &l.lines {
            for s in &line.segs {
                // Every non-space glyph fits within the text width.
                let p = d.para(s.pr);
                for k in s.start..s.end {
                    if p.cells[k].ch != ' ' {
                        assert!(s.xs[k - s.start] < l.width);
                    }
                }
            }
        }
    }

    #[test]
    fn locate_and_hit_roundtrip() {
        let d = doc_with(&"alpha beta gamma delta ".repeat(10), Align::Full);
        let l = Layout::build(&d);
        let pr = ParaRef::top(0);
        for off in 0..=d.para(pr).len() {
            let (li, x) = l.locate(Pos::new(pr, off));
            assert_eq!(l.hit(li, x), Some(Pos::new(pr, off)), "offset {off}");
        }
    }

    #[test]
    fn paginates() {
        let mut d = Document::new();
        d.blocks = (0..200)
            .map(|i| Block::Para(Paragraph::from_text(&format!("line {i}"), Fmt::default())))
            .collect();
        let l = Layout::build(&d);
        assert_eq!(l.pages, 4); // 54 lines per letter page
    }

    /// A paragraph "a [picture] b" after `before` one-line paragraphs.
    fn picture_doc(before: usize, cx: u64, cy: u64) -> (Document, char) {
        let mut d = Document::new();
        let xml = crate::pictures::tests::picture_xml("rId1", cx, cy);
        let ch = d.new_obj(InlineObj {
            kind: ObjKind::Visible,
            xml,
            label: "[Image: test]".into(),
            in_run: true,
        });
        let mut p = Paragraph::from_text("a  b", Fmt::default());
        p.cells_mut()
            .insert(2, Cell::with_cp(ch, Fmt::default(), 0));
        d.blocks = (0..before)
            .map(|i| Block::Para(Paragraph::from_text(&format!("line {i}"), Fmt::default())))
            .collect();
        d.blocks.push(Block::Para(p));
        (d, ch)
    }

    const FULL: LayoutOpts = LayoutOpts {
        full_size_pictures: true,
    };
    const LABELS: LayoutOpts = LayoutOpts {
        full_size_pictures: false,
    };

    #[test]
    fn full_size_pictures_reserve_lines() {
        // 1" x 1": 10 columns, 6 lines.
        let (d, ch) = picture_doc(0, 914_400, 914_400);
        let l = Layout::build_with(&d, FULL);
        let kinds: Vec<LineKind> = l.lines.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [vec![LineKind::Picture; 5], vec![LineKind::Text]].concat()
        );
        assert_eq!(
            l.pics,
            vec![PicBox {
                line: 5,
                x: 2,
                cols: 10,
                rows: 6,
                obj: ch,
                src: Pos::new(ParaRef::top(0), 2)
            }]
        );
        let g = l.glyphs(5, &d);
        let label: String = g
            .iter()
            .filter(|g| g.kind == GKind::Object)
            .map(|g| g.ch)
            .collect();
        assert_eq!(label, "[Image: t…");
        assert_eq!(l.locate(Pos::new(ParaRef::top(0), 4)), (5, 13));
        // As a placeholder: one line, as wide as the label.
        let l = Layout::build_with(&d, LABELS);
        assert_eq!(l.lines.len(), 1);
        assert_eq!((l.pics[0].cols, l.pics[0].rows), (13, 1));
    }

    #[test]
    fn tall_picture_moves_to_next_page_whole() {
        let (d, _) = picture_doc(52, 914_400, 914_400);
        let l = Layout::build_with(&d, FULL);
        assert_eq!(l.pages, 2);
        let first = l
            .lines
            .iter()
            .position(|x| x.kind == LineKind::Picture)
            .unwrap();
        assert!(l.lines[first..].iter().all(|x| x.page == 2));
        let pb = l.pics[0];
        assert_eq!((l.lines[pb.line].page, pb.rows), (2, 6));
        // The printed page puts the picture's box at the top.
        let pages = crate::print::compose(&d, &l);
        assert_eq!(
            pages[1]
                .pics
                .iter()
                .map(|(top, p)| (*top, p.rows))
                .collect::<Vec<_>>(),
            vec![(0, 6)]
        );
        assert!(pages[1].rows[5].iter().any(|g| g.ch == 'b'));
    }

    #[test]
    fn wide_picture_shrinks_to_fit() {
        // 20" x 2" on a 6.5" line: 65 columns, proportionally 2 lines.
        let (d, _) = picture_doc(0, 20 * 914_400, 2 * 914_400);
        let l = Layout::build_with(&d, FULL);
        assert_eq!((l.pics[0].cols, l.pics[0].rows), (65, 4));
    }

    #[test]
    fn markers() {
        assert_eq!(roman(14), "xiv");
        assert_eq!(alpha(28), "ab");
    }
}
