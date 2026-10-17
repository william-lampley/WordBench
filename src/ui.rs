//! Drawing: document view, status line, menu bar, dialogs, Reveal Codes,
//! print preview and help.

use crate::app::*;
use crate::commands::{self, Keymap, MENUS};
use crate::graphics::Placement;
use crate::layout::{GKind, Glyph, LineKind, str_width};
use crate::model::*;
use crate::reveal::{self, Tok};
use ratatui::Frame;
use ratatui::buffer::{Buffer, CellDiffOption};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthChar;

struct Palette {
    text: Style,
    outline: Style,
    /// Background colour of the text area (for keeping document colours
    /// readable).
    bg_rgb: u32,
    bold_fg: Option<Color>,
    script_fg: Color,
    select: Style,
    status: Style,
    status_strong: Style,
    menu: Style,
    menu_sel: Style,
    dialog: Style,
    dialog_sel: Style,
    dialog_title: Style,
    border: Style,
    dim: Style,
    fnref: Style,
    code: Style,
    ruler: Style,
    desk: Style,
    paper: Style,
}

/// A colour theme: every screen element as RGB, chosen so each one stays
/// readable on its background (checked by a test).
pub struct ThemeSpec {
    pub name: &'static str,
    pub bg: u32,
    pub fg: u32,
    /// Bold text (brighter in dark themes).
    pub bold: u32,
    /// Notes, page breaks, rulers.
    pub dim: u32,
    /// Table borders.
    pub border: u32,
    /// Footnote numbers, picture placeholders.
    pub accent: u32,
    /// Reveal Codes codes.
    pub code: u32,
    pub script: u32,
    pub sel_bg: u32,
    pub sel_fg: u32,
    pub bar_bg: u32,
    pub bar_fg: u32,
    pub menu_bg: u32,
    pub menu_fg: u32,
    pub hi_bg: u32,
    pub hi_fg: u32,
    /// Paper outline: deliberately faint.
    pub outline: u32,
}

pub const THEMES: &[(Theme, ThemeSpec)] = &[
    (
        Theme::Blue,
        ThemeSpec {
            name: "Classic blue",
            bg: 0x0000AA,
            fg: 0xD0D0D0,
            bold: 0xFFFFFF,
            dim: 0x9FB4FF,
            border: 0x7FFFFF,
            accent: 0xFFFF55,
            code: 0xFFFF55,
            script: 0x7FFFFF,
            sel_bg: 0xD0D0D0,
            sel_fg: 0x0000AA,
            bar_bg: 0x0000AA,
            bar_fg: 0xD0D0D0,
            menu_bg: 0xC0C0C0,
            menu_fg: 0x000000,
            hi_bg: 0x000080,
            hi_fg: 0xFFFFFF,
            outline: 0x3A4FD6,
        },
    ),
    (
        Theme::Dark,
        ThemeSpec {
            name: "Dark",
            bg: 0x1E1E2E,
            fg: 0xCDD6F4,
            bold: 0xFFFFFF,
            dim: 0x8A8FB0,
            border: 0x89B4FA,
            accent: 0xF9E2AF,
            code: 0xFAB387,
            script: 0x94E2D5,
            sel_bg: 0x45475A,
            sel_fg: 0xFFFFFF,
            bar_bg: 0x313244,
            bar_fg: 0xCDD6F4,
            menu_bg: 0x313244,
            menu_fg: 0xCDD6F4,
            hi_bg: 0x89B4FA,
            hi_fg: 0x11111B,
            outline: 0x45475E,
        },
    ),
    (
        Theme::Light,
        ThemeSpec {
            name: "Light (paper)",
            bg: 0xFBFBF8,
            fg: 0x1F1F1F,
            bold: 0x000000,
            dim: 0x6B6B6B,
            border: 0x3B6EA5,
            accent: 0x9A5B00,
            code: 0xA6266E,
            script: 0x00727A,
            sel_bg: 0xB3D4FF,
            sel_fg: 0x000000,
            bar_bg: 0xE4E4DE,
            bar_fg: 0x1F1F1F,
            menu_bg: 0xE4E4DE,
            menu_fg: 0x1F1F1F,
            hi_bg: 0x2F5F9F,
            hi_fg: 0xFFFFFF,
            outline: 0xCFCFC6,
        },
    ),
    (
        Theme::Solarized,
        ThemeSpec {
            name: "Solarized dark",
            bg: 0x002B36,
            fg: 0x93A1A1,
            bold: 0xEEE8D5,
            dim: 0x7A9095,
            border: 0x2AA198,
            accent: 0xB58900,
            code: 0xCB4B16,
            script: 0x6C71C4,
            sel_bg: 0x586E75,
            sel_fg: 0xFDF6E3,
            bar_bg: 0x073642,
            bar_fg: 0x93A1A1,
            menu_bg: 0xEEE8D5,
            menu_fg: 0x073642,
            hi_bg: 0x1C6BA0,
            hi_fg: 0xFDF6E3,
            outline: 0x2A5560,
        },
    ),
    (
        Theme::Amber,
        ThemeSpec {
            name: "Amber screen",
            bg: 0x140C00,
            fg: 0xFFB000,
            bold: 0xFFD27F,
            dim: 0xC08400,
            border: 0xE89A00,
            accent: 0xFFE0A0,
            code: 0xFFE0A0,
            script: 0xFFCC66,
            sel_bg: 0xFFB000,
            sel_fg: 0x140C00,
            bar_bg: 0x3A2600,
            bar_fg: 0xFFB000,
            menu_bg: 0xFFB000,
            menu_fg: 0x140C00,
            hi_bg: 0x140C00,
            hi_fg: 0xFFD27F,
            outline: 0x5A3D00,
        },
    ),
    (
        Theme::Green,
        ThemeSpec {
            name: "Green screen",
            bg: 0x001400,
            fg: 0x33FF66,
            bold: 0xB3FFC6,
            dim: 0x1FB34A,
            border: 0x29D957,
            accent: 0xCCFFD9,
            code: 0xCCFFD9,
            script: 0x99FFB3,
            sel_bg: 0x33FF66,
            sel_fg: 0x001400,
            bar_bg: 0x003300,
            bar_fg: 0x33FF66,
            menu_bg: 0x33FF66,
            menu_fg: 0x001400,
            hi_bg: 0x001400,
            hi_fg: 0xB3FFC6,
            outline: 0x124A1B,
        },
    ),
    (
        Theme::Contrast,
        ThemeSpec {
            name: "High contrast",
            bg: 0x000000,
            fg: 0xFFFFFF,
            bold: 0xFFFFFF,
            dim: 0xC8C8C8,
            border: 0xFFFF00,
            accent: 0x00FFFF,
            code: 0xFFFF00,
            script: 0x00FFFF,
            sel_bg: 0xFFFF00,
            sel_fg: 0x000000,
            bar_bg: 0xFFFFFF,
            bar_fg: 0x000000,
            menu_bg: 0xFFFFFF,
            menu_fg: 0x000000,
            hi_bg: 0x00FFFF,
            hi_fg: 0x000000,
            outline: 0x505050,
        },
    ),
];

/// Whether the terminal shows 24-bit colour; otherwise the nearest of the
/// standard 256 colours is used (which, unlike the first 16, every terminal
/// shows the same way).
fn truecolor() -> bool {
    static TC: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TC.get_or_init(|| {
        let ct = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_lowercase();
        let term = std::env::var("TERM").unwrap_or_default();
        ct.contains("truecolor")
            || ct.contains("24bit")
            || term.contains("direct")
            || std::env::var_os("KONSOLE_VERSION").is_some()
            || std::env::var_os("WT_SESSION").is_some()
            || term.contains("kitty")
            || term.contains("wezterm")
            || term.contains("alacritty")
    })
}

/// Nearest xterm 256-colour index.
fn to_256(c: u32) -> u8 {
    let (r, g, b) = (
        (c >> 16 & 255) as i32,
        (c >> 8 & 255) as i32,
        (c & 255) as i32,
    );
    let level = |v: i32| {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((v - 35) / 40).min(5)
        }
    };
    let steps = [0, 95, 135, 175, 215, 255];
    let (lr, lg, lb) = (level(r), level(g), level(b));
    let cube = 16 + 36 * lr + 6 * lg + lb;
    let cube_err = (r - steps[lr as usize]).pow(2)
        + (g - steps[lg as usize]).pow(2)
        + (b - steps[lb as usize]).pow(2);
    let avg = (r + g + b) / 3;
    let gi = ((avg - 8) / 10).clamp(0, 23);
    let gv = 8 + gi * 10;
    let gray_err = (r - gv).pow(2) + (g - gv).pow(2) + (b - gv).pow(2);
    if gray_err < cube_err {
        (232 + gi) as u8
    } else {
        cube as u8
    }
}

pub fn rgb(c: u32) -> Color {
    if truecolor() {
        Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
    } else {
        Color::Indexed(to_256(c))
    }
}

pub fn theme_spec(t: Theme) -> Option<&'static ThemeSpec> {
    THEMES.iter().find(|(k, _)| *k == t).map(|(_, s)| s)
}

fn palette(t: Theme) -> Palette {
    let Some(s) = theme_spec(t) else {
        // Terminal colours: the terminal's own background and text colours.
        let text = Style::default();
        let light_bg = std::env::var("COLORFGBG")
            .ok()
            .and_then(|v| v.rsplit(';').next()?.parse::<u8>().ok())
            .is_some_and(|b| b == 7 || b == 15);
        return Palette {
            text,
            outline: Style::default().add_modifier(Modifier::DIM),
            bg_rgb: if light_bg { 0xFFFFFF } else { 0x000000 },
            bold_fg: None,
            script_fg: Color::Cyan,
            select: Style::default().add_modifier(Modifier::REVERSED),
            status: Style::default().add_modifier(Modifier::REVERSED),
            status_strong: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            menu: Style::default().add_modifier(Modifier::REVERSED),
            menu_sel: Style::default().add_modifier(Modifier::BOLD),
            dialog: Style::default(),
            dialog_sel: Style::default().add_modifier(Modifier::REVERSED),
            dialog_title: Style::default().add_modifier(Modifier::BOLD),
            border: Style::default(),
            dim: Style::default().add_modifier(Modifier::DIM),
            fnref: Style::default().add_modifier(Modifier::BOLD),
            code: Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ruler: Style::default().add_modifier(Modifier::DIM),
            desk: Style::default().fg(rgb(0xC0C0C0)).bg(rgb(0x202020)),
            paper: Style::default().fg(rgb(0x000000)).bg(rgb(0xFFFFFF)),
        };
    };
    let text = Style::default().fg(rgb(s.fg)).bg(rgb(s.bg));
    Palette {
        text,
        outline: text.fg(rgb(s.outline)),
        bg_rgb: s.bg,
        bold_fg: Some(rgb(s.bold)),
        script_fg: rgb(s.script),
        select: Style::default().fg(rgb(s.sel_fg)).bg(rgb(s.sel_bg)),
        status: Style::default().fg(rgb(s.bar_fg)).bg(rgb(s.bar_bg)),
        status_strong: Style::default()
            .fg(rgb(s.bar_fg))
            .bg(rgb(s.bar_bg))
            .add_modifier(Modifier::BOLD),
        menu: Style::default().fg(rgb(s.menu_fg)).bg(rgb(s.menu_bg)),
        menu_sel: Style::default().fg(rgb(s.hi_fg)).bg(rgb(s.hi_bg)),
        dialog: Style::default().fg(rgb(s.menu_fg)).bg(rgb(s.menu_bg)),
        dialog_sel: Style::default().fg(rgb(s.hi_fg)).bg(rgb(s.hi_bg)),
        dialog_title: Style::default()
            .fg(rgb(s.menu_fg))
            .bg(rgb(s.menu_bg))
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        border: text.fg(rgb(s.border)),
        dim: text.fg(rgb(s.dim)),
        fnref: text.fg(rgb(s.accent)),
        code: text.fg(rgb(s.code)).add_modifier(Modifier::BOLD),
        ruler: text.fg(rgb(s.dim)),
        desk: Style::default().fg(rgb(s.dim)).bg(rgb(0x202020)),
        paper: Style::default().fg(rgb(0x000000)).bg(rgb(0xFFFFFF)),
    }
}

fn put_char(buf: &mut Buffer, x: u16, y: u16, ch: char, st: Style) {
    if let Some(c) = buf.cell_mut((x, y)) {
        c.set_char(ch);
        c.set_style(st);
    }
}

/// Write a string, clipped to `max_x`; returns the column after it.
fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, st: Style, max_x: u16) -> u16 {
    let mut x = x;
    let max_x = max_x.min(buf.area.right());
    if y >= buf.area.bottom() {
        return x;
    }
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
        if w == 0 {
            continue;
        }
        if x + w > max_x {
            break;
        }
        put_char(buf, x, y, ch, st);
        if w == 2
            && let Some(c) = buf.cell_mut((x + 1, y))
        {
            c.set_diff_option(CellDiffOption::Skip);
        }
        x += w;
    }
    x
}

fn fill(buf: &mut Buffer, r: Rect, st: Style) {
    let r = r.intersection(buf.area);
    for y in r.top()..r.bottom() {
        for x in r.left()..r.right() {
            put_char(buf, x, y, ' ', st);
        }
    }
}

fn boxed(buf: &mut Buffer, r: Rect, title: &str, st: Style, title_st: Style) {
    fill(buf, r, st);
    if r.width < 2 || r.height < 2 {
        return;
    }
    let (l, t, rr, b) = (r.left(), r.top(), r.right() - 1, r.bottom() - 1);
    for x in l + 1..rr {
        put_char(buf, x, t, '═', st);
        put_char(buf, x, b, '═', st);
    }
    for y in t + 1..b {
        put_char(buf, l, y, '║', st);
        put_char(buf, rr, y, '║', st);
    }
    put_char(buf, l, t, '╔', st);
    put_char(buf, rr, t, '╗', st);
    put_char(buf, l, b, '╚', st);
    put_char(buf, rr, b, '╝', st);
    if !title.is_empty() {
        let s = format!(" {title} ");
        let w = str_width(&s).min(r.width.saturating_sub(4));
        put(buf, l + (r.width - w) / 2, t, &s, title_st, rr);
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn glyph_style(g: &Glyph, selected: bool, p: &Palette) -> Style {
    let mut st = match g.kind {
        GKind::Border => p.border,
        GKind::Note => p.dim,
        GKind::FnRef | GKind::Object => p.fnref,
        GKind::Marker | GKind::Text => p.text,
    };
    let f = g.fmt;
    if f.has(Fmt::BOLD) {
        st = st.add_modifier(Modifier::BOLD);
        if let Some(c) = p.bold_fg {
            st = st.fg(c);
        }
    }
    if f.has(Fmt::ITALIC) {
        st = st.add_modifier(Modifier::ITALIC);
    }
    if f.has(Fmt::UNDERLINE) {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if f.has(Fmt::STRIKE) {
        st = st.add_modifier(Modifier::CROSSED_OUT);
    }
    if (f.has(Fmt::SUPER) || f.has(Fmt::SUB)) && g.kind == GKind::Text {
        st = st.fg(p.script_fg);
    }
    match (g.bg, g.color) {
        (Some(b), c) => {
            let fg = crate::props::readable(c.unwrap_or(0x000000), b);
            st = st.bg(rgb(b)).fg(rgb(fg));
        }
        (None, Some(c)) if g.kind == GKind::Text => {
            st = st.fg(rgb(crate::props::readable(c, p.bg_rgb)))
        }
        _ => {}
    }
    if selected {
        st = st.patch(p.select);
    }
    st
}

/// First item shown in a pull-down menu that is too tall for the screen.
pub fn menu_first_row(m: usize, item: usize, screen_h: u16) -> usize {
    let (_, y, _, h) = dropdown_rect(m);
    let rows = h.min(screen_h.saturating_sub(y)).saturating_sub(2) as usize;
    let n = MENUS[m].1.len();
    if n > rows && rows > 0 {
        (item + 1).saturating_sub(rows).min(n - rows)
    } else {
        0
    }
}

/// Location of a pull-down menu: (x, y, width, height).
pub fn dropdown_rect(m: usize) -> (u16, u16, u16, u16) {
    let items = MENUS[m].1;
    let w = items
        .iter()
        .flatten()
        .map(|c| c.label().len() + 16)
        .max()
        .unwrap_or(20) as u16
        + 2;
    (
        menu_title_x(m).saturating_sub(1),
        1,
        w,
        items.len() as u16 + 2,
    )
}

/// Columns between sheets in the side-by-side page view.
const PAGE_GAP: u16 = 3;

/// Where a layout line was drawn: screen row, the column of text column 0,
/// and the screen columns its sheet covers.
#[derive(Clone, Copy, Debug)]
pub struct LineSpot {
    pub li: usize,
    pub y: u16,
    pub ox: i32,
    pub lo: u16,
    pub hi: u16,
}

fn inches(v: f32) -> String {
    format!("{}\"", trim_num(v))
}

impl App {
    pub fn draw(&mut self, f: &mut Frame) {
        self.ensure_layout();
        let area = f.area();
        let p = palette(self.cfg.theme);
        let buf = f.buffer_mut();
        fill(buf, area, p.text);
        self.geo.width = area.width;
        self.geo.height = area.height;
        if area.height < 4 || area.width < 20 {
            put(buf, 0, 0, "Window too small", p.text, area.width);
            return;
        }

        match &self.mode {
            Mode::Help { scroll } => {
                let scroll = *scroll;
                self.draw_help(buf, area, scroll, &p);
                return;
            }
            Mode::Preview { .. } => {
                self.draw_preview(buf, area, &p);
                return;
            }
            _ => {}
        }

        let bar = self.cfg.keymap == Keymap::Modern || matches!(self.mode, Mode::Menu { .. });
        self.geo.menu_bar = bar;
        let top = u16::from(bar);
        let status_y = area.height - 1;
        let panel_h: u16 = if matches!(self.mode, Mode::Spell(_)) {
            8.min(area.height / 2)
        } else {
            0
        };
        let avail = area.height.saturating_sub(top + 1 + panel_h);
        let reveal_h = if self.reveal && avail >= 9 {
            avail / 3 + 1
        } else {
            0
        };
        let doc_h = avail - reveal_h;
        let doc = Rect::new(0, top, area.width, doc_h);
        let cursor = match self.page_columns(doc.width) {
            n if n >= 2 => self.draw_pages(buf, doc, &p, n),
            _ => self.draw_doc(buf, doc, &p),
        };
        if reveal_h > 0 {
            self.draw_reveal(buf, Rect::new(0, top + doc_h, area.width, reveal_h), &p);
        }
        if panel_h > 0 {
            self.draw_spell(
                buf,
                Rect::new(0, top + doc_h + reveal_h, area.width, panel_h),
                &p,
            );
        }
        let prompt_cursor = self.draw_status(buf, status_y, area.width, &p);
        if bar {
            self.draw_menu_bar(buf, area.width, &p);
        }
        let dialog_cursor = self.draw_dialogs(buf, area, &p);

        let cur = match &self.mode {
            Mode::Prompt(_) => prompt_cursor,
            Mode::Form(_) | Mode::Palette { .. } | Mode::FontPick(_) | Mode::HfEdit(_) => {
                dialog_cursor
            }
            Mode::Normal | Mode::Replace(_) => cursor,
            _ => None,
        };
        if let Some((x, y)) = cur {
            f.set_cursor_position(Position::new(x, y));
        }
    }

    fn draw_doc(&mut self, buf: &mut Buffer, area: Rect, p: &Palette) -> Option<(u16, u16)> {
        let tw = self.layout.width + 1;
        let (cl, cx) = self.layout.locate(self.ed.cur);
        let pg = &self.ed.doc.page;
        let (ml, mr) = (
            (pg.margin_left * CPI).round() as u16,
            (pg.margin_right * CPI).round() as u16,
        );
        let paper = ml + self.layout.width + mr;
        let outline = self.cfg.outline;
        let text_x = if outline && area.width >= paper + 2 {
            // Centre the whole sheet of paper.
            self.hscroll = 0;
            area.x + (area.width - paper - 2) / 2 + 1 + ml
        } else if area.width >= tw + 2 {
            self.hscroll = 0;
            area.x + (area.width - tw) / 2
        } else {
            if cx < self.hscroll {
                self.hscroll = cx.saturating_sub(8);
            } else if cx >= self.hscroll + area.width {
                self.hscroll = cx + 8 - area.width;
            }
            area.x
        };
        let h = area.height.max(1) as usize;
        let nlines = self.layout.lines.len();
        // With the outline, the top edge of the first sheet takes a row.
        let room = if outline && h > 3 { h - 1 } else { h };
        if self.follow {
            if cl < self.scroll {
                self.scroll = cl;
            } else if cl >= self.scroll + room {
                self.scroll = cl + 1 - room;
            }
        }
        self.scroll = self.scroll.min(nlines.saturating_sub(1));
        let top_pad = usize::from(outline && h > 3 && self.scroll == 0);
        self.geo.doc_y = area.y + top_pad as u16;
        self.geo.doc_h = area.height - top_pad as u16;
        self.geo.text_x = text_x;
        // Blank lines left on the last sheet, so its bottom edge is drawn
        // where the page really ends.
        let last_page = self.layout.lines.last().map(|l| l.page).unwrap_or(1);
        let used = self
            .layout
            .lines
            .iter()
            .filter(|l| {
                l.page == last_page
                    && !matches!(l.kind, LineKind::PageBreak { .. } | LineKind::SectionMark)
            })
            .count();
        let cap = self
            .ed
            .doc
            .page
            .body_lines()
            .saturating_sub(crate::layout::reserved_lines(&self.ed.doc));
        let remaining = cap.saturating_sub(used);

        let sel = self.ed.sel_range();
        self.screen_map.clear();
        // Paper edges (screen columns), when they are on screen.
        let left_edge = text_x as i32 - self.hscroll as i32 - ml as i32 - 1;
        let right_edge = text_x as i32 - self.hscroll as i32 + self.layout.width as i32 + mr as i32;
        let on_screen = |x: i32| x >= area.left() as i32 && x < area.right() as i32;
        let edge = |buf: &mut Buffer, x: i32, y: u16, ch: char| {
            if outline && on_screen(x) && buf.cell((x as u16, y)).is_some_and(|c| c.symbol() == " ")
            {
                put_char(buf, x as u16, y, ch, p.outline);
            }
        };
        let hline = |buf: &mut Buffer, y: u16, l: char, r: char| {
            for x in left_edge + 1..right_edge {
                if on_screen(x) {
                    put_char(buf, x as u16, y, '─', p.outline);
                }
            }
            edge(buf, left_edge, y, l);
            edge(buf, right_edge, y, r);
        };
        for row in 0..h {
            let y = area.y + row as u16;
            if row < top_pad {
                // Top of the first sheet.
                hline(buf, y, '╭', '╮');
                continue;
            }
            let li = self.scroll + row - top_pad;
            if li >= nlines {
                if !outline {
                    break;
                }
                // The rest of the last sheet, then its bottom edge.
                let k = li - nlines;
                if k < remaining {
                    edge(buf, left_edge, y, '│');
                    edge(buf, right_edge, y, '│');
                } else if k == remaining {
                    hline(buf, y, '╰', '╯');
                } else {
                    break;
                }
                continue;
            }
            edge(buf, left_edge, y, '│');
            edge(buf, right_edge, y, '│');
            self.screen_map.push(LineSpot {
                li,
                y,
                ox: text_x as i32 - self.hscroll as i32,
                lo: area.left(),
                hi: area.right(),
            });
            let line = &self.layout.lines[li];
            if outline && let LineKind::PageBreak { forced, section } = line.kind {
                // Between two sheets of paper.
                let ch = if forced { '═' } else { '─' };
                for x in left_edge..=right_edge {
                    if on_screen(x) {
                        put_char(buf, x as u16, y, ch, p.outline);
                    }
                }
                edge(buf, left_edge, y, ' ');
                if on_screen(left_edge) {
                    put_char(
                        buf,
                        left_edge as u16,
                        y,
                        if forced { '╞' } else { '├' },
                        p.outline,
                    );
                }
                if on_screen(right_edge) {
                    put_char(
                        buf,
                        right_edge as u16,
                        y,
                        if forced { '╡' } else { '┤' },
                        p.outline,
                    );
                }
                let label = format!(" Page {} ", line.page);
                let lx = (right_edge - 2 - str_width(&label) as i32).max(area.left() as i32) as u16;
                put(buf, lx, y, &label, p.dim, area.right());
                if section {
                    put(
                        buf,
                        text_x.saturating_sub(self.hscroll) + 2,
                        y,
                        " Section break ",
                        p.dim,
                        area.right(),
                    );
                }
                continue;
            }
            if line.kind == LineKind::SectionMark {
                for x in area.left()..area.right() {
                    put_char(buf, x, y, '┄', p.dim);
                }
                put(
                    buf,
                    text_x + 2,
                    y,
                    " Section break (continuous) ",
                    p.dim,
                    area.right(),
                );
                continue;
            }
            if let LineKind::PageBreak { forced, section } = line.kind {
                let ch = if forced { '═' } else { '─' };
                for x in area.left()..area.right() {
                    put_char(buf, x, y, ch, p.dim);
                }
                if section {
                    put(buf, text_x + 2, y, " Section break ", p.dim, area.right());
                }
                let label = format!(" Page {} ", line.page);
                let lx = area.right().saturating_sub(str_width(&label) + 2);
                put(buf, lx, y, &label, p.dim, area.right());
                continue;
            }
            let ox = text_x as i32 - self.hscroll as i32;
            self.draw_line(buf, li, y, ox, area, p, sel);
        }
        let content = Rect::new(
            area.x,
            area.y + top_pad as u16,
            area.width,
            area.height - top_pad as u16,
        );
        self.draw_pictures(buf, content, p);
        if cl >= self.scroll && cl + top_pad < self.scroll + h {
            let x = (cx.min(self.layout.width) as i32 - self.hscroll as i32 + text_x as i32)
                .clamp(area.left() as i32, area.right() as i32 - 1) as u16;
            Some((x, area.y + (cl - self.scroll + top_pad) as u16))
        } else {
            None
        }
    }

    /// Width of one sheet of paper on screen (with its edges) and its margins.
    fn sheet(&self) -> (u16, u16, u16) {
        let pg = &self.ed.doc.page;
        let (ml, mr) = (
            (pg.margin_left * CPI).round() as u16,
            (pg.margin_right * CPI).round() as u16,
        );
        (ml + self.layout.width + mr + 2, ml, mr)
    }

    /// How many pages fit side by side (1 = the normal continuous view).
    pub fn page_columns(&self, width: u16) -> usize {
        if !self.cfg.side_by_side || self.layout.pages < 2 {
            return 1;
        }
        let (w, _, _) = self.sheet();
        ((width + PAGE_GAP) / (w + PAGE_GAP))
            .max(1)
            .min(self.layout.pages as u16) as usize
    }

    /// Pages side by side, each on its own sheet, as in Word's multiple
    /// pages view. Rows of pages scroll vertically.
    fn draw_pages(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        p: &Palette,
        n: usize,
    ) -> Option<(u16, u16)> {
        let (sheet_w, ml, _) = self.sheet();
        let npages = self.layout.pages.max(1) as usize;
        let mut pages: Vec<Vec<usize>> = vec![Vec::new(); npages];
        for (li, l) in self.layout.lines.iter().enumerate() {
            if !matches!(l.kind, LineKind::PageBreak { .. }) {
                pages[(l.page.max(1) as usize - 1).min(npages - 1)].push(li);
            }
        }
        let cap = self
            .ed
            .doc
            .page
            .body_lines()
            .saturating_sub(crate::layout::reserved_lines(&self.ed.doc));
        // Each row of pages: top edge, the page lines, bottom edge, a gap.
        let nrows = npages.div_ceil(n);
        let inner: Vec<usize> = (0..nrows)
            .map(|r| {
                pages[r * n..((r + 1) * n).min(npages)]
                    .iter()
                    .map(Vec::len)
                    .max()
                    .unwrap_or(0)
                    .max(cap)
            })
            .collect();
        let mut ry = Vec::with_capacity(nrows + 1);
        let mut acc = 0usize;
        for h in &inner {
            ry.push(acc);
            acc += h + 3;
        }
        let total = acc;
        let h = area.height.max(1) as usize;
        // Keep the cursor's line on screen.
        let (cl, cx) = self.layout.locate(self.ed.cur);
        let cpage = (self
            .layout
            .lines
            .get(cl)
            .map(|l| l.page)
            .unwrap_or(1)
            .max(1) as usize
            - 1)
        .min(npages - 1);
        let k = pages[cpage].iter().position(|&li| li == cl).unwrap_or(0);
        let cy = ry[cpage / n] + 1 + k;
        if self.follow {
            if cy < self.scroll + 1 {
                self.scroll = cy.saturating_sub(1);
            } else if cy + 1 >= self.scroll + h {
                self.scroll = cy + 2 - h.min(cy + 2);
            }
        }
        self.scroll = self.scroll.min(total.saturating_sub(1));
        self.hscroll = 0;
        let total_w = n as u16 * sheet_w + (n as u16 - 1) * PAGE_GAP;
        let x0 = area.x + (area.width.saturating_sub(total_w)) / 2;
        self.geo.doc_y = area.y;
        self.geo.doc_h = area.height;
        self.geo.text_x = x0 + 1 + ml;
        self.screen_map.clear();
        let sel = self.ed.sel_range();
        let mut cursor = None;
        for row in 0..h {
            let vy = self.scroll + row;
            if vy >= total {
                break;
            }
            let y = area.y + row as u16;
            let r = ry.partition_point(|&t| t <= vy) - 1;
            let local = vy - ry[r];
            for c in 0..n {
                let pi = r * n + c;
                if pi >= npages {
                    break;
                }
                let left = x0 + c as u16 * (sheet_w + PAGE_GAP);
                let right = left + sheet_w - 1;
                let hline = |buf: &mut Buffer, l: char, rc: char| {
                    put_char(buf, left, y, l, p.outline);
                    for x in left + 1..right {
                        put_char(buf, x, y, '─', p.outline);
                    }
                    put_char(buf, right, y, rc, p.outline);
                };
                if local == 0 {
                    hline(buf, '╭', '╮');
                    let label = format!(" Page {} ", pi + 1);
                    put(
                        buf,
                        right.saturating_sub(str_width(&label) + 1),
                        y,
                        &label,
                        p.dim,
                        right,
                    );
                    continue;
                }
                if local == inner[r] + 1 {
                    hline(buf, '╰', '╯');
                    continue;
                }
                if local > inner[r] + 1 {
                    continue;
                }
                put_char(buf, left, y, '│', p.outline);
                put_char(buf, right, y, '│', p.outline);
                let Some(&li) = pages[pi].get(local - 1) else {
                    continue;
                };
                let ox = (left + 1 + ml) as i32;
                let sheet = Rect::new(left + 1, y, sheet_w - 2, 1);
                self.screen_map.push(LineSpot {
                    li,
                    y,
                    ox,
                    lo: left,
                    hi: right + 1,
                });
                if self.layout.lines[li].kind == LineKind::SectionMark {
                    for x in sheet.left()..sheet.right() {
                        put_char(buf, x, y, '┄', p.dim);
                    }
                    put(
                        buf,
                        ox as u16 + 2,
                        y,
                        " Section break (continuous) ",
                        p.dim,
                        sheet.right(),
                    );
                    continue;
                }
                self.draw_line(buf, li, y, ox, sheet, p, sel);
                if li == cl {
                    cursor = Some(((ox + cx.min(self.layout.width) as i32) as u16, y));
                }
            }
        }
        self.draw_pictures(buf, area, p);
        cursor
    }

    /// Draw layout line `li` on screen row `y`, with text column 0 at `ox`.
    #[allow(clippy::too_many_arguments)]
    fn draw_line(
        &self,
        buf: &mut Buffer,
        li: usize,
        y: u16,
        ox: i32,
        area: Rect,
        p: &Palette,
        sel: Option<(Pos, Pos)>,
    ) {
        for g in self.layout.glyphs(li, &self.ed.doc) {
            let sx = g.x as i32 + ox;
            if sx < area.left() as i32 || sx >= area.right() as i32 {
                continue;
            }
            let selected = match (sel, g.src) {
                (Some((a, b)), Some(pos)) => pos >= a && pos < b,
                _ => false,
            };
            let st = glyph_style(&g, selected, p);
            let w = UnicodeWidthChar::width(g.ch).unwrap_or(1) as u16;
            put_char(buf, sx as u16, y, g.ch, st);
            if w == 2
                && let Some(c) = buf.cell_mut((sx as u16 + 1, y))
            {
                c.set_diff_option(CellDiffOption::Skip);
            }
        }
    }

    /// Full-size pictures: with terminal graphics, blank their boxes and
    /// queue them to be drawn after the frame; otherwise shade the lines
    /// reserved above the placeholder.
    fn draw_pictures(&mut self, buf: &mut Buffer, area: Rect, p: &Palette) {
        let graphics = self.gfx.proto.is_some()
            && self.layout.opts.full_size_pictures
            && matches!(self.mode, Mode::Normal | Mode::Prompt(_) | Mode::Replace(_));
        self.gfx.pending.clear();
        let st = self.ed.doc.store.borrow();
        let spot = |li: usize| self.screen_map.iter().find(|s| s.li == li);
        for pb in &self.layout.pics {
            let top = (pb.line + 1).saturating_sub(pb.rows as usize);
            // Where the picture's line is on screen (in either view).
            let Some(at) = spot(pb.line) else { continue };
            let sx = pb.x as i32 + at.ox;
            let y0 = at.y as i32 - (pb.line - top) as i32;
            let fits = spot(top).is_some_and(|t| t.y as i32 == y0)
                && y0 >= area.top() as i32
                && sx >= at.lo as i32
                && sx + pb.cols as i32 <= at.hi as i32;
            let key = match st.obj(pb.obj) {
                Some(o) if graphics && fits => self.gfx.picture(&self.ed.doc, o),
                _ => None,
            };
            for r in 0..pb.rows as i32 {
                let y = y0 + r;
                let bottom = r + 1 == pb.rows as i32;
                if y < area.top() as i32 || y >= area.bottom() as i32 || (key.is_none() && bottom) {
                    continue;
                }
                for c in 0..pb.cols as i32 {
                    let x = sx + c;
                    if x >= area.left() as i32 && x < area.right() as i32 {
                        let (ch, s) = if key.is_some() {
                            (' ', p.text)
                        } else {
                            ('░', p.dim)
                        };
                        put_char(buf, x as u16, y as u16, ch, s);
                    }
                }
            }
            if let Some(key) = key {
                self.gfx.pending.push(Placement {
                    key,
                    x: sx as u16,
                    y: y0 as u16,
                    cols: pb.cols,
                    rows: pb.rows,
                });
            }
        }
    }

    fn draw_reveal(&self, buf: &mut Buffer, area: Rect, p: &Palette) {
        fill(buf, area, p.text);
        // Ruler with tab stops (classic style).
        let w = self.layout.width;
        let text_x = self.geo.text_x as i32 - self.hscroll as i32;
        for x in area.left()..area.right() {
            put_char(buf, x, area.y, '░', p.ruler);
        }
        for c in 0..=w {
            let sx = text_x + c as i32;
            if sx < area.left() as i32 || sx >= area.right() as i32 {
                continue;
            }
            let ch = if c == 0 {
                '['
            } else if c == w {
                ']'
            } else if c % 5 == 0 {
                '▲'
            } else {
                '─'
            };
            put_char(buf, sx as u16, area.y, ch, p.ruler);
        }

        let toks = reveal::tokens(&self.ed.doc, &self.layout, self.ed.cur, 6, 12);
        let width = area.width.saturating_sub(2).max(10);
        let mut lines: Vec<Vec<(String, Style)>> = vec![Vec::new()];
        let mut x = 0u16;
        let mut cursor_line = 0usize;
        let cur = self.ed.cur;
        for t in &toks {
            let (s, st, at_cursor) = match t {
                Tok::Ch(c, pos) => (c.to_string(), p.text, *pos == cur),
                Tok::Code(s, pos) => (s.clone(), p.code, *pos == Some(cur)),
                Tok::Break => {
                    lines.push(Vec::new());
                    x = 0;
                    continue;
                }
            };
            let sw = str_width(&s).max(1);
            if x + sw > width && x > 0 {
                lines.push(Vec::new());
                x = 0;
            }
            if at_cursor {
                cursor_line = lines.len() - 1;
            }
            let st = if at_cursor { st.patch(p.select) } else { st };
            lines.last_mut().unwrap().push((s, st));
            x += sw;
        }
        let rows = area.height.saturating_sub(1) as usize;
        let start = cursor_line
            .saturating_sub(rows / 2)
            .min(lines.len().saturating_sub(rows));
        for (i, line) in lines.iter().skip(start).take(rows).enumerate() {
            let y = area.y + 1 + i as u16;
            let mut x = area.x + 1;
            for (s, st) in line {
                x = put(buf, x, y, s, *st, area.right());
            }
        }
    }

    fn draw_status(
        &mut self,
        buf: &mut Buffer,
        y: u16,
        width: u16,
        p: &Palette,
    ) -> Option<(u16, u16)> {
        let row = Rect::new(0, y, width, 1);
        let classic = self.cfg.keymap == Keymap::Classic;
        fill(buf, row, p.status);
        match &self.mode {
            Mode::Prompt(pr) => {
                let x = put(buf, 0, y, &pr.label, p.status_strong, width);
                let avail = width.saturating_sub(x + 1) as usize;
                let skip = pr.cur.saturating_sub(avail);
                let shown: String = pr.input.iter().skip(skip).take(avail).collect();
                put(buf, x, y, &shown, p.status, width);
                let cx: u16 = pr.input[skip..pr.cur]
                    .iter()
                    .map(|c| UnicodeWidthChar::width(*c).unwrap_or(1) as u16)
                    .sum();
                return Some((x + cx, y));
            }
            Mode::Confirm { msg, .. } => {
                put(buf, 0, y, msg, p.status_strong, width);
                return None;
            }
            Mode::Replace(r) => {
                let s = format!(
                    "Replace \"{}\" with \"{}\"?  Y(es)  N(o)  A(ll)  Esc",
                    r.from, r.to
                );
                put(buf, 0, y, &s, p.status_strong, width);
                return None;
            }
            _ => {}
        }
        let (li, cx) = self.layout.locate(self.ed.cur);
        let (page, line_in_page) = self.layout.page_line(li);
        let (look, _) = self.cursor_look();
        let font = {
            let name = look
                .font
                .as_deref()
                .map(|f| self.ed.doc.font_name(f))
                .unwrap_or_default();
            let size = look
                .size
                .map(|h| format!(" {}pt", trim_num(h as f32 / 2.0)))
                .unwrap_or_default();
            format!("{name}{size}")
        };
        let left = if let Some(m) = &self.msg {
            m.clone()
        } else if self.pending_retrieve {
            "Move cursor; press Enter to retrieve.".into()
        } else if self.block_mode {
            "Block on".into()
        } else if classic {
            format!("{}   {font}", self.file_name())
        } else {
            String::new()
        };
        if classic {
            let pg = &self.ed.doc.page;
            let hdr =
                crate::print::hf_height(&crate::print::header_footer_rows(&self.ed.doc).0) as f32;
            let ln = pg.margin_top + (line_in_page as f32 + hdr) / LPI;
            let pos = pg.margin_left + cx as f32 / CPI;
            let trk = if self.ed.track { "TRK " } else { "" };
            let lhs = format!("{trk}Doc 1 Pg {page} Ln {} Pos ", inches(ln));
            let pos_s = inches(pos);
            let total = str_width(&lhs) + str_width(&pos_s) + 1;
            let rx = width.saturating_sub(total);
            put(
                buf,
                0,
                y,
                &left,
                if self.msg.is_some() {
                    p.status_strong
                } else {
                    p.status
                },
                rx.saturating_sub(1),
            );
            let x = put(buf, rx, y, &lhs, p.status, width);
            // As in classic word processors, the Pos number shows the active attributes.
            let mut g = Glyph::plain(0, ' ', GKind::Text);
            g.fmt = self.cursor_look().1;
            let st = glyph_style(&g, false, p)
                .patch(Style::default().bg(p.status.bg.unwrap_or(Color::Reset)));
            let st = if self.cfg.theme == Theme::Plain {
                st.add_modifier(Modifier::REVERSED)
            } else {
                st
            };
            put(buf, x, y, &pos_s, st, width);
        } else {
            let name = match &self.ed.path {
                Some(p) => p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                None => "Untitled".into(),
            };
            let x = put(
                buf,
                1,
                y,
                &format!("{name}{}", if self.ed.modified { " *" } else { "" }),
                p.status_strong,
                width,
            );
            let words = self.word_count();
            let right = format!(
                "{}{font}  Page {page}/{}  Ln {}  Col {}  {words} words  F10 Menu  F1 Help ",
                if self.ed.track { "TRACK  " } else { "" },
                self.layout.pages,
                line_in_page + 1,
                cx + 1
            );
            let rx = width.saturating_sub(str_width(&right));
            put(buf, x + 2, y, &left, p.status, rx.saturating_sub(1));
            put(buf, rx, y, &right, p.status, width);
        }
        None
    }

    fn draw_menu_bar(&self, buf: &mut Buffer, width: u16, p: &Palette) {
        fill(buf, Rect::new(0, 0, width, 1), p.menu);
        let active = match self.mode {
            Mode::Menu { m, .. } => Some(m),
            _ => None,
        };
        for (i, (name, _)) in MENUS.iter().enumerate() {
            let x = menu_title_x(i);
            let st = if active == Some(i) {
                p.menu_sel
            } else {
                p.menu
            };
            put(buf, x, 0, &format!(" {name} "), st, width);
            // Underline the menu's access letter.
            let key = commands::MENU_KEYS.get(i).copied().unwrap_or(' ');
            if let Some((ci, ch)) = name
                .char_indices()
                .find(|(_, c)| c.to_ascii_lowercase() == key)
            {
                put(
                    buf,
                    x + 1 + ci as u16,
                    0,
                    &ch.to_string(),
                    st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
                    width,
                );
            }
        }
        let tag = if self.cfg.keymap == Keymap::Classic {
            "Classic keys "
        } else {
            "WordBench "
        };
        let tx = width.saturating_sub(str_width(tag));
        if tx > menu_title_x(MENUS.len()) {
            put(buf, tx, 0, tag, p.menu, width);
        }
    }

    fn draw_dialogs(&self, buf: &mut Buffer, area: Rect, p: &Palette) -> Option<(u16, u16)> {
        match &self.mode {
            Mode::Menu { m, item } => {
                let (x, y, w, h) = dropdown_rect(*m);
                let r = Rect::new(x, y, w, h).intersection(area);
                boxed(buf, r, "", p.dialog, p.dialog_title);
                let keys = commands::item_keys(*m);
                // Scroll long menus so the selected item is visible.
                let rows = r.height.saturating_sub(2) as usize;
                let n = MENUS[*m].1.len();
                let first = menu_first_row(*m, *item, area.height);
                if first > 0 {
                    put(buf, x + w - 3, y, "▲", p.dialog_title, x + w);
                }
                if first + rows < n {
                    put(buf, x + w - 3, r.bottom() - 1, "▼", p.dialog_title, x + w);
                }
                for (i, it) in MENUS[*m].1.iter().enumerate().skip(first).take(rows) {
                    let yy = y + 1 + (i - first) as u16;
                    match it {
                        None => {
                            for xx in x + 1..x + w - 1 {
                                put_char(buf, xx, yy, '─', p.dialog);
                            }
                        }
                        Some(cmd) => {
                            let st = if i == *item { p.dialog_sel } else { p.dialog };
                            fill(buf, Rect::new(x + 1, yy, w - 2, 1), st);
                            put(buf, x + 2, yy, cmd.label(), st, x + w - 1);
                            if let Some(Some((ci, ch))) = keys.get(i) {
                                let shown: char = cmd.label().chars().nth(*ci).unwrap_or(*ch);
                                put(
                                    buf,
                                    x + 2 + *ci as u16,
                                    yy,
                                    &shown.to_string(),
                                    st.add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
                                    x + w - 1,
                                );
                            }
                            if let Some(k) = commands::key_for(self.cfg.keymap, *cmd) {
                                let kx = (x + w - 2).saturating_sub(str_width(&k));
                                put(buf, kx, yy, &k, st, x + w - 1);
                            }
                        }
                    }
                }
                None
            }
            Mode::Choice(c) => {
                let w = c
                    .items
                    .iter()
                    .map(|(l, _)| str_width(l))
                    .max()
                    .unwrap_or(10)
                    .max(str_width(&c.title) + 4)
                    + 6;
                let max_rows = area.height.saturating_sub(4).max(3) as usize;
                let rows = c.items.len().min(max_rows);
                let r = centered(area, w.min(area.width), rows as u16 + 2);
                boxed(buf, r, &c.title, p.dialog, p.dialog_title);
                let start = c
                    .sel
                    .saturating_sub(rows.saturating_sub(1))
                    .min(c.items.len().saturating_sub(rows));
                for (i, (label, _)) in c.items.iter().enumerate().skip(start).take(rows) {
                    let yy = r.y + 1 + (i - start) as u16;
                    let st = if i == c.sel { p.dialog_sel } else { p.dialog };
                    fill(
                        buf,
                        Rect::new(r.x + 1, yy, r.width.saturating_sub(2), 1),
                        st,
                    );
                    put(buf, r.x + 2, yy, label, st, r.right().saturating_sub(1));
                }
                None
            }
            Mode::Form(f) => self.draw_form(buf, area, f, p),
            Mode::HfEdit(h) => {
                let tw = self.layout.width;
                let w = (tw + 12).min(area.width);
                let rows = h.lines.len() as u16;
                let hgt = (rows * 2 + 9).min(area.height);
                let r = centered(area, w, hgt);
                boxed(
                    buf,
                    r,
                    if h.footer { "Footer" } else { "Header" },
                    p.dialog,
                    p.dialog_title,
                );
                let mut cursor = None;
                let tx = r.x + 7;
                let edge = r.right().saturating_sub(2);
                for (i, (t, a)) in h.lines.iter().enumerate() {
                    let y = r.y + 1 + i as u16;
                    if y >= r.bottom() - 1 {
                        break;
                    }
                    let tag = match a {
                        Align::Center => "[Ctr]",
                        Align::Right => "[Rgt]",
                        _ => "[Lft]",
                    };
                    let st = if i == h.row { p.dialog_sel } else { p.dialog };
                    fill(buf, Rect::new(r.x + 1, y, r.width - 2, 1), st);
                    put(buf, r.x + 1, y, tag, st, tx);
                    let s: String = t.iter().collect();
                    put(buf, tx, y, &s, st, edge);
                    if i == h.row {
                        cursor = Some(((tx + h.col.min(t.len()) as u16).min(edge), y));
                    }
                }
                // Preview of page 1, aligned across the page width.
                let py = r.y + 2 + rows;
                put(
                    buf,
                    r.x + 2,
                    py,
                    &format!("Preview (page 1 of {}):", self.layout.pages),
                    p.dialog_title,
                    edge,
                );
                let pages = self.layout.pages;
                for (i, (t, a)) in h.parts().iter().enumerate() {
                    let y = py + 1 + i as u16;
                    if y >= r.bottom().saturating_sub(4) {
                        break;
                    }
                    let s = crate::print::expand(t, 1, pages);
                    let sw = str_width(&s).min(tw);
                    let x = r.x
                        + 4
                        + match a {
                            Align::Center => (tw - sw) / 2,
                            Align::Right => tw - sw,
                            _ => 0,
                        };
                    put(buf, x, y, &s, p.dialog, edge);
                }
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 3,
                    "Tab alignment   Enter new line   Ctrl+P page no.   Ctrl+N pages",
                    p.dialog,
                    edge,
                );
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 2,
                    "Ctrl+D date   F10 save   Esc cancel",
                    p.dialog,
                    edge,
                );
                cursor
            }
            Mode::ThemePick { sel, .. } => {
                // On the right, so the document shows the preview.
                let n = Theme::ALL.len() as u16;
                let w = 44u16.min(area.width);
                let h = (n + 6).min(area.height);
                let r = Rect::new(area.right().saturating_sub(w + 1), area.y + 1, w, h);
                boxed(buf, r, "Color Theme", p.dialog, p.dialog_title);
                for (i, t) in Theme::ALL.iter().enumerate() {
                    let yy = r.y + 1 + i as u16;
                    if yy >= r.bottom().saturating_sub(4) {
                        break;
                    }
                    let st = if i == *sel { p.dialog_sel } else { p.dialog };
                    fill(buf, Rect::new(r.x + 1, yy, r.width - 2, 1), st);
                    put(
                        buf,
                        r.x + 2,
                        yy,
                        &format!("{} {}", i + 1, t.name()),
                        st,
                        r.right() - 8,
                    );
                    // A small sample of the theme: text, a border, a note.
                    let sx = r.right() - 7;
                    match theme_spec(*t) {
                        Some(ts) => {
                            let base = Style::default().bg(rgb(ts.bg));
                            put(buf, sx, yy, " Aa", base.fg(rgb(ts.fg)), r.right() - 1);
                            put(buf, sx + 3, yy, "│", base.fg(rgb(ts.border)), r.right() - 1);
                            put(
                                buf,
                                sx + 4,
                                yy,
                                "1 ",
                                base.fg(rgb(ts.accent)),
                                r.right() - 1,
                            );
                        }
                        None => {
                            put(buf, sx, yy, " Aa│1 ", Style::default(), r.right() - 1);
                        }
                    }
                }
                let outline = if self.cfg.outline { "On" } else { "Off" };
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 4,
                    &format!("O  Paper outline: {outline}"),
                    p.dialog,
                    r.right() - 1,
                );
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 3,
                    "Up/Down previews each theme",
                    p.dialog,
                    r.right() - 1,
                );
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 2,
                    "Enter keeps it   Esc goes back",
                    p.dialog,
                    r.right() - 1,
                );
                None
            }
            Mode::FontPick(fp) => {
                self.draw_form(buf, area, &fp.form, p);
                let items = fp.items();
                let w = 56u16.min(area.width);
                let r = centered(area, w, area.height.saturating_sub(4).max(8));
                boxed(buf, r, "Choose a font", p.dialog, p.dialog_title);
                let px = put(
                    buf,
                    r.x + 2,
                    r.y + 1,
                    "Search: ",
                    p.dialog_title,
                    r.right() - 1,
                );
                let qx = put(buf, px, r.y + 1, &fp.query, p.dialog, r.right() - 1);
                let rows = r.height.saturating_sub(4) as usize;
                let first = (fp.sel + 1).saturating_sub(rows);
                for (i, it) in items.iter().enumerate().skip(first).take(rows) {
                    let yy = r.y + 2 + (i - first) as u16;
                    let st = if i == fp.sel { p.dialog_sel } else { p.dialog };
                    fill(buf, Rect::new(r.x + 1, yy, r.width - 2, 1), st);
                    let (label, note) = match it {
                        None => ("(Default font of the style)".to_string(), String::new()),
                        Some(name) => {
                            let in_doc = fp.fonts[..fp.in_doc].iter().any(|f| f == *name);
                            let note = match (fp.subs.get(*name), in_doc) {
                                (Some(sub), _) => format!("shows as {sub}"),
                                (None, true) => "in document".to_string(),
                                _ => String::new(),
                            };
                            ((*name).clone(), note)
                        }
                    };
                    put(buf, r.x + 2, yy, &label, st, r.right() - 1);
                    if !note.is_empty() {
                        let nst = if i == fp.sel {
                            st
                        } else {
                            p.dialog.add_modifier(Modifier::DIM)
                        };
                        put(
                            buf,
                            (r.right() - 2).saturating_sub(str_width(&note)),
                            yy,
                            &note,
                            nst,
                            r.right() - 1,
                        );
                    }
                }
                if items.is_empty() {
                    put(
                        buf,
                        r.x + 2,
                        r.y + 2,
                        "No installed font matches; Enter uses the name as typed",
                        p.dialog,
                        r.right() - 1,
                    );
                }
                let foot = format!(
                    "{} fonts · type to search (misspellings OK) · Enter choose · Esc back",
                    fp.fonts.len()
                );
                put(buf, r.x + 2, r.bottom() - 2, &foot, p.dialog, r.right() - 1);
                Some((qx, r.y + 1))
            }
            Mode::Palette { query, sel } => {
                let items = self.palette_items(query);
                let w = 70u16.min(area.width);
                let rows = (area.height.saturating_sub(7) as usize).max(3);
                let r = centered(area, w, (rows as u16 + 5).min(area.height));
                boxed(buf, r, "All commands", p.dialog, p.dialog_title);
                let prompt = "Search: ";
                let px = put(buf, r.x + 2, r.y + 1, prompt, p.dialog_title, r.right() - 1);
                let qx = put(buf, px, r.y + 1, query, p.dialog, r.right() - 1);
                let list_rows = r.height.saturating_sub(4) as usize;
                let first = (*sel + 1).saturating_sub(list_rows);
                for (i, (cmd, menu)) in items.iter().enumerate().skip(first).take(list_rows) {
                    let yy = r.y + 2 + (i - first) as u16;
                    let st = if i == *sel { p.dialog_sel } else { p.dialog };
                    fill(buf, Rect::new(r.x + 1, yy, r.width - 2, 1), st);
                    let label = if menu.is_empty() {
                        cmd.label().to_string()
                    } else {
                        format!("{}  ({menu})", cmd.label())
                    };
                    put(buf, r.x + 2, yy, &label, st, r.right() - 1);
                    if let Some(k) = commands::key_for(self.cfg.keymap, *cmd) {
                        let kx = (r.right() - 2).saturating_sub(str_width(&k));
                        put(buf, kx, yy, &k, st, r.right() - 1);
                    }
                }
                if items.is_empty() {
                    put(
                        buf,
                        r.x + 2,
                        r.y + 2,
                        "No matching command",
                        p.dialog,
                        r.right() - 1,
                    );
                }
                put(
                    buf,
                    r.x + 2,
                    r.bottom() - 2,
                    "Type to search, Up/Down choose, Enter run, Esc close",
                    p.dialog,
                    r.right() - 1,
                );
                Some((qx, r.y + 1))
            }
            Mode::Thes(t) => {
                let w = t
                    .rows
                    .iter()
                    .map(|(_, s)| str_width(s))
                    .max()
                    .unwrap_or(10)
                    .max(30)
                    + 6;
                let max_rows = area.height.saturating_sub(6).max(3) as usize;
                let rows = t.rows.len().min(max_rows);
                let r = Rect::new(
                    area.width.saturating_sub(w + 2),
                    2,
                    w.min(area.width),
                    rows as u16 + 3,
                );
                boxed(
                    buf,
                    r,
                    &format!("Thesaurus: {}", t.word),
                    p.dialog,
                    p.dialog_title,
                );
                let start = t
                    .sel
                    .saturating_sub(rows.saturating_sub(1))
                    .min(t.rows.len().saturating_sub(rows));
                for (i, (head, s)) in t.rows.iter().enumerate().skip(start).take(rows) {
                    let yy = r.y + 1 + (i - start) as u16;
                    let st = if i == t.sel {
                        p.dialog_sel
                    } else if *head {
                        p.dialog_title
                    } else {
                        p.dialog
                    };
                    let text = if *head { s.clone() } else { format!("  {s}") };
                    fill(
                        buf,
                        Rect::new(r.x + 1, yy, r.width.saturating_sub(2), 1),
                        st,
                    );
                    put(buf, r.x + 2, yy, &text, st, r.right().saturating_sub(1));
                }
                put(
                    buf,
                    r.x + 2,
                    r.bottom().saturating_sub(2),
                    "Enter replace  L look up  Esc",
                    p.dialog,
                    r.right().saturating_sub(1),
                );
                None
            }
            _ => None,
        }
    }

    fn draw_form(&self, buf: &mut Buffer, area: Rect, f: &Form, p: &Palette) -> Option<(u16, u16)> {
        let w = 72u16.min(area.width);
        let h = (f.fields.len() as u16 + 4).min(area.height);
        let r = centered(area, w, h);
        boxed(buf, r, &f.title, p.dialog, p.dialog_title);
        let mut n = 0usize;
        let mut cursor = None;
        let label_w = 30u16;
        for (i, fld) in f.fields.iter().enumerate() {
            let yy = r.y + 1 + i as u16;
            if yy >= r.bottom().saturating_sub(2) {
                break;
            }
            if matches!(fld.val, FVal::Heading) {
                let label = if f.act == FormAct::Caption && i == 0 {
                    format!("Preview:  {}", self.caption_preview(f))
                } else {
                    fld.label.clone()
                };
                put(buf, r.x + 2, yy, &label, p.dialog_title, r.right() - 1);
                continue;
            }
            let key = if n < 9 {
                char::from(b'1' + n as u8)
            } else {
                char::from(b'a' + (n - 9) as u8)
            };
            n += 1;
            let st = if i == f.sel { p.dialog_sel } else { p.dialog };
            fill(buf, Rect::new(r.x + 1, yy, r.width - 2, 1), st);
            put(
                buf,
                r.x + 3,
                yy,
                &format!("{key} - {}", fld.label),
                st,
                r.right() - 1,
            );
            let vx = r.x + 3 + label_w;
            match (&f.edit, i == f.sel) {
                (Some((b, c)), true) => {
                    let s: String = b.iter().collect();
                    put(
                        buf,
                        vx,
                        yy,
                        &s,
                        st.add_modifier(Modifier::UNDERLINED),
                        r.right() - 1,
                    );
                    cursor = Some((
                        vx + b[..*c]
                            .iter()
                            .map(|ch| UnicodeWidthChar::width(*ch).unwrap_or(1) as u16)
                            .sum::<u16>(),
                        yy,
                    ));
                }
                _ => {
                    put(buf, vx, yy, &fld.display(), st, r.right() - 1);
                }
            }
        }
        let hint = if f.edit.is_some() {
            "Type a value, Enter to set, Esc to undo"
        } else {
            "Number/arrows select, Enter/Space change, F7 or F10 save, Esc cancel"
        };
        put(buf, r.x + 2, r.bottom() - 2, hint, p.dialog, r.right() - 1);
        cursor
    }

    fn draw_spell(&self, buf: &mut Buffer, area: Rect, p: &Palette) {
        let Mode::Spell(st) = &self.mode else { return };
        fill(buf, area, p.text);
        for x in area.left()..area.right() {
            put_char(buf, x, area.y, '═', p.ruler);
        }
        let cols = 4usize;
        let colw = (area.width / cols as u16).max(10);
        let rows = area.height.saturating_sub(3) as usize;
        if st.sugg.is_empty() {
            put(buf, 2, area.y + 1, "(no suggestions)", p.dim, area.right());
        }
        for (i, s) in st.sugg.iter().take(26.min(rows * cols)).enumerate() {
            let x = 2 + (i % cols) as u16 * colw;
            let y = area.y + 1 + (i / cols) as u16;
            let label = format!("{}. {s}", (b'a' + i as u8) as char);
            put(buf, x, y, &label, p.text, (x + colw).min(area.right()));
        }
        let msg = format!(
            "Not Found: \"{}\"   1 Skip Once  2 Skip  3 Add  4 Edit  Esc Exit",
            st.word
        );
        put(
            buf,
            1,
            area.bottom() - 1,
            &msg,
            p.status_strong,
            area.right(),
        );
    }

    fn draw_help(&self, buf: &mut Buffer, area: Rect, scroll: usize, p: &Palette) {
        boxed(buf, area, "Help", p.dialog, p.dialog_title);
        let lines = self.help_lines();
        let rows = area.height.saturating_sub(3) as usize;
        let scroll = scroll.min(lines.len().saturating_sub(rows));
        for (i, l) in lines.iter().skip(scroll).take(rows).enumerate() {
            let st = if !l.starts_with(' ') && !l.is_empty() {
                p.dialog_title
            } else {
                p.dialog
            };
            put(buf, 2, 1 + i as u16, l, st, area.right() - 1);
        }
        put(
            buf,
            2,
            area.bottom() - 2,
            "Up/Down/PgUp/PgDn scroll, any other key closes",
            p.dialog,
            area.right() - 1,
        );
    }

    fn draw_preview(&self, buf: &mut Buffer, area: Rect, p: &Palette) {
        let Mode::Preview {
            page,
            scroll,
            hscroll,
            pages,
        } = &self.mode
        else {
            return;
        };
        fill(buf, area, p.desk);
        let pg = &self.ed.doc.page;
        let pw = (pg.width * CPI).round() as i32;
        let ph = (pg.height * LPI).round() as i32;
        let view_h = area.height.saturating_sub(1) as i32;
        let x0 = ((area.width as i32 - pw) / 2).max(0) - *hscroll as i32;
        let y0 = 1 - *scroll as i32;
        let ml = (pg.margin_left * CPI).round() as i32;
        let mt = (pg.margin_top * LPI).round() as i32;
        for row in 0..ph {
            let y = y0 + row;
            if y < 0 || y >= view_h {
                continue;
            }
            for col in 0..pw {
                let x = x0 + col;
                if x >= 0 && x < area.width as i32 {
                    put_char(buf, x as u16, y as u16, ' ', p.paper);
                }
            }
        }
        if let Some(pp) = pages.get(*page) {
            // Lines reserved for pictures, above their placeholder.
            for (top, pb) in &pp.pics {
                for r in *top..top + pb.rows as usize - 1 {
                    let y = y0 + mt + r as i32;
                    for c in 0..pb.cols as i32 {
                        let x = x0 + ml + pb.x as i32 + c;
                        if y >= 0 && y < view_h && x >= 0 && x < area.width as i32 {
                            put_char(buf, x as u16, y as u16, '░', p.paper.fg(rgb(0x606060)));
                        }
                    }
                }
            }
            for (r, glyphs) in pp.rows.iter().enumerate() {
                let y = y0 + mt + r as i32;
                if y < 0 || y >= view_h {
                    continue;
                }
                for g in glyphs {
                    let x = x0 + ml + g.x as i32;
                    if x < 0 || x >= area.width as i32 {
                        continue;
                    }
                    let mut st = p.paper;
                    if g.fmt.has(Fmt::BOLD) {
                        st = st.add_modifier(Modifier::BOLD);
                    }
                    if g.fmt.has(Fmt::ITALIC) {
                        st = st.add_modifier(Modifier::ITALIC);
                    }
                    if g.fmt.has(Fmt::UNDERLINE) {
                        st = st.add_modifier(Modifier::UNDERLINED);
                    }
                    if g.fmt.has(Fmt::STRIKE) {
                        st = st.add_modifier(Modifier::CROSSED_OUT);
                    }
                    if g.kind == GKind::Border || g.kind == GKind::Note {
                        st = st.fg(rgb(0x606060));
                    }
                    put_char(buf, x as u16, y as u16, g.ch, st);
                }
            }
        }
        let status = format!(
            " Print preview: page {} of {}   PgUp/PgDn page   Arrows scroll   Esc close ",
            page + 1,
            pages.len()
        );
        let y = area.bottom() - 1;
        fill(buf, Rect::new(0, y, area.width, 1), p.menu);
        put(buf, 0, y, &status, p.menu, area.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::{contrast, readable};

    #[test]
    fn every_theme_keeps_everything_readable() {
        for (_, t) in THEMES {
            let check = |what: &str, fg: u32, bg: u32, min: f32| {
                let c = contrast(fg, bg);
                assert!(
                    c >= min,
                    "{}: {what} contrast {c:.2} < {min} ({fg:06X} on {bg:06X})",
                    t.name
                );
            };
            check("text", t.fg, t.bg, 4.5);
            check("bold", t.bold, t.bg, 4.5);
            check("table borders", t.border, t.bg, 3.0);
            check("notes and page breaks", t.dim, t.bg, 3.0);
            check("footnote numbers and pictures", t.accent, t.bg, 3.0);
            check("reveal codes", t.code, t.bg, 3.0);
            check("super/subscript", t.script, t.bg, 3.0);
            check("selection", t.sel_fg, t.sel_bg, 4.5);
            check("status line", t.bar_fg, t.bar_bg, 4.5);
            check("menus and dialogs", t.menu_fg, t.menu_bg, 4.5);
            check("highlighted item", t.hi_fg, t.hi_bg, 4.5);
            // Colours documents use: captions, links, tracked changes, greys,
            // black and white text.
            for doc in [
                0x44546A, 0x0563C1, 0x2E9E3E, 0xC00000, 0x808080, 0x000000, 0xFFFFFF, 0xFFFF00,
                0x002060, 0x7030A0,
            ] {
                check(
                    &format!("document colour {doc:06X}"),
                    readable(doc, t.bg),
                    t.bg,
                    3.0,
                );
            }
        }
    }

    #[test]
    fn colour_fallback_is_close() {
        assert_eq!(to_256(0x000000), 16);
        assert_eq!(to_256(0xFFFFFF), 231);
        assert_eq!(to_256(0x0000AA), 19);
        assert_eq!(to_256(0x808080), 244);
    }
}

#[cfg(test)]
mod outline_tests {
    use super::*;
    use crate::props::contrast;

    #[test]
    fn paper_outline_is_faint_but_visible() {
        for (t, s) in THEMES {
            let c = contrast(s.outline, s.bg);
            let max = if *t == Theme::Contrast { 3.0 } else { 2.2 };
            assert!(
                (1.4..=max).contains(&c),
                "{}: outline contrast {c:.2}",
                s.name
            );
            // Fainter than anything else on the page.
            assert!(
                c < contrast(s.dim, s.bg) && c < contrast(s.border, s.bg),
                "{}",
                s.name
            );
        }
    }
}
