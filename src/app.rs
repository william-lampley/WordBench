//! Application state and input handling: modes (menus, prompts, dialogs),
//! command dispatch, file operations, spell check and thesaurus flows.

use crate::commands::{self, Cmd, Keymap, MENUS};
use crate::editor::{Editor, FindResult};
use crate::io;
use crate::layout::Layout;
use crate::model::*;
use crate::print::{PrintPage, compose};
use crate::tools::{Speller, Thesaurus, config_dir};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Blue,
    Dark,
    Light,
    Solarized,
    Amber,
    Green,
    Contrast,
    /// The terminal's own colours.
    Plain,
}

impl Theme {
    pub const ALL: [Theme; 8] = [
        Theme::Blue,
        Theme::Dark,
        Theme::Light,
        Theme::Solarized,
        Theme::Amber,
        Theme::Green,
        Theme::Contrast,
        Theme::Plain,
    ];
    /// Name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Theme::Blue => "blue",
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Solarized => "solarized",
            Theme::Amber => "amber",
            Theme::Green => "green",
            Theme::Contrast => "contrast",
            Theme::Plain => "plain",
        }
    }
    pub fn name(self) -> &'static str {
        crate::ui::theme_spec(self)
            .map(|s| s.name)
            .unwrap_or("Terminal colors")
    }
    pub fn from_key(k: &str) -> Theme {
        Theme::ALL
            .into_iter()
            .find(|t| t.key() == k)
            .unwrap_or(Theme::Blue)
    }
}

pub struct Config {
    pub keymap: Keymap,
    pub theme: Theme,
    pub lang: String,
    /// Name used for comments and tracked changes.
    pub author: String,
    /// Pictures at full size (drawn with terminal graphics when possible),
    /// or one-line placeholders.
    pub pictures: bool,
    /// Faint outline of the sheet of paper around the text.
    pub outline: bool,
    /// Show pages side by side when the window is wide enough.
    pub side_by_side: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            keymap: Keymap::Classic,
            theme: Theme::Blue,
            lang: "en_US".into(),
            author: crate::platform::user_name(),
            pictures: true,
            outline: true,
            side_by_side: true,
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let mut c = Config::default();
        let text = std::fs::read_to_string(config_dir().join("config")).unwrap_or_default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match (k.trim(), v.trim()) {
                ("keymap", "modern") => c.keymap = Keymap::Modern,
                ("keymap", _) => c.keymap = Keymap::Classic,
                ("theme", t) => c.theme = Theme::from_key(t),
                ("lang", v) if !v.is_empty() => c.lang = v.to_string(),
                ("author", v) if !v.is_empty() => c.author = v.to_string(),
                ("pictures", v) => c.pictures = v != "placeholders",
                ("outline", v) => c.outline = v != "off",
                ("pages", v) => c.side_by_side = v != "single",
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) -> Result<(), String> {
        if cfg!(test) {
            return Ok(()); // never touch the real settings file from tests
        }
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let s = format!(
            "# WordBench settings\nkeymap={}\ntheme={}\nlang={}\nauthor={}\npictures={}\noutline={}\npages={}\n",
            if self.keymap == Keymap::Modern {
                "modern"
            } else {
                "classic"
            },
            self.theme.key(),
            self.lang,
            self.author,
            if self.pictures {
                "full"
            } else {
                "placeholders"
            },
            if self.outline { "on" } else { "off" },
            if self.side_by_side {
                "side-by-side"
            } else {
                "single"
            }
        );
        std::fs::write(dir.join("config"), s).map_err(|e| e.to_string())
    }
}

/// What to do once a save (or a "discard changes?" question) is settled.
#[derive(Clone, Debug)]
pub enum After {
    Nothing,
    Quit,
    ExitAsk,
    New,
    Open(PathBuf),
}

pub enum PromptAct {
    Open,
    SaveAs(After),
    ExportPdf,
    Find(bool),
    ReplaceFrom,
    ReplaceTo(String),
    FootnoteNew,
    FootnoteEdit(u32),
    TableCols,
    TableRows(usize),
    SpellEdit(ParaRef, usize, usize),
    Link,
    CommentNew,
    CommentEdit(String),
    Picture,
    EndnoteNew,
}

pub struct Prompt {
    pub label: String,
    pub input: Vec<char>,
    pub cur: usize,
    pub act: PromptAct,
}

pub enum ConfirmAct {
    SaveFirst(After),
    ExitWp,
}

#[derive(Clone)]
pub enum ChoiceAct {
    Cmd(Cmd),
    Path(PathBuf),
    MoveCut,
    MoveCopy,
    MoveDelete,
    Style(String),
    GoTo(Pos),
    /// Save to this path with the chosen extension, then continue.
    SaveAsType(PathBuf, &'static str, After),
}

pub struct Choice {
    pub title: String,
    pub items: Vec<(String, ChoiceAct)>,
    pub sel: usize,
    pub numbered: bool,
}

pub enum FVal {
    Heading,
    Choice(Vec<String>, usize),
    Text(String),
    Num(f32),
}

pub struct Field {
    pub label: String,
    pub key: &'static str,
    pub val: FVal,
    /// The value shown when the dialog opened (to apply only changes).
    pub orig: String,
}

impl Field {
    pub fn new(label: impl Into<String>, key: &'static str, val: FVal) -> Field {
        let mut f = Field {
            label: label.into(),
            key,
            val,
            orig: String::new(),
        };
        f.orig = f.display();
        f
    }

    pub fn changed(&self) -> bool {
        self.display() != self.orig
    }

    pub fn display(&self) -> String {
        match &self.val {
            FVal::Heading => String::new(),
            FVal::Choice(opts, i) => opts.get(*i).cloned().unwrap_or_default(),
            FVal::Text(s) => s.clone(),
            FVal::Num(v) => format!("{}\"", trim_num(*v)),
        }
    }
}

/// Word's built-in style names are lower case ("heading 1").
pub fn display_style_name(name: &str) -> String {
    let mut c = name.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

pub fn trim_num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FormAct {
    Format,
    Settings,
    Font,
    Props,
    Caption,
}

pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub sel: usize,
    pub edit: Option<(Vec<char>, usize)>,
    pub act: FormAct,
}

pub struct FontPick {
    pub form: Box<Form>,
    pub query: String,
    pub sel: usize,
    /// Fonts used in the document come first.
    pub fonts: Vec<String>,
    pub in_doc: usize,
    /// Fonts that aren't installed, with the font shown in their place.
    pub subs: std::collections::HashMap<String, String>,
}

impl FontPick {
    /// Matching entries; `None` stands for "default font".
    pub fn items(&self) -> Vec<Option<&String>> {
        let mut v: Vec<Option<&String>> = Vec::new();
        if self.query.trim().is_empty() {
            v.push(None);
        }
        v.extend(
            crate::props::search_fonts(&self.fonts, &self.query)
                .into_iter()
                .map(Some),
        );
        v
    }
}

/// The header or footer editor: several lines, each with an alignment.
pub struct HfEdit {
    pub footer: bool,
    pub lines: Vec<(Vec<char>, Align)>,
    pub row: usize,
    pub col: usize,
    /// The Format dialog it was opened from (returned to afterwards).
    pub back: Option<Box<Form>>,
}

impl HfEdit {
    pub fn parts(&self) -> Vec<(String, Align)> {
        self.lines
            .iter()
            .map(|(t, a)| (t.iter().collect(), *a))
            .collect()
    }
}

/// One-line description of a header or footer for the Format dialog.
pub fn hf_summary(s: &str) -> String {
    let lines = crate::print::hf_lines(s);
    match lines.len() {
        0 => "(none)".into(),
        1 => lines[0].0.clone(),
        n => format!(
            "{}  (+{} more line{})",
            lines[0].0,
            n - 1,
            if n == 2 { "" } else { "s" }
        ),
    }
}

pub struct SpellSt {
    pub pr: ParaRef,
    pub s: usize,
    pub e: usize,
    pub word: String,
    pub sugg: Vec<String>,
}

pub struct ThesSt {
    pub pr: ParaRef,
    pub s: usize,
    pub e: usize,
    pub word: String,
    /// (is_heading, text)
    pub rows: Vec<(bool, String)>,
    pub sel: usize,
}

pub struct ReplaceSt {
    pub from: String,
    pub to: String,
    pub count: usize,
}

pub enum Mode {
    Normal,
    Menu {
        m: usize,
        item: usize,
    },
    Prompt(Prompt),
    Confirm {
        msg: String,
        act: ConfirmAct,
    },
    Choice(Choice),
    Form(Form),
    Spell(SpellSt),
    Thes(ThesSt),
    Replace(ReplaceSt),
    Help {
        scroll: usize,
    },
    /// Searchable list of every command.
    Palette {
        query: String,
        sel: usize,
    },
    /// Colour themes: moving through the list previews each one; Enter
    /// keeps it, Esc goes back to the theme (and outline) from before.
    ThemePick {
        sel: usize,
        original: Theme,
        original_outline: bool,
    },
    /// Header or footer editor.
    HfEdit(HfEdit),
    /// Font list opened from the Font dialog (which it returns to).
    FontPick(FontPick),
    Preview {
        page: usize,
        scroll: u16,
        hscroll: u16,
        pages: Vec<PrintPage>,
    },
}

/// Screen geometry from the last frame (for mouse hit testing).
#[derive(Default, Clone, Copy)]
pub struct Geo {
    pub doc_y: u16,
    pub doc_h: u16,
    pub text_x: u16,
    pub width: u16,
    pub height: u16,
    pub menu_bar: bool,
}

pub struct App {
    pub ed: Editor,
    pub layout: Layout,
    layout_ver: u64,
    pub cfg: Config,
    pub mode: Mode,
    pub msg: Option<String>,
    pub scroll: usize,
    pub hscroll: u16,
    pub follow: bool,
    pub reveal: bool,
    pub block_mode: bool,
    home_prefix: u8,
    pub pending_retrieve: bool,
    last_search: String,
    last_replace: String,
    speller: Option<Speller>,
    thesaurus: Option<Thesaurus>,
    spell_checked: usize,
    pub quit: bool,
    mouse_down: bool,
    pub geo: Geo,
    words_cache: (u64, usize),
    installed_fonts: Option<Vec<String>>,
    font_subs: Option<std::collections::HashMap<String, String>>,
    /// Where each visible line was drawn (for the mouse and pictures).
    pub screen_map: Vec<crate::ui::LineSpot>,
    /// Copy to the terminal's clipboard with OSC 52 (off in tests).
    pub osc52: bool,
    /// The desktop clipboard (None in tests).
    pub sysclip: Option<crate::clip::SysClip>,
    /// Pictures drawn with terminal graphics (none in tests).
    pub gfx: crate::graphics::Graphics,
}

pub fn menu_title_x(i: usize) -> u16 {
    1 + MENUS[..i]
        .iter()
        .map(|(n, _)| n.len() as u16 + 2)
        .sum::<u16>()
}

pub fn first_item(m: usize) -> usize {
    MENUS[m].1.iter().position(Option::is_some).unwrap_or(0)
}

fn expand_path(s: &str) -> PathBuf {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\"))
        && let Some(home) = crate::platform::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(s)
}

/// Caption numbering: (shown, Word SEQ format switch, list number format).
pub const CAPTION_FORMATS: [(&str, &str, &str); 5] = [
    ("1, 2, 3", "ARABIC", "decimal"),
    ("a, b, c", "alphabetic", "lowerLetter"),
    ("A, B, C", "ALPHABETIC", "upperLetter"),
    ("i, ii, iii", "roman", "lowerRoman"),
    ("I, II, III", "ROMAN", "upperRoman"),
];

/// File types offered when saving without an extension.
pub const SAVE_TYPES: [(&str, &str); 5] = [
    ("docx", "Word document"),
    ("wbd", "WordBench document"),
    ("txt", "Plain text (no formatting)"),
    ("md", "Markdown text (no formatting)"),
    ("pdf", "PDF (export a copy)"),
];

fn with_ext(p: PathBuf, ext: &str) -> PathBuf {
    if p.extension().is_some() {
        p
    } else {
        p.with_extension(ext)
    }
}

pub fn today() -> String {
    if let Some(d) = crate::platform::local_date() {
        return d;
    }
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0) as i64;
    // Civil-from-days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    format!("{} {d}, {y}", MONTHS[(m - 1) as usize])
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn spell_word_char(c: char) -> bool {
    c.is_alphabetic() || c == '\'' || c == '’'
}

impl App {
    pub fn new(mut ed: Editor, cfg: Config) -> App {
        ed.author = cfg.author.clone();
        crate::layout::set_full_size_pictures(cfg.pictures);
        let layout = Layout::build(&ed.doc);
        let layout_ver = ed.version;
        App {
            ed,
            layout,
            layout_ver,
            cfg,
            mode: Mode::Normal,
            msg: None,
            scroll: 0,
            hscroll: 0,
            follow: true,
            reveal: false,
            block_mode: false,
            home_prefix: 0,
            pending_retrieve: false,
            last_search: String::new(),
            last_replace: String::new(),
            speller: None,
            thesaurus: None,
            spell_checked: 0,
            quit: false,
            mouse_down: false,
            geo: Geo::default(),
            words_cache: (u64::MAX, 0),
            installed_fonts: None,
            font_subs: None,
            screen_map: Vec::new(),
            osc52: false,
            sysclip: None,
            gfx: crate::graphics::Graphics::default(),
        }
    }

    pub fn ensure_layout(&mut self) {
        if self.layout_ver != self.ed.version {
            self.layout = Layout::build(&self.ed.doc);
            self.layout_ver = self.ed.version;
        }
    }

    pub fn word_count(&mut self) -> usize {
        if self.words_cache.0 != self.ed.version {
            self.words_cache = (self.ed.version, self.ed.doc.word_count());
        }
        self.words_cache.1
    }

    pub fn file_name(&self) -> String {
        match &self.ed.path {
            Some(p) => p.display().to_string(),
            None => "(untitled)".into(),
        }
    }

    fn say(&mut self, s: impl Into<String>) {
        self.msg = Some(s.into());
    }

    // ===================================================================
    // Event dispatch

    pub fn handle(&mut self, ev: Event) {
        self.ensure_layout();
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k),
            Event::Paste(s) => match &mut self.mode {
                Mode::Normal => {
                    self.ed.insert_text(&s);
                    self.follow = true;
                }
                Mode::Prompt(p) => {
                    let line: Vec<char> = s.lines().next().unwrap_or("").chars().collect();
                    let n = line.len();
                    p.input.splice(p.cur..p.cur, line);
                    p.cur += n;
                }
                _ => {}
            },
            Event::Mouse(m) => self.on_mouse(m),
            _ => {}
        }
        self.ensure_layout();
    }

    fn on_key(&mut self, k: KeyEvent) {
        self.msg = None;
        let mut mode = std::mem::replace(&mut self.mode, Mode::Normal);
        let keep = match &mut mode {
            Mode::Normal => {
                self.normal_key(k);
                false
            }
            Mode::Menu { m, item } => self.menu_key(k, m, item),
            Mode::Prompt(p) => self.prompt_key(k, p),
            Mode::Confirm { .. } => {
                let Mode::Confirm { msg, act } = mode else {
                    unreachable!()
                };
                self.confirm_key(k, msg, act);
                return;
            }
            Mode::Choice(c) => self.choice_key(k, c),
            Mode::Form(f) => self.form_key(k, f),
            Mode::Spell(s) => self.spell_key(k, s),
            Mode::Thes(t) => self.thes_key(k, t),
            Mode::Replace(r) => self.replace_key(k, r),
            Mode::Help { scroll } => self.help_key(k, scroll),
            Mode::Palette { query, sel } => self.palette_key(k, query, sel),
            Mode::FontPick(fp) => self.font_pick_key(k, fp),
            Mode::HfEdit(h) => self.hf_key(k, h),
            Mode::ThemePick {
                sel,
                original,
                original_outline,
            } => {
                let (o, oo) = (*original, *original_outline);
                self.theme_pick_key(k, sel, o, oo)
            }
            Mode::Preview {
                page,
                scroll,
                hscroll,
                pages,
            } => self.preview_key(k, page, scroll, hscroll, pages.len()),
        };
        if keep && matches!(self.mode, Mode::Normal) {
            self.mode = mode;
        }
    }

    // ===================================================================
    // Normal editing keys

    fn go(&mut self, p: Pos, extend: bool) {
        self.ed.move_to(p, extend);
        self.ed.goal_x = None;
        self.follow = true;
    }

    fn vertical(&mut self, dir: i32, extend: bool) {
        let goal = self
            .ed
            .goal_x
            .unwrap_or_else(|| self.layout.locate(self.ed.cur).1);
        match self.layout.vertical(self.ed.cur, goal, dir) {
            Some(p) => self.ed.move_to(p, extend),
            None if dir < 0 => self.ed.move_to(self.ed.doc_start(), extend),
            None => self.ed.move_to(self.ed.doc.end_pos(), extend),
        }
        self.ed.goal_x = Some(goal);
        self.follow = true;
    }

    fn screen_edge(&mut self, bottom: bool, extend: bool) {
        // The first or last text line drawn on screen (in either view).
        let lines = &self.layout.lines;
        let spots = self
            .screen_map
            .iter()
            .filter(|s| !lines[s.li].segs.is_empty());
        let spot = if bottom {
            spots.max_by_key(|s| (s.y, s.li))
        } else {
            spots.min_by_key(|s| (s.y, s.li))
        };
        if let Some(p) = spot.and_then(|s| self.layout.hit(s.li, 0)) {
            self.go(p, extend);
        }
    }

    fn page_jump(&mut self, dir: i32, extend: bool) {
        let (li, _) = self.layout.locate(self.ed.cur);
        let page = self.layout.lines.get(li).map(|l| l.page).unwrap_or(1) as i64;
        let target = page + dir as i64;
        if target < 1 {
            return self.go(self.ed.doc_start(), extend);
        }
        if target > self.layout.pages as i64 {
            return self.go(self.ed.doc.end_pos(), extend);
        }
        let start = self.layout.page_start(target as u32);
        for l in start..self.layout.lines.len() {
            if !self.layout.lines[l].segs.is_empty() {
                if let Some(p) = self.layout.hit(l, 0) {
                    self.go(p, extend);
                }
                return;
            }
        }
    }

    fn classic_home(&mut self, k: &KeyEvent) -> bool {
        let n = self.home_prefix;
        self.home_prefix = 0;
        let extend = k.modifiers.contains(KeyModifiers::SHIFT) || self.block_mode;
        match k.code {
            KeyCode::Home => {
                self.home_prefix = n + 1;
                true
            }
            KeyCode::Up if n >= 2 => {
                self.go(self.ed.doc_start(), extend);
                true
            }
            KeyCode::Down if n >= 2 => {
                self.go(self.ed.doc.end_pos(), extend);
                true
            }
            KeyCode::Up => {
                self.screen_edge(false, extend);
                true
            }
            KeyCode::Down => {
                self.screen_edge(true, extend);
                true
            }
            KeyCode::Left => {
                self.go(self.ed.line_start(&self.layout), extend);
                true
            }
            KeyCode::Right => {
                self.go(self.ed.line_end(&self.layout), extend);
                true
            }
            _ => false,
        }
    }

    fn normal_key(&mut self, k: KeyEvent) {
        let c = commands::chord(&k);
        let classic = self.cfg.keymap == Keymap::Classic;
        if classic && self.home_prefix > 0 && self.classic_home(&k) {
            return;
        }
        self.home_prefix = 0;
        if let Some(cmd) = commands::lookup(self.cfg.keymap, c) {
            self.run(cmd);
            return;
        }
        if let Some(m) = commands::menu_for_alt(c) {
            self.mode = Mode::Menu {
                m,
                item: first_item(m),
            };
            return;
        }
        self.follow = true;
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let extend = shift || self.block_mode;
        let cur = self.ed.cur;
        match k.code {
            KeyCode::Left => {
                let p = if ctrl {
                    self.ed.word_left(cur)
                } else {
                    self.ed.pos_left(cur)
                };
                self.go(p, extend);
            }
            KeyCode::Right => {
                let p = if ctrl {
                    self.ed.word_right(cur)
                } else {
                    self.ed.pos_right(cur)
                };
                self.go(p, extend);
            }
            // Ctrl+Up/Down: previous/next paragraph, as in Word.
            KeyCode::Up if ctrl => {
                let p = if cur.off > 0 {
                    Pos::new(cur.pr, 0)
                } else {
                    self.ed
                        .doc
                        .prev_ref(cur.pr)
                        .map(|r| Pos::new(r, 0))
                        .unwrap_or(cur)
                };
                self.go(p, extend);
            }
            KeyCode::Down if ctrl => {
                let p = self
                    .ed
                    .doc
                    .next_ref(cur.pr)
                    .map(|r| Pos::new(r, 0))
                    .unwrap_or_else(|| self.ed.doc.end_pos());
                self.go(p, extend);
            }
            KeyCode::Up => self.vertical(-1, extend),
            KeyCode::Down => self.vertical(1, extend),
            KeyCode::Home if ctrl => self.go(self.ed.doc_start(), extend),
            KeyCode::Home => {
                self.go(self.ed.line_start(&self.layout), extend);
                if classic {
                    self.home_prefix = 1;
                }
            }
            KeyCode::End if ctrl && classic => {
                self.ed.delete_to_line_end(&self.layout);
            }
            KeyCode::End if ctrl => self.go(self.ed.doc.end_pos(), extend),
            KeyCode::End => self.go(self.ed.line_end(&self.layout), extend),
            KeyCode::PageUp | KeyCode::PageDown => {
                let dir = if k.code == KeyCode::PageUp { -1 } else { 1 };
                if classic {
                    self.page_jump(dir, extend);
                } else {
                    for _ in 0..self.geo.doc_h.saturating_sub(1).max(1) {
                        self.vertical(dir, extend);
                    }
                }
            }
            KeyCode::Enter if shift => self.ed.line_break(),
            KeyCode::Enter => {
                if self.pending_retrieve {
                    self.pending_retrieve = false;
                    self.ed.paste();
                } else {
                    self.block_mode = false;
                    self.ed.newline();
                }
            }
            KeyCode::Backspace if ctrl || alt => self.ed.delete_word_back(),
            KeyCode::Backspace => {
                self.block_mode = false;
                self.ed.backspace();
            }
            KeyCode::Delete => {
                self.block_mode = false;
                self.ed.delete();
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let back = shift || k.code == KeyCode::BackTab;
                let in_list = self.ed.para().props.list.is_some();
                if self.ed.cur.pr.cell.is_some() {
                    if back {
                        self.ed.prev_cell();
                    } else {
                        self.ed.next_cell(true);
                    }
                } else if in_list && (self.ed.cur.off == 0 || back) {
                    self.ed.change_level(if back { -1 } else { 1 });
                } else if !back {
                    self.ed.insert_char('\t');
                }
            }
            KeyCode::Esc => {
                self.block_mode = false;
                self.pending_retrieve = false;
                self.ed.anchor = None;
            }
            KeyCode::Char(ch) if !(ctrl || alt) || (ctrl && alt) => {
                self.block_mode = false;
                self.ed.insert_char(ch);
            }
            KeyCode::Insert => self.say("Typeover is not supported; text is always inserted"),
            _ => {
                let help = if classic { "F3" } else { "F1" };
                self.say(format!(
                    "{} is not assigned ({help} shows the keyboard)",
                    commands::chord_label(&c)
                ));
            }
        }
    }

    // ===================================================================
    // Commands

    fn fmt_cmd(&mut self, flag: u16, name: &str) {
        let had_sel = self.ed.sel_range().is_some();
        let on = self.ed.toggle_fmt(flag);
        self.block_mode = false;
        if !had_sel {
            self.say(format!("{name} {}", if on { "on" } else { "off" }));
        }
    }

    fn align_cmd(&mut self, a: Align) {
        self.ed.set_props(|p| p.align = a);
        self.block_mode = false;
        self.say(format!("Justification: {}", Align::NAMES[a.index()]));
    }

    fn choice(&mut self, title: &str, items: Vec<(String, ChoiceAct)>, numbered: bool) {
        let items = if numbered {
            items
                .into_iter()
                .enumerate()
                .map(|(i, (l, a))| (format!("{} {l}", i + 1), a))
                .collect()
        } else {
            items
        };
        self.mode = Mode::Choice(Choice {
            title: title.into(),
            items,
            sel: 0,
            numbered,
        });
    }

    fn cmd_items(cmds: &[Cmd]) -> Vec<(String, ChoiceAct)> {
        cmds.iter()
            .map(|c| {
                (
                    c.label().trim_end_matches("...").to_string(),
                    ChoiceAct::Cmd(*c),
                )
            })
            .collect()
    }

    fn prompt(&mut self, label: &str, init: &str, act: PromptAct) {
        let input: Vec<char> = init.chars().collect();
        let cur = input.len();
        self.mode = Mode::Prompt(Prompt {
            label: label.into(),
            input,
            cur,
            act,
        });
    }

    pub fn run(&mut self, cmd: Cmd) {
        use Cmd::*;
        self.follow = true;
        let classic = self.cfg.keymap == Keymap::Classic;
        match cmd {
            New => self.guard_unsaved(After::New),
            Open => self.prompt(
                if classic {
                    "Document to be retrieved: "
                } else {
                    "Open file: "
                },
                "",
                PromptAct::Open,
            ),
            ListFiles => {
                let dir = self
                    .ed
                    .path
                    .as_ref()
                    .and_then(|p| p.parent().map(Path::to_path_buf))
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| PathBuf::from("."));
                self.list_files(&dir);
            }
            Save => self.save(After::Nothing),
            SaveAs => {
                let cur = self
                    .ed
                    .path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.prompt(
                    if classic {
                        "Document to be saved: "
                    } else {
                        "Save as: "
                    },
                    &cur,
                    PromptAct::SaveAs(After::Nothing),
                );
            }
            ExportPdf => {
                let cur = self
                    .ed
                    .path
                    .as_ref()
                    .map(|p| p.with_extension("pdf").display().to_string())
                    .unwrap_or_else(|| "document.pdf".into());
                self.prompt("Export PDF to: ", &cur, PromptAct::ExportPdf);
            }
            PrintMenu => self.choice("Print", Self::cmd_items(&[Print, ExportPdf, Preview]), true),
            Print => match io::pdf::print(&self.ed.doc) {
                Ok(m) => self.say(m),
                Err(e) => self.say(format!("Print failed: {e}")),
            },
            Preview => {
                let pages = compose(&self.ed.doc, &self.layout);
                let (li, _) = self.layout.locate(self.ed.cur);
                let page = self
                    .layout
                    .lines
                    .get(li)
                    .map(|l| l.page as usize)
                    .unwrap_or(1)
                    .saturating_sub(1);
                self.mode = Mode::Preview {
                    page,
                    scroll: 0,
                    hscroll: 0,
                    pages,
                };
            }
            Exit => {
                if classic {
                    if self.ed.modified {
                        self.mode = Mode::Confirm {
                            msg: "Save document? Yes (No)".into(),
                            act: ConfirmAct::SaveFirst(After::ExitAsk),
                        };
                    } else {
                        self.do_after(After::ExitAsk);
                    }
                } else {
                    self.guard_unsaved(After::Quit);
                }
            }
            Undo => {
                if !self.ed.undo() {
                    self.say("Nothing to undo");
                }
            }
            Redo => {
                if !self.ed.redo() {
                    self.say("Nothing to redo");
                }
            }
            Cut | Copy => {
                let ok = if cmd == Cut {
                    self.ed.cut()
                } else {
                    self.ed.copy()
                };
                self.block_mode = false;
                if ok {
                    self.copy_to_terminal();
                    self.say(if cmd == Cut {
                        "Cut to clipboard"
                    } else {
                        "Copied to clipboard"
                    });
                } else {
                    self.say("Select some text first (Shift+arrows or Block)");
                }
            }
            Paste | PastePlain => {
                self.block_mode = false;
                self.paste(cmd == PastePlain);
            }
            SelectAll => self.ed.select_all(),
            Block => {
                self.block_mode = !self.block_mode;
                if self.block_mode {
                    self.ed.anchor = Some(self.ed.cur);
                } else {
                    self.ed.anchor = None;
                }
            }
            MoveMenu => {
                if self.ed.sel_range().is_some() {
                    self.choice(
                        "Block",
                        vec![
                            ("Move (cut)".into(), ChoiceAct::MoveCut),
                            ("Copy".into(), ChoiceAct::MoveCopy),
                            ("Delete".into(), ChoiceAct::MoveDelete),
                        ],
                        true,
                    );
                } else {
                    self.say("Block some text first (Alt+F4 or F12, then move the cursor)");
                }
            }
            InsertDate => self.ed.insert_text(&today()),
            Find => {
                let s = self.last_search.clone();
                self.prompt(
                    if classic { "-> Srch: " } else { "Find: " },
                    &s,
                    PromptAct::Find(true),
                );
            }
            FindNext | FindPrev => {
                let fwd = cmd == FindNext;
                if self.last_search.is_empty() {
                    let s = self.last_search.clone();
                    self.prompt(
                        if fwd { "-> Srch: " } else { "<- Srch: " },
                        &s,
                        PromptAct::Find(fwd),
                    );
                } else {
                    self.do_find(fwd);
                }
            }
            Replace => {
                let s = self.last_search.clone();
                self.prompt("Replace: ", &s, PromptAct::ReplaceFrom);
            }
            Bold => self.fmt_cmd(Fmt::BOLD, "Bold"),
            Italic => self.fmt_cmd(Fmt::ITALIC, "Italic"),
            Underline => self.fmt_cmd(Fmt::UNDERLINE, "Underline"),
            Strike => self.fmt_cmd(Fmt::STRIKE, "Strikeout"),
            Superscript => self.fmt_cmd(Fmt::SUPER, "Superscript"),
            Subscript => self.fmt_cmd(Fmt::SUB, "Subscript"),
            ClearFmt => {
                self.ed.clear_fmt();
                self.block_mode = false;
                self.say("Normal text");
            }
            FontMenu => self.choice(
                "Appearance",
                Self::cmd_items(&[
                    Bold,
                    Italic,
                    Underline,
                    Strike,
                    Superscript,
                    Subscript,
                    ClearFmt,
                ]),
                true,
            ),
            AlignLeft => self.align_cmd(Align::Left),
            AlignCenter => self.align_cmd(Align::Center),
            AlignRight => self.align_cmd(Align::Right),
            AlignFull => self.align_cmd(Align::Full),
            Indent => self.ed.change_level(1),
            Outdent => self.ed.change_level(-1),
            LineSpacing => {
                let s = if self.ed.para().props.spacing >= 2 {
                    1
                } else {
                    2
                };
                self.ed.set_props(|p| p.spacing = s);
                self.say(format!("Line spacing: {s}"));
            }
            StyleMenu => {
                let sheet = self.ed.doc.styles.clone();
                let cur = self.ed.para().props.style.clone();
                let items: Vec<(String, ChoiceAct)> = sheet
                    .menu()
                    .iter()
                    .map(|s| (display_style_name(&s.name), ChoiceAct::Style(s.id.clone())))
                    .collect();
                let n = items.len();
                self.choice("Paragraph style", items, n <= 9);
                if let Mode::Choice(c) = &mut self.mode {
                    c.sel = sheet
                        .menu()
                        .iter()
                        .position(|s| s.id == cur || (cur.is_empty() && sheet.is_default(&s.id)))
                        .unwrap_or(0);
                }
            }
            FontDialog => self.open_font_form(),
            FormatMenu => self.open_format_form(),
            BulletList => self.ed.toggle_list(ListKind::Bullet),
            NumberList => self.ed.toggle_list(ListKind::Number),
            PageBreak => {
                if !self.ed.page_break() {
                    self.say("Page breaks cannot go inside a table");
                }
            }
            TableMenu => self.choice(
                "Tables",
                Self::cmd_items(&[
                    InsertTable,
                    TableAddRow,
                    TableAddCol,
                    TableDelRow,
                    TableDelCol,
                    TableDelete,
                ]),
                true,
            ),
            InsertTable => {
                if self.ed.cur.pr.cell.is_some() {
                    self.say("Tables cannot be nested");
                } else {
                    self.prompt("Number of columns: ", "3", PromptAct::TableCols);
                }
            }
            TableAddRow | TableAddCol | TableDelRow | TableDelCol | TableDelete => {
                let ok = match cmd {
                    TableAddRow => self.ed.table_add_row(),
                    TableAddCol => self.ed.table_add_col(),
                    TableDelRow => self.ed.table_del_row(),
                    TableDelCol => self.ed.table_del_col(),
                    _ => self.ed.table_delete(),
                };
                if !ok {
                    self.say("The cursor is not in a table");
                }
            }
            FootnoteMenu => {
                self.choice("Footnote", Self::cmd_items(&[Footnote, EditFootnote]), true)
            }
            Footnote => self.prompt("Footnote text: ", "", PromptAct::FootnoteNew),
            EditFootnote => match self.ed.footnote_near_cursor() {
                Some(id) => {
                    let n = self.layout.fn_num.get(&id).cloned().unwrap_or_default();
                    let text = self.ed.doc.footnotes.get(&id).cloned().unwrap_or_default();
                    self.prompt(
                        &format!("Footnote {n}: "),
                        &text,
                        PromptAct::FootnoteEdit(id),
                    );
                }
                None => self.say("Put the cursor next to a footnote number to edit it"),
            },
            Spell => self.start_spell(),
            Thesaurus => self.start_thesaurus(),
            WordCount => {
                let w = self.word_count();
                let c = self.ed.doc.char_count();
                self.say(format!(
                    "Words: {w}   Characters: {c}   Pages: {}",
                    self.layout.pages
                ));
            }
            Settings => self.open_settings_form(),
            ToggleKeymap => {
                self.cfg.keymap = if classic {
                    Keymap::Modern
                } else {
                    Keymap::Classic
                };
                let _ = self.cfg.save();
                self.say(if classic {
                    "Modern keyboard: Ctrl+S save, Ctrl+B bold, F10 menu, F1 help (Alt+K switches back)"
                } else {
                    "Classic keyboard: F10 save, F6 bold, F7 exit, F3 help, Alt+= menu (Alt+K switches back)"
                });
            }
            ToggleTheme => {
                let sel = Theme::ALL
                    .iter()
                    .position(|t| *t == self.cfg.theme)
                    .unwrap_or(0);
                self.mode = Mode::ThemePick {
                    sel,
                    original: self.cfg.theme,
                    original_outline: self.cfg.outline,
                };
            }
            SideBySide => {
                self.cfg.side_by_side = !self.cfg.side_by_side;
                self.scroll = 0;
                let _ = self.cfg.save();
                self.say(if self.cfg.side_by_side {
                    "Pages side by side when the window is wide enough"
                } else {
                    "One page at a time"
                });
            }
            PaperOutline => {
                self.cfg.outline = !self.cfg.outline;
                let _ = self.cfg.save();
                self.say(if self.cfg.outline {
                    "Paper outline on"
                } else {
                    "Paper outline off"
                });
            }
            RevealCodes => self.reveal = !self.reveal,
            Help => self.mode = Mode::Help { scroll: 0 },
            Support => {
                let url = crate::SUPPORT_URL;
                match crate::platform::open(url) {
                    Ok(()) => self.say(format!("Opening {url} - thank you!")),
                    Err(_) => self.say(format!("Support WordBench at {url}")),
                }
            }
            MenuBar => {
                self.mode = Mode::Menu {
                    m: 0,
                    item: first_item(0),
                }
            }
            CommandPalette => {
                self.mode = Mode::Palette {
                    query: String::new(),
                    sel: 0,
                }
            }
            InsertCaption => self.open_caption_form(),
            EditHeader => self.open_hf_editor(false, None),
            EditFooter => self.open_hf_editor(true, None),
            UpdateFields => {
                let n = self.ed.update_fields();
                self.say(format!("{n} field(s) updated"));
            }
            SelectParagraph => {
                let len = self.ed.para().len();
                let pr = self.ed.cur.pr;
                self.ed.select_range(Pos::new(pr, 0), Pos::new(pr, len));
            }
            ReviewMenu => {
                let m = MENUS.iter().position(|(n, _)| *n == "Review").unwrap_or(0);
                self.mode = Mode::Menu {
                    m,
                    item: first_item(m),
                };
            }
            InsertLink => {
                if self.ed.sel_range().is_none() {
                    self.say("Select the text to link first");
                } else {
                    let cur = self
                        .ed
                        .link_at_cursor()
                        .map(|(_, u)| u)
                        .unwrap_or_else(|| "https://".into());
                    self.prompt(
                        "Link to (web address, or #bookmark): ",
                        &cur,
                        PromptAct::Link,
                    );
                }
            }
            FollowLink => match self.ed.link_at_cursor() {
                Some((_, url)) => self.follow_link(&url),
                None => self.say("The cursor is not on a hyperlink"),
            },
            RemoveLink => {
                if !self.ed.remove_link() {
                    self.say("The cursor is not on a hyperlink");
                }
            }
            InsertComment => self.prompt("Comment: ", "", PromptAct::CommentNew),
            Comments => {
                let items: Vec<(String, ChoiceAct)> = self
                    .ed
                    .comment_positions()
                    .into_iter()
                    .map(|(id, pos)| {
                        let c = self.ed.doc.comments.get(&id).cloned().unwrap_or_default();
                        let date = c.date.get(..10).unwrap_or("");
                        (
                            format!("{} ({date}): {}", c.author, c.text),
                            ChoiceAct::GoTo(pos),
                        )
                    })
                    .collect();
                if items.is_empty() {
                    self.say("This document has no comments");
                } else {
                    self.choice("Comments  (Enter goes to the comment)", items, false);
                }
            }
            EditComment => match self.ed.comment_at_cursor() {
                Some(id) => {
                    let text = self
                        .ed
                        .doc
                        .comments
                        .get(&id)
                        .map(|c| c.text.clone())
                        .unwrap_or_default();
                    self.prompt("Edit comment: ", &text, PromptAct::CommentEdit(id));
                }
                None => self.say(
                    "Put the cursor on a [Comment] marker (Review > List comments finds them)",
                ),
            },
            DeleteComment => match self.ed.comment_at_cursor() {
                Some(id) => {
                    self.ed.delete_comment(&id);
                    self.say("Comment deleted");
                }
                None => self.say("Put the cursor on a [Comment] marker"),
            },
            TrackChanges => {
                self.ed.track = !self.ed.track;
                self.ed.doc.track_changes = self.ed.track;
                self.ed.modified = true;
                self.say(if self.ed.track {
                    format!("Tracking changes as {}", self.ed.author)
                } else {
                    "Track changes off".to_string()
                });
            }
            NextChange => {
                if self.ed.next_change() {
                    let n = self.ed.change_count();
                    self.say(format!("{n} tracked change(s) in the document; Accept/Reject change acts on the selection"));
                } else {
                    self.say("No tracked changes");
                }
            }
            AcceptChange | RejectChange => {
                let accept = cmd == AcceptChange;
                if self.ed.sel_range().is_none() {
                    self.ed.next_change();
                }
                match self.ed.sel_range() {
                    Some(r) => {
                        let n = self.ed.review(accept, Some(r));
                        self.say(format!(
                            "{n} change(s) {}",
                            if accept { "accepted" } else { "rejected" }
                        ));
                    }
                    None => self.say("No tracked changes"),
                }
            }
            AcceptAll | RejectAll => {
                let accept = cmd == AcceptAll;
                let n = self.ed.review(accept, None);
                self.say(format!(
                    "{n} change(s) {}",
                    if accept { "accepted" } else { "rejected" }
                ));
            }
            Endnote => self.prompt("Endnote text: ", "", PromptAct::EndnoteNew),
            Properties => self.open_props_form(),
            InsertPicture => self.prompt("Picture file (PNG, JPEG, GIF): ", "", PromptAct::Picture),
        }
    }

    /// Put what was just copied on the desktop clipboard (text and HTML),
    /// or failing that the terminal's.
    fn copy_to_terminal(&mut self) {
        let text = crate::io::html::plain(&self.ed.clipboard);
        if let Some(sc) = self.sysclip.as_mut() {
            let html = crate::io::html::from_blocks(&self.ed.clipboard, &self.ed.doc);
            if sc.set(&text, &html) {
                return;
            }
        }
        if !self.osc52 {
            return;
        }
        let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(seq.as_bytes());
        let _ = out.flush();
    }

    /// Paste: something copied in another program comes from the desktop
    /// clipboard (formatted, if it has HTML); our own copies use the
    /// internal clipboard, which keeps everything.
    pub fn paste(&mut self, plain_only: bool) {
        if let Some((text, html)) = self.sysclip.as_mut().and_then(|sc| sc.get()) {
            let ours = self.sysclip.as_ref().is_some_and(|sc| sc.is_ours(&text));
            if !ours || self.ed.clipboard.is_empty() {
                if !plain_only && let Some(h) = html {
                    let blocks = crate::io::html::to_blocks(&h, &mut self.ed.doc);
                    if !blocks.is_empty() {
                        self.ed.paste_foreign(&blocks);
                        self.follow = true;
                        return;
                    }
                }
                if !text.is_empty() {
                    self.ed.insert_text(&text.replace("\r\n", "\n"));
                    self.follow = true;
                    return;
                }
            }
        }
        if plain_only {
            let text = crate::io::html::plain(&self.ed.clipboard);
            if !text.is_empty() {
                self.ed.insert_text(&text);
                return;
            }
        } else if self.ed.paste() {
            return;
        }
        self.say("Clipboard is empty");
    }

    fn do_find(&mut self, forward: bool) {
        self.block_mode = false;
        let q = self.last_search.clone();
        match self.ed.find(&q, forward) {
            FindResult::Found => {}
            FindResult::Wrapped => self.say("Search wrapped around"),
            FindResult::NotFound => self.say("* Not found *"),
        }
    }

    // ===================================================================
    // Files

    fn guard_unsaved(&mut self, after: After) {
        if self.ed.modified {
            let name = self
                .ed
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            let msg = format!(
                "Save changes to {}? (Y/N, Esc cancels)",
                name.as_deref().unwrap_or("this document")
            );
            self.mode = Mode::Confirm {
                msg,
                act: ConfirmAct::SaveFirst(after),
            };
        } else {
            self.do_after(after);
        }
    }

    fn do_after(&mut self, after: After) {
        match after {
            After::Nothing => {}
            After::Quit => self.quit = true,
            After::ExitAsk => {
                self.mode = Mode::Confirm {
                    msg: "Exit WordBench? No (Yes)".into(),
                    act: ConfirmAct::ExitWp,
                };
            }
            After::New => self.replace_editor(Editor::new()),
            After::Open(p) => self.open_path(&p),
        }
    }

    fn replace_editor(&mut self, mut ed: Editor) {
        ed.clipboard = std::mem::take(&mut self.ed.clipboard);
        ed.author = self.cfg.author.clone();
        ed.version = self.ed.version + 1;
        self.ed = ed;
        self.scroll = 0;
        self.hscroll = 0;
        self.block_mode = false;
        self.pending_retrieve = false;
        self.ensure_layout();
    }

    pub fn open_path(&mut self, p: &Path) {
        match io::load(p) {
            Ok(doc) => {
                self.replace_editor(Editor::with_doc(doc, Some(p.to_path_buf())));
                let track = if self.ed.track {
                    "  (Track changes is on)"
                } else {
                    ""
                };
                self.say(format!("Opened {}{track}", p.display()));
            }
            Err(_) if !p.exists() => {
                self.replace_editor(Editor::with_doc(Document::new(), Some(p.to_path_buf())));
                self.say(format!("New file: {}", p.display()));
            }
            Err(e) => self.say(format!("Could not open {}: {e}", p.display())),
        }
    }

    fn save(&mut self, after: After) {
        match self.ed.path.clone() {
            Some(p) if io::ext(&p) != "pdf" => {
                if self.save_to(&p) {
                    self.do_after(after);
                }
            }
            _ => {
                let classic = self.cfg.keymap == Keymap::Classic;
                self.prompt(
                    if classic {
                        "Document to be saved: "
                    } else {
                        "Save as: "
                    },
                    "",
                    PromptAct::SaveAs(after),
                );
            }
        }
    }

    fn save_to(&mut self, p: &Path) -> bool {
        self.ed.update_fields();
        match io::save(&self.ed.doc, p) {
            Ok(note) => {
                if io::ext(p) != "pdf" {
                    self.ed.path = Some(p.to_path_buf());
                    self.ed.modified = false;
                }
                let note = note.map(|n| format!(" ({n})")).unwrap_or_default();
                self.say(format!("Saved {}{note}", p.display()));
                true
            }
            Err(e) => {
                self.say(format!("Save failed: {e}"));
                false
            }
        }
    }

    fn list_files(&mut self, dir: &Path) {
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let mut entries: Vec<(bool, String, PathBuf, u64)> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .filter_map(Result::ok)
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| {
                    let md = e.metadata().ok();
                    let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                    (
                        !is_dir,
                        e.file_name().to_string_lossy().into_owned(),
                        e.path(),
                        md.map(|m| m.len()).unwrap_or(0),
                    )
                })
                .collect(),
            Err(e) => return self.say(format!("Cannot list {}: {e}", dir.display())),
        };
        entries.sort_by_key(|a| (a.0, a.1.to_lowercase()));
        let mut items = Vec::new();
        if let Some(parent) = dir.parent() {
            items.push((
                "..  <Parent>".to_string(),
                ChoiceAct::Path(parent.to_path_buf()),
            ));
        }
        for (is_file, name, path, size) in entries {
            let label = if is_file {
                format!("{name:<36} {size:>9}")
            } else {
                format!("{name}/")
            };
            items.push((label, ChoiceAct::Path(path)));
        }
        self.choice(&format!("List Files: {}", dir.display()), items, false);
    }

    // ===================================================================
    // Modal key handlers. Each returns true to keep the mode open.

    fn menu_key(&mut self, k: KeyEvent, m: &mut usize, item: &mut usize) -> bool {
        let c = commands::chord(&k);
        let items = MENUS[*m].1;
        let step = |item: &mut usize, dir: i32| {
            let n = items.len() as i32;
            let mut i = *item as i32;
            for _ in 0..n {
                i = (i + dir).rem_euclid(n);
                if items[i as usize].is_some() {
                    break;
                }
            }
            *item = i as usize;
        };
        match k.code {
            KeyCode::Esc | KeyCode::F(1) | KeyCode::F(10) | KeyCode::F(9) => false,
            KeyCode::Left => {
                *m = (*m + MENUS.len() - 1) % MENUS.len();
                *item = first_item(*m);
                true
            }
            KeyCode::Right => {
                *m = (*m + 1) % MENUS.len();
                *item = first_item(*m);
                true
            }
            KeyCode::Up | KeyCode::BackTab => {
                step(item, -1);
                true
            }
            KeyCode::Down | KeyCode::Tab => {
                step(item, 1);
                true
            }
            KeyCode::Home | KeyCode::PageUp => {
                *item = first_item(*m);
                true
            }
            KeyCode::End | KeyCode::PageDown => {
                *item = items.iter().rposition(Option::is_some).unwrap_or(0);
                true
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(Some(cmd)) = items.get(*item) {
                    self.run(*cmd);
                }
                false
            }
            KeyCode::Char('=') if c.alt => false,
            // Alt+letter always switches menus; a plain letter picks the
            // item with that underlined letter.
            KeyCode::Char(ch) if c.alt => {
                if let Some(i) = commands::menu_for_key(ch) {
                    *m = i;
                    *item = first_item(i);
                }
                true
            }
            KeyCode::Char(ch) => {
                let ch = ch.to_ascii_lowercase();
                let keys = commands::item_keys(*m);
                if let Some(i) = keys.iter().position(|k| k.is_some_and(|(_, c)| c == ch)) {
                    if let Some(Some(cmd)) = items.get(i) {
                        self.run(*cmd);
                    }
                    return false;
                }
                true
            }
            _ => true,
        }
    }

    fn prompt_key(&mut self, k: KeyEvent, p: &mut Prompt) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => false,
            KeyCode::Enter => {
                let text: String = p.input.iter().collect();
                let act = std::mem::replace(&mut p.act, PromptAct::Open);
                self.prompt_submit(act, text);
                false
            }
            KeyCode::Left => {
                p.cur = p.cur.saturating_sub(1);
                true
            }
            KeyCode::Right => {
                p.cur = (p.cur + 1).min(p.input.len());
                true
            }
            KeyCode::Home => {
                p.cur = 0;
                true
            }
            KeyCode::End => {
                p.cur = p.input.len();
                true
            }
            KeyCode::Backspace => {
                if p.cur > 0 {
                    p.cur -= 1;
                    p.input.remove(p.cur);
                }
                true
            }
            KeyCode::Delete => {
                if p.cur < p.input.len() {
                    p.input.remove(p.cur);
                }
                true
            }
            KeyCode::Char('u') if ctrl => {
                p.input.drain(..p.cur);
                p.cur = 0;
                true
            }
            KeyCode::Tab
                if matches!(
                    p.act,
                    PromptAct::Open | PromptAct::SaveAs(_) | PromptAct::ExportPdf
                ) =>
            {
                let s: String = p.input.iter().collect();
                if let Some(done) = complete_path(&s) {
                    p.input = done.chars().collect();
                    p.cur = p.input.len();
                }
                true
            }
            KeyCode::F(5) if matches!(p.act, PromptAct::Open) => {
                self.run(Cmd::ListFiles);
                false
            }
            KeyCode::Char(ch) if !ctrl => {
                p.input.insert(p.cur, ch);
                p.cur += 1;
                true
            }
            _ => true,
        }
    }

    fn prompt_submit(&mut self, act: PromptAct, text: String) {
        match act {
            PromptAct::Open => {
                if !text.trim().is_empty() {
                    self.guard_unsaved(After::Open(expand_path(&text)));
                }
            }
            PromptAct::SaveAs(after) => {
                if text.trim().is_empty() {
                    return;
                }
                let p = expand_path(&text);
                if p.extension().is_none() {
                    // No extension typed: ask which kind of file to save.
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let items = SAVE_TYPES
                        .iter()
                        .map(|(ext, what)| {
                            (
                                format!("{what:<34} {name}.{ext}"),
                                ChoiceAct::SaveAsType(p.clone(), ext, after.clone()),
                            )
                        })
                        .collect();
                    self.choice("Save as type", items, true);
                    return;
                }
                if self.save_to(&p) {
                    self.do_after(after);
                }
            }
            PromptAct::ExportPdf => {
                if text.trim().is_empty() {
                    return;
                }
                let p = with_ext(expand_path(&text), "pdf");
                match io::pdf::save(&self.ed.doc, &p) {
                    Ok(()) => self.say(format!(
                        "Exported {} ({} pages)",
                        p.display(),
                        self.layout.pages
                    )),
                    Err(e) => self.say(format!("Export failed: {e}")),
                }
            }
            PromptAct::Find(fwd) => {
                if !text.is_empty() {
                    self.last_search = text;
                    self.do_find(fwd);
                }
            }
            PromptAct::ReplaceFrom => {
                if !text.is_empty() {
                    let r = self.last_replace.clone();
                    self.prompt("Replace with: ", &r, PromptAct::ReplaceTo(text));
                }
            }
            PromptAct::ReplaceTo(from) => {
                self.last_search = from.clone();
                self.last_replace = text.clone();
                self.ed.anchor = None;
                self.replace_next(ReplaceSt {
                    from,
                    to: text,
                    count: 0,
                });
            }
            PromptAct::FootnoteNew => {
                if !text.trim().is_empty() {
                    self.ed.insert_footnote(text.trim().to_string());
                }
            }
            PromptAct::FootnoteEdit(id) => self.ed.set_footnote(id, text.trim().to_string()),
            PromptAct::TableCols => match text.trim().parse::<usize>() {
                Ok(n) if (1..=20).contains(&n) => {
                    self.prompt("Number of rows: ", "2", PromptAct::TableRows(n))
                }
                _ => self.say("Enter a number of columns from 1 to 20"),
            },
            PromptAct::TableRows(cols) => match text.trim().parse::<usize>() {
                Ok(n) if (1..=500).contains(&n) => {
                    if !self.ed.insert_table(n, cols) {
                        self.say("Tables cannot be nested");
                    }
                }
                _ => self.say("Enter a number of rows from 1 to 500"),
            },
            PromptAct::Link => {
                if !text.trim().is_empty()
                    && let Err(e) = self.ed.add_link(&text)
                {
                    self.say(e);
                }
            }
            PromptAct::CommentNew => {
                if !text.trim().is_empty() {
                    self.ed.add_comment(text.trim());
                    self.say("Comment added");
                }
            }
            PromptAct::CommentEdit(id) => self.ed.edit_comment(&id, text.trim().to_string()),
            PromptAct::Picture => {
                if !text.trim().is_empty() {
                    match self.ed.insert_picture(&expand_path(&text)) {
                        Ok(m) => self.say(format!("Inserted {m} (kept when saved as .docx)")),
                        Err(e) => self.say(e),
                    }
                }
            }
            PromptAct::EndnoteNew => {
                if !text.trim().is_empty() {
                    self.ed.insert_footnote(text.trim().to_string());
                    if let Some(id) = self.ed.doc.footnotes.keys().next_back().copied() {
                        self.ed.doc.note_meta.insert(
                            id,
                            NoteMeta {
                                endnote: true,
                                orig_text: String::new(),
                                xml: String::new(),
                            },
                        );
                        self.ed.touch();
                    }
                }
            }
            PromptAct::SpellEdit(pr, s, e) => {
                self.ed.replace_in_para(pr, s, e, &text);
                self.ed.anchor = None;
                self.spell_from(Pos::new(pr, s + text.chars().count()));
            }
        }
    }

    fn confirm_key(&mut self, k: KeyEvent, msg: String, act: ConfirmAct) {
        let yes = match k.code {
            KeyCode::Char('y' | 'Y') => true,
            KeyCode::Char('n' | 'N') => false,
            KeyCode::Enter => matches!(act, ConfirmAct::SaveFirst(_)),
            KeyCode::Esc | KeyCode::F(1) => return,
            _ => {
                self.mode = Mode::Confirm { msg, act };
                return;
            }
        };
        match act {
            ConfirmAct::SaveFirst(after) => {
                if yes {
                    self.save(after);
                } else {
                    self.do_after(after);
                }
            }
            ConfirmAct::ExitWp => {
                if yes {
                    self.quit = true;
                } else {
                    self.replace_editor(Editor::new());
                }
            }
        }
    }

    fn choice_key(&mut self, k: KeyEvent, c: &mut Choice) -> bool {
        let n = c.items.len();
        if n == 0 {
            return false;
        }
        let page = self.geo.height.saturating_sub(8).max(1) as usize;
        match k.code {
            KeyCode::Esc | KeyCode::F(1) | KeyCode::F(7) => false,
            KeyCode::Up => {
                c.sel = (c.sel + n - 1) % n;
                true
            }
            KeyCode::Down => {
                c.sel = (c.sel + 1) % n;
                true
            }
            KeyCode::PageUp => {
                c.sel = c.sel.saturating_sub(page);
                true
            }
            KeyCode::PageDown => {
                c.sel = (c.sel + page).min(n - 1);
                true
            }
            KeyCode::Home => {
                c.sel = 0;
                true
            }
            KeyCode::End => {
                c.sel = n - 1;
                true
            }
            KeyCode::Enter => {
                let act = c.items[c.sel].1.clone();
                self.choice_act(act);
                false
            }
            KeyCode::Char(d) if c.numbered && d.is_ascii_digit() => {
                let i = d.to_digit(10).unwrap_or(0) as usize;
                if (1..=n).contains(&i) {
                    let act = c.items[i - 1].1.clone();
                    self.choice_act(act);
                    return false;
                }
                true
            }
            KeyCode::Char(ch) => {
                let lc = ch.to_lowercase().next().unwrap_or(ch);
                let start = c.sel + 1;
                let label_start = |l: &str| {
                    let l = if c.numbered {
                        l.split_once(' ').map(|x| x.1).unwrap_or(l)
                    } else {
                        l
                    };
                    l.to_lowercase().starts_with(lc)
                };
                if let Some(i) = (0..n)
                    .map(|k| (start + k) % n)
                    .find(|&i| label_start(&c.items[i].0))
                {
                    c.sel = i;
                }
                true
            }
            _ => true,
        }
    }

    fn choice_act(&mut self, act: ChoiceAct) {
        match act {
            ChoiceAct::Cmd(c) => self.run(c),
            ChoiceAct::Path(p) => {
                if p.is_dir() {
                    self.list_files(&p);
                } else {
                    self.guard_unsaved(After::Open(p));
                }
            }
            ChoiceAct::MoveCut | ChoiceAct::MoveCopy => {
                let ok = if matches!(act, ChoiceAct::MoveCut) {
                    self.ed.cut()
                } else {
                    self.ed.copy()
                };
                self.block_mode = false;
                self.ed.anchor = None;
                if ok {
                    self.copy_to_terminal();
                    self.pending_retrieve = true;
                }
            }
            ChoiceAct::MoveDelete => {
                self.ed.delete_selection();
                self.block_mode = false;
            }
            ChoiceAct::SaveAsType(p, ext, after) => {
                if self.save_to(&p.with_extension(ext)) {
                    self.do_after(after);
                }
            }
            ChoiceAct::GoTo(p) => {
                self.ed.move_to(p, false);
                self.follow = true;
            }
            ChoiceAct::Style(s) => {
                self.ed.apply_style(&s);
                self.block_mode = false;
            }
        }
    }

    // ----- forms ---------------------------------------------------------

    fn open_format_form(&mut self) {
        let p = self.ed.para().props.clone();
        let d = &self.ed.doc;
        let pg = &d.page;
        let landscape = pg.width > pg.height;
        let (pw, ph) = if landscape {
            (pg.height, pg.width)
        } else {
            (pg.width, pg.height)
        };
        let mut papers: Vec<String> = PAPER_SIZES.iter().map(|(n, _, _)| n.to_string()).collect();
        let paper = PAPER_SIZES
            .iter()
            .position(|(_, w, h)| (w - pw).abs() < 0.05 && (h - ph).abs() < 0.05)
            .unwrap_or_else(|| {
                papers.push(format!("Custom {}\" x {}\"", trim_num(pw), trim_num(ph)));
                papers.len() - 1
            });
        let opts = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let heading = |s: &str| Field::new(s, "", FVal::Heading);
        let menu = d.styles.menu();
        let style_names: Vec<String> = menu.iter().map(|s| display_style_name(&s.name)).collect();
        let style_ix = menu
            .iter()
            .position(|s| s.id == p.style || (p.style.is_empty() && d.styles.is_default(&s.id)))
            .unwrap_or(0);
        let lines = || opts(&["0", "1", "2", "3"]);
        let fields = vec![
            heading("Line (current paragraph or block)"),
            Field::new(
                "Justification",
                "align",
                FVal::Choice(opts(&Align::NAMES), p.align.index()),
            ),
            Field::new(
                "Line spacing",
                "spacing",
                FVal::Choice(opts(&["1", "2"]), (p.spacing >= 2) as usize),
            ),
            Field::new(
                "Blank lines before",
                "before",
                FVal::Choice(lines(), p.space_before.min(3) as usize),
            ),
            Field::new(
                "Blank lines after",
                "after",
                FVal::Choice(lines(), p.space_after.min(3) as usize),
            ),
            Field::new("Left indent", "indent", FVal::Num(p.indent as f32 / CPI)),
            Field::new(
                "First line indent",
                "first",
                FVal::Num(p.first_indent as f32 / CPI),
            ),
            Field::new(
                "Paragraph style",
                "style",
                FVal::Choice(style_names, style_ix),
            ),
            heading("Page"),
            Field::new("Paper size", "paper", FVal::Choice(papers, paper)),
            Field::new(
                "Orientation",
                "orient",
                FVal::Choice(opts(&["Portrait", "Landscape"]), landscape as usize),
            ),
            Field::new("Left margin", "ml", FVal::Num(pg.margin_left)),
            Field::new("Right margin", "mr", FVal::Num(pg.margin_right)),
            Field::new("Top margin", "mt", FVal::Num(pg.margin_top)),
            Field::new("Bottom margin", "mb", FVal::Num(pg.margin_bottom)),
            Field::new(
                "Page numbering",
                "pagenum",
                FVal::Choice(opts(&PageNumPos::NAMES), pg.page_numbers.index()),
            ),
            heading("Document"),
            Field::new(
                "Header (Enter to edit)",
                "header",
                FVal::Text(hf_summary(&d.header)),
            ),
            Field::new(
                "Footer (Enter to edit)",
                "footer",
                FVal::Text(hf_summary(&d.footer)),
            ),
        ];
        self.mode = Mode::Form(Form {
            title: "Format".into(),
            fields,
            sel: 1,
            edit: None,
            act: FormAct::Format,
        });
    }

    /// The look and formatting of text at the cursor (styles applied).
    pub fn cursor_look(&self) -> (crate::props::CharLook, Fmt) {
        let doc = &self.ed.doc;
        let st = doc.store.borrow();
        let ps = doc.styles.get(&self.ed.para().props.style);
        let run = st.look(self.ed.typing_cp).clone();
        let cs = run.char_style.as_deref().and_then(|id| doc.styles.get(id));
        let mut look = run;
        if let Some(cs) = cs {
            look = look.over(st.look(cs.cp));
        }
        if let Some(ps) = ps {
            look = look.over(st.look(ps.cp));
        }
        look = look.over(st.look(doc.styles.base_cp));
        let sfmt = ps
            .map(|s| s.fmt)
            .unwrap_or_default()
            .union(cs.map(|s| s.fmt).unwrap_or_default());
        (look, Fmt::effective(sfmt, self.ed.typing_fmt))
    }

    fn open_font_form(&mut self) {
        use crate::props::{COLORS, HIGHLIGHTS};
        let (look, fmt) = self.cursor_look();
        let font = look
            .font
            .as_deref()
            .map(|f| self.ed.doc.font_name(f))
            .unwrap_or_default();
        let size = look
            .size
            .map(|h| trim_num(h as f32 / 2.0))
            .unwrap_or_default();
        let mut colors: Vec<String> = std::iter::once("Automatic".to_string())
            .chain(COLORS.iter().map(|(n, _)| n.to_string()))
            .collect();
        let color_ix = match look.rgb() {
            None => 0,
            Some(c) => COLORS
                .iter()
                .position(|(_, v)| *v == c)
                .map(|i| i + 1)
                .unwrap_or_else(|| {
                    colors.push(format!("#{c:06X}"));
                    colors.len() - 1
                }),
        };
        let mut hls: Vec<String> = std::iter::once("None".to_string())
            .chain(HIGHLIGHTS.iter().map(|(_, n, _)| n.to_string()))
            .collect();
        let hl_ix = match look.background() {
            None => 0,
            Some(c) => HIGHLIGHTS
                .iter()
                .position(|(_, _, v)| *v == c)
                .map(|i| i + 1)
                .unwrap_or_else(|| {
                    hls.push(format!("#{c:06X} (shading)"));
                    hls.len() - 1
                }),
        };
        let yn = |on: bool| FVal::Choice(vec!["No".into(), "Yes".into()], on as usize);
        let pos = if fmt.has(Fmt::SUPER) {
            1
        } else if fmt.has(Fmt::SUB) {
            2
        } else {
            0
        };
        let what = if self.ed.sel_range().is_some() {
            "selected text"
        } else {
            "new typing"
        };
        let fields = vec![
            Field::new(format!("Applies to {what}"), "", FVal::Heading),
            Field::new("Font", "font", FVal::Text(font)),
            Field::new("Size (points)", "size", FVal::Text(size)),
            Field::new("Color", "color", FVal::Choice(colors, color_ix)),
            Field::new("Highlight", "hl", FVal::Choice(hls, hl_ix)),
            Field::new("Bold", "b", yn(fmt.has(Fmt::BOLD))),
            Field::new("Italic", "i", yn(fmt.has(Fmt::ITALIC))),
            Field::new("Underline", "u", yn(fmt.has(Fmt::UNDERLINE))),
            Field::new("Strikeout", "s", yn(fmt.has(Fmt::STRIKE))),
            Field::new(
                "Position",
                "pos",
                FVal::Choice(
                    vec!["Normal".into(), "Superscript".into(), "Subscript".into()],
                    pos,
                ),
            ),
            Field::new("Small caps", "sc", yn(look.small_caps == Some(true))),
            Field::new("All caps", "caps", yn(look.caps == Some(true))),
        ];
        self.mode = Mode::Form(Form {
            title: "Font".into(),
            fields,
            sel: 1,
            edit: None,
            act: FormAct::Font,
        });
    }

    fn open_caption_form(&mut self) {
        let in_table = self.ed.cur.pr.cell.is_some();
        let mut labels: Vec<String> = vec!["Figure".into(), "Table".into(), "Equation".into()];
        for l in self.ed.caption_labels() {
            if !labels.iter().any(|x| x.eq_ignore_ascii_case(&l)) {
                labels.push(l);
            }
        }
        let opts = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let fields = vec![
            Field::new("Caption", "", FVal::Heading),
            Field::new("Caption text", "text", FVal::Text(String::new())),
            Field::new(
                "Label",
                "label",
                FVal::Choice(labels, usize::from(in_table)),
            ),
            Field::new(
                "New label (overrides Label)",
                "newlabel",
                FVal::Text(String::new()),
            ),
            Field::new(
                "Position",
                "pos",
                FVal::Choice(
                    opts(&["Below selected item", "Above selected item"]),
                    usize::from(in_table),
                ),
            ),
            Field::new(
                "Exclude label from caption",
                "exclude",
                FVal::Choice(opts(&["No", "Yes"]), 0),
            ),
            Field::new(
                "Numbering",
                "numfmt",
                FVal::Choice(opts(&CAPTION_FORMATS.map(|f| f.0)), 0),
            ),
        ];
        self.mode = Mode::Form(Form {
            title: "Insert Caption".into(),
            fields,
            sel: 1,
            edit: None,
            act: FormAct::Caption,
        });
    }

    /// The caption a Caption dialog would insert (for its preview).
    pub fn caption_preview(&self, f: &Form) -> String {
        let get = |k: &str| f.fields.iter().find(|x| x.key == k);
        let new = get("newlabel").map(Field::display).unwrap_or_default();
        let label = if new.trim().is_empty() {
            get("label").map(Field::display).unwrap_or_default()
        } else {
            new.trim().to_string()
        };
        let fmt_ix = match get("numfmt").map(|x| &x.val) {
            Some(FVal::Choice(_, i)) => *i,
            _ => 0,
        };
        let ident = label.replace(' ', "_");
        let n = self
            .ed
            .doc
            .store
            .borrow()
            .objects
            .iter()
            .filter(|o| {
                o.label
                    .strip_prefix("Field:SEQ ")
                    .and_then(|r| r.split_whitespace().next())
                    == Some(ident.as_str())
            })
            .count() as u32
            + 1;
        let num = crate::props::format_num(CAPTION_FORMATS[fmt_ix.min(4)].2, n);
        let text = get("text").map(Field::display).unwrap_or_default();
        let exclude = get("exclude").map(Field::display).as_deref() == Some("Yes");
        let mut s = if exclude {
            num
        } else {
            format!("{label} {num}")
        };
        if !text.trim().is_empty() {
            s.push_str(&format!(": {}", text.trim()));
        }
        s
    }

    fn open_props_form(&mut self) {
        let c = &self.ed.doc.core;
        let fields = vec![
            Field::new("Title", "title", FVal::Text(c.title.clone())),
            Field::new("Subject", "subject", FVal::Text(c.subject.clone())),
            Field::new("Author", "author", FVal::Text(c.author.clone())),
            Field::new("Keywords", "keywords", FVal::Text(c.keywords.clone())),
            Field::new("Comments", "description", FVal::Text(c.description.clone())),
        ];
        self.mode = Mode::Form(Form {
            title: "Document Properties".into(),
            fields,
            sel: 0,
            edit: None,
            act: FormAct::Props,
        });
    }

    fn follow_link(&mut self, url: &str) {
        if let Some(name) = url.strip_prefix('#') {
            let want = format!("Bookmark:{name}");
            let st = self.ed.doc.store.clone();
            for pr in self.ed.doc.refs() {
                if let Some(k) = self
                    .ed
                    .doc
                    .para(pr)
                    .cells
                    .iter()
                    .position(|c| st.borrow().obj(c.ch).is_some_and(|o| o.label == want))
                {
                    self.ed.move_to(Pos::new(pr, k), false);
                    self.follow = true;
                    return;
                }
            }
            return self.say(format!("Bookmark not found: {name}"));
        }
        if !(url.starts_with("http://")
            || url.starts_with("https://")
            || url.starts_with("mailto:"))
        {
            return self.say(format!("Link: {url}"));
        }
        match crate::platform::open(url) {
            Ok(()) => self.say(format!("Opening {url}")),
            Err(_) => self.say(format!("Link: {url}")),
        }
    }

    fn open_settings_form(&mut self) {
        let opts = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let fields = vec![
            Field::new(
                "Keyboard",
                "keymap",
                FVal::Choice(
                    opts(&["Classic (function keys)", "Modern (Ctrl keys)"]),
                    (self.cfg.keymap == Keymap::Modern) as usize,
                ),
            ),
            Field::new(
                "Colors",
                "theme",
                FVal::Choice(
                    Theme::ALL.iter().map(|t| t.name().to_string()).collect(),
                    Theme::ALL
                        .iter()
                        .position(|t| *t == self.cfg.theme)
                        .unwrap_or(0),
                ),
            ),
            Field::new(
                "Spelling language",
                "lang",
                FVal::Text(self.cfg.lang.clone()),
            ),
            Field::new(
                "Your name (comments, changes)",
                "author",
                FVal::Text(self.cfg.author.clone()),
            ),
            Field::new(
                "Paper outline",
                "outline",
                FVal::Choice(opts(&["On", "Off"]), (!self.cfg.outline) as usize),
            ),
            Field::new(
                "Pages",
                "pages",
                FVal::Choice(
                    opts(&["Side by side when there's room", "One at a time"]),
                    (!self.cfg.side_by_side) as usize,
                ),
            ),
            Field::new(
                "Pictures",
                "pictures",
                FVal::Choice(
                    opts(&[
                        "Full size (graphics if the terminal can)",
                        "Placeholders only",
                    ]),
                    (!self.cfg.pictures) as usize,
                ),
            ),
        ];
        self.mode = Mode::Form(Form {
            title: "Setup".into(),
            fields,
            sel: 0,
            edit: None,
            act: FormAct::Settings,
        });
    }

    fn form_key(&mut self, k: KeyEvent, f: &mut Form) -> bool {
        if let Some((buf, cur)) = &mut f.edit {
            match k.code {
                KeyCode::Esc => f.edit = None,
                KeyCode::Enter | KeyCode::Tab => {
                    let s: String = buf.iter().collect();
                    match &mut f.fields[f.sel].val {
                        FVal::Text(t) => *t = s,
                        FVal::Num(v) => match s.trim().trim_end_matches('"').parse::<f32>() {
                            Ok(n) if (-6.0..=30.0).contains(&n) => *v = n,
                            _ => self.say("Enter a measurement in inches, e.g. 1 or 0.5"),
                        },
                        _ => {}
                    }
                    f.edit = None;
                }
                KeyCode::Left => *cur = cur.saturating_sub(1),
                KeyCode::Right => *cur = (*cur + 1).min(buf.len()),
                KeyCode::Home => *cur = 0,
                KeyCode::End => *cur = buf.len(),
                KeyCode::Backspace => {
                    if *cur > 0 {
                        *cur -= 1;
                        buf.remove(*cur);
                    }
                }
                KeyCode::Delete => {
                    if *cur < buf.len() {
                        buf.remove(*cur);
                    }
                }
                KeyCode::Char(ch) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    buf.insert(*cur, ch);
                    *cur += 1;
                }
                _ => {}
            }
            return true;
        }
        let selectable: Vec<usize> = f
            .fields
            .iter()
            .enumerate()
            .filter(|(_, x)| !matches!(x.val, FVal::Heading))
            .map(|(i, _)| i)
            .collect();
        let pos = selectable.iter().position(|&i| i == f.sel).unwrap_or(0);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => return false,
            KeyCode::F(7) | KeyCode::F(10) => {
                self.apply_form(f);
                return false;
            }
            KeyCode::Char('s') if ctrl => {
                self.apply_form(f);
                return false;
            }
            KeyCode::Up | KeyCode::BackTab => {
                f.sel = selectable[(pos + selectable.len() - 1) % selectable.len()]
            }
            KeyCode::Down | KeyCode::Tab => f.sel = selectable[(pos + 1) % selectable.len()],
            KeyCode::Home | KeyCode::PageUp => f.sel = selectable[0],
            KeyCode::End | KeyCode::PageDown => f.sel = *selectable.last().unwrap(),
            KeyCode::Char(d) if d.is_ascii_digit() || d.is_ascii_lowercase() => {
                // Fields are numbered 1-9 then a-z.
                let idx = if d.is_ascii_digit() {
                    d as usize - '1' as usize
                } else {
                    9 + (d as usize - 'a' as usize)
                };
                if d != '0' && idx < selectable.len() {
                    f.sel = selectable[idx];
                    if f.act == FormAct::Font && f.fields[f.sel].key == "font" {
                        self.open_font_pick(f);
                        return false;
                    }
                    if f.act == FormAct::Format
                        && matches!(f.fields[f.sel].key, "header" | "footer")
                    {
                        let footer = f.fields[f.sel].key == "footer";
                        let form = std::mem::replace(
                            f,
                            Form {
                                title: String::new(),
                                fields: Vec::new(),
                                sel: 0,
                                edit: None,
                                act: FormAct::Format,
                            },
                        );
                        self.open_hf_editor(footer, Some(Box::new(form)));
                        return false;
                    }
                    self.form_activate(f, 1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right
                if f.act == FormAct::Format
                    && f.fields
                        .get(f.sel)
                        .is_some_and(|x| x.key == "header" || x.key == "footer") =>
            {
                let footer = f.fields[f.sel].key == "footer";
                let form = std::mem::replace(
                    f,
                    Form {
                        title: String::new(),
                        fields: Vec::new(),
                        sel: 0,
                        edit: None,
                        act: FormAct::Format,
                    },
                );
                self.open_hf_editor(footer, Some(Box::new(form)));
                return false;
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right
                if f.act == FormAct::Font
                    && f.fields.get(f.sel).is_some_and(|x| x.key == "font") =>
            {
                self.open_font_pick(f);
                return false;
            }
            KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Right => self.form_activate(f, 1),
            KeyCode::Left => self.form_activate(f, -1),
            _ => {}
        }
        true
    }

    /// Fonts the document uses, then the installed ones.
    fn font_choices(&mut self) -> (Vec<String>, usize) {
        if self.installed_fonts.is_none() {
            self.installed_fonts = Some(crate::props::installed_fonts());
        }
        let doc = &self.ed.doc;
        let mut used: Vec<String> = Vec::new();
        {
            let st = doc.store.borrow();
            for l in &st.looks {
                if let Some(f) = &l.font {
                    let name = doc.font_name(f);
                    if !used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
                        used.push(name);
                    }
                }
            }
        }
        used.sort_by_key(|f| f.to_lowercase());
        let n = used.len();
        // Installed fonts and common document fonts, alphabetically.
        let installed = self.installed_fonts.clone().unwrap_or_default();
        let mut rest: Vec<String> = installed.clone();
        for f in crate::props::COMMON_FONTS {
            if !installed.iter().any(|i| i.eq_ignore_ascii_case(f)) {
                rest.push(f.to_string());
            }
        }
        rest.sort_by_key(|f| f.to_lowercase());
        let mut all = used;
        for f in rest {
            if !all[..n].iter().any(|u| u.eq_ignore_ascii_case(&f)) {
                all.push(f);
            }
        }
        (all, n)
    }

    fn open_hf_editor(&mut self, footer: bool, back: Option<Box<Form>>) {
        let src = if footer {
            &self.ed.doc.footer
        } else {
            &self.ed.doc.header
        };
        let mut lines: Vec<(Vec<char>, Align)> = crate::print::hf_lines(src)
            .into_iter()
            .map(|(t, a)| (t.chars().collect(), a))
            .collect();
        if lines.is_empty() {
            lines.push((Vec::new(), if footer { Align::Center } else { Align::Left }));
        }
        let col = lines[0].0.len();
        self.mode = Mode::HfEdit(HfEdit {
            footer,
            lines,
            row: 0,
            col,
            back,
        });
    }

    fn hf_key(&mut self, k: KeyEvent, h: &mut HfEdit) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let close = |app: &mut App, h: &mut HfEdit| {
            if let Some(mut form) = h.back.take() {
                let (hd, ft) = (app.ed.doc.header.clone(), app.ed.doc.footer.clone());
                for f in form.fields.iter_mut() {
                    match f.key {
                        "header" => f.val = FVal::Text(hf_summary(&hd)),
                        "footer" => f.val = FVal::Text(hf_summary(&ft)),
                        _ => {}
                    }
                }
                app.mode = Mode::Form(*form);
            }
        };
        let insert = |h: &mut HfEdit, s: &str| {
            let (line, _) = &mut h.lines[h.row];
            for (i, c) in s.chars().enumerate() {
                line.insert(h.col + i, c);
            }
            h.col += s.chars().count();
        };
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => {
                close(self, h);
                return false;
            }
            KeyCode::F(10) | KeyCode::F(7) => {}
            KeyCode::Char('s') if ctrl => {}
            KeyCode::Char('p') if ctrl => {
                insert(h, "{page}");
                return true;
            }
            KeyCode::Char('n') if ctrl => {
                insert(h, "{pages}");
                return true;
            }
            KeyCode::Char('d') if ctrl => {
                insert(h, "{date}");
                return true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let back = k.code == KeyCode::BackTab || k.modifiers.contains(KeyModifiers::SHIFT);
                let a = &mut h.lines[h.row].1;
                *a = match (*a, back) {
                    (Align::Left, false) | (Align::Right, true) => Align::Center,
                    (Align::Center, false) | (Align::Left, true) => Align::Right,
                    _ => Align::Left,
                };
                return true;
            }
            KeyCode::Enter => {
                if h.lines.len() < 12 {
                    let a = h.lines[h.row].1;
                    let rest = h.lines[h.row].0.split_off(h.col);
                    h.lines.insert(h.row + 1, (rest, a));
                    h.row += 1;
                    h.col = 0;
                }
                return true;
            }
            KeyCode::Backspace => {
                if h.col > 0 {
                    h.col -= 1;
                    h.lines[h.row].0.remove(h.col);
                } else if h.row > 0 {
                    let (line, _) = h.lines.remove(h.row);
                    h.row -= 1;
                    h.col = h.lines[h.row].0.len();
                    h.lines[h.row].0.extend(line);
                }
                return true;
            }
            KeyCode::Delete => {
                if h.col < h.lines[h.row].0.len() {
                    h.lines[h.row].0.remove(h.col);
                } else if h.row + 1 < h.lines.len() {
                    let (line, _) = h.lines.remove(h.row + 1);
                    h.lines[h.row].0.extend(line);
                }
                return true;
            }
            KeyCode::Left => {
                if h.col > 0 {
                    h.col -= 1;
                } else if h.row > 0 {
                    h.row -= 1;
                    h.col = h.lines[h.row].0.len();
                }
                return true;
            }
            KeyCode::Right => {
                if h.col < h.lines[h.row].0.len() {
                    h.col += 1;
                } else if h.row + 1 < h.lines.len() {
                    h.row += 1;
                    h.col = 0;
                }
                return true;
            }
            KeyCode::Up => {
                h.row = h.row.saturating_sub(1);
                h.col = h.col.min(h.lines[h.row].0.len());
                return true;
            }
            KeyCode::Down => {
                h.row = (h.row + 1).min(h.lines.len() - 1);
                h.col = h.col.min(h.lines[h.row].0.len());
                return true;
            }
            KeyCode::Home => {
                h.col = 0;
                return true;
            }
            KeyCode::End => {
                h.col = h.lines[h.row].0.len();
                return true;
            }
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                insert(h, &c.to_string());
                return true;
            }
            _ => return true,
        }
        // Save.
        let text = crate::print::hf_join(&h.parts());
        let footer = h.footer;
        let changed = if footer {
            self.ed.doc.footer != text
        } else {
            self.ed.doc.header != text
        };
        if changed {
            self.ed.edit_doc(|d| {
                if footer {
                    d.footer = text
                } else {
                    d.header = text
                }
            });
        }
        let n = crate::print::hf_lines(if footer {
            &self.ed.doc.footer
        } else {
            &self.ed.doc.header
        })
        .len();
        self.say(format!(
            "{} saved ({n} line{})",
            if footer { "Footer" } else { "Header" },
            if n == 1 { "" } else { "s" }
        ));
        close(self, h);
        false
    }

    fn open_font_pick(&mut self, f: &mut Form) {
        let form = std::mem::replace(
            f,
            Form {
                title: String::new(),
                fields: Vec::new(),
                sel: 0,
                edit: None,
                act: FormAct::Font,
            },
        );
        let (fonts, in_doc) = self.font_choices();
        // Which of the listed fonts aren't installed, and what shows instead.
        let installed = self.installed_fonts.clone().unwrap_or_default();
        let missing: Vec<&str> = fonts
            .iter()
            .filter(|f| !installed.iter().any(|i| i.eq_ignore_ascii_case(f)))
            .map(String::as_str)
            .collect();
        let known = self.font_subs.get_or_insert_with(Default::default);
        let todo: Vec<&str> = missing
            .iter()
            .copied()
            .filter(|f| !known.contains_key(*f))
            .collect();
        known.extend(crate::props::font_substitutes(&todo));
        let subs = missing
            .iter()
            .filter_map(|f| known.get(*f).map(|s| (f.to_string(), s.clone())))
            .collect();
        self.mode = Mode::FontPick(FontPick {
            form: Box::new(form),
            query: String::new(),
            sel: 0,
            fonts,
            in_doc,
            subs,
        });
    }

    fn font_pick_key(&mut self, k: KeyEvent, fp: &mut FontPick) -> bool {
        let n = fp.items().len();
        let page = self.geo.height.saturating_sub(10).max(1) as usize;
        let back = |fp: &mut FontPick, value: Option<String>| -> Form {
            let mut form = std::mem::replace(
                &mut *fp.form,
                Form {
                    title: String::new(),
                    fields: Vec::new(),
                    sel: 0,
                    edit: None,
                    act: FormAct::Font,
                },
            );
            if let Some(v) = value
                && let Some(fld) = form.fields.iter_mut().find(|x| x.key == "font")
            {
                fld.val = FVal::Text(v);
            }
            form
        };
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => {
                self.mode = Mode::Form(back(fp, None));
                return false;
            }
            KeyCode::Enter => {
                let items = fp.items();
                let chosen = match items.get(fp.sel) {
                    Some(Some(name)) => (*name).clone(),
                    Some(None) => String::new(),
                    // Nothing matches: use the name as typed.
                    None => fp.query.trim().to_string(),
                };
                self.mode = Mode::Form(back(fp, Some(chosen)));
                return false;
            }
            KeyCode::Up | KeyCode::BackTab => fp.sel = fp.sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => fp.sel = (fp.sel + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => fp.sel = fp.sel.saturating_sub(page),
            KeyCode::PageDown => fp.sel = (fp.sel + page).min(n.saturating_sub(1)),
            KeyCode::Home => fp.sel = 0,
            KeyCode::End => fp.sel = n.saturating_sub(1),
            KeyCode::Backspace => {
                fp.query.pop();
                fp.sel = 0;
            }
            KeyCode::Char(ch)
                if !k
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                fp.query.push(ch);
                fp.sel = 0;
            }
            _ => {}
        }
        true
    }

    fn form_activate(&mut self, f: &mut Form, dir: i32) {
        match &mut f.fields[f.sel].val {
            FVal::Choice(opts, i) => *i = (*i as i32 + dir).rem_euclid(opts.len() as i32) as usize,
            FVal::Text(s) => {
                let b: Vec<char> = s.chars().collect();
                let n = b.len();
                f.edit = Some((b, n));
            }
            FVal::Num(v) => {
                let b: Vec<char> = trim_num(*v).chars().collect();
                let n = b.len();
                f.edit = Some((b, n));
            }
            FVal::Heading => {}
        }
    }

    fn apply_form(&mut self, f: &Form) {
        let choice = |k: &str| {
            f.fields
                .iter()
                .find(|x| x.key == k)
                .and_then(|x| match &x.val {
                    FVal::Choice(o, i) => Some((*i, o[*i].clone())),
                    _ => None,
                })
        };
        let num = |k: &str| {
            f.fields
                .iter()
                .find(|x| x.key == k)
                .and_then(|x| match x.val {
                    FVal::Num(v) => Some(v),
                    _ => None,
                })
        };
        let text = |k: &str| {
            f.fields
                .iter()
                .find(|x| x.key == k)
                .and_then(|x| match &x.val {
                    FVal::Text(s) => Some(s.clone()),
                    _ => None,
                })
        };
        match f.act {
            FormAct::Settings => {
                self.cfg.keymap = if choice("keymap").map(|c| c.0) == Some(1) {
                    Keymap::Modern
                } else {
                    Keymap::Classic
                };
                self.cfg.theme = Theme::ALL[choice("theme")
                    .map(|c| c.0)
                    .unwrap_or(0)
                    .min(Theme::ALL.len() - 1)];
                let lang = text("lang").unwrap_or_default().trim().to_string();
                if !lang.is_empty() && lang != self.cfg.lang {
                    self.cfg.lang = lang;
                    self.speller = None;
                }
                self.cfg.outline = choice("outline").map(|c| c.0) != Some(1);
                self.cfg.side_by_side = choice("pages").map(|c| c.0) != Some(1);
                let author = text("author").unwrap_or_default().trim().to_string();
                if !author.is_empty() {
                    self.cfg.author = author.clone();
                    self.ed.author = author;
                }
                let pictures = choice("pictures").map(|c| c.0) != Some(1);
                if pictures != self.cfg.pictures {
                    self.cfg.pictures = pictures;
                    crate::layout::set_full_size_pictures(pictures);
                    self.layout = Layout::build(&self.ed.doc);
                }
                match self.cfg.save() {
                    Ok(()) => self.say("Settings saved"),
                    Err(e) => self.say(format!("Could not save settings: {e}")),
                }
            }
            FormAct::Caption => {
                let new = text("newlabel").unwrap_or_default();
                let label = if new.trim().is_empty() {
                    choice("label")
                        .map(|c| c.1)
                        .unwrap_or_else(|| "Figure".into())
                } else {
                    new.trim().to_string()
                };
                let above = choice("pos").map(|c| c.0) == Some(1);
                let exclude = choice("exclude").map(|c| c.0) == Some(1);
                let fmt = CAPTION_FORMATS[choice("numfmt").map(|c| c.0).unwrap_or(0).min(4)].1;
                let preview = self.caption_preview(f);
                self.ed.insert_caption(
                    &label,
                    &text("text").unwrap_or_default(),
                    above,
                    fmt,
                    exclude,
                );
                self.say(format!("Inserted caption: {preview}"));
            }
            FormAct::Props => {
                let core = CoreProps {
                    title: text("title").unwrap_or_default(),
                    subject: text("subject").unwrap_or_default(),
                    author: text("author").unwrap_or_default(),
                    keywords: text("keywords").unwrap_or_default(),
                    description: text("description").unwrap_or_default(),
                };
                if core != self.ed.doc.core {
                    self.ed.edit_doc(|d| d.core = core);
                }
            }
            FormAct::Font => {
                use crate::props::{COLORS, CharEdit, HIGHLIGHTS};
                let field = |k: &str| f.fields.iter().find(|x| x.key == k);
                let changed = |k: &str| field(k).is_some_and(Field::changed);
                let mut edit = CharEdit::default();
                if changed("font") {
                    let t = text("font").unwrap_or_default().trim().to_string();
                    edit.font = Some((!t.is_empty()).then_some(t));
                }
                if changed("size") {
                    let t = text("size").unwrap_or_default();
                    match t.trim().trim_end_matches("pt").trim() {
                        "" => edit.size = Some(None),
                        v => match v.parse::<f32>() {
                            Ok(pt) if (1.0..=1638.0).contains(&pt) => {
                                edit.size = Some(Some((pt * 2.0).round() as u16))
                            }
                            _ => self.say("Enter a size in points, e.g. 11 or 12.5"),
                        },
                    }
                }
                if changed("color")
                    && let Some((i, name)) = choice("color")
                {
                    edit.color = Some(match i {
                        0 => None,
                        i if i <= COLORS.len() => Some(COLORS[i - 1].1),
                        _ => crate::props::parse_hex(&name),
                    });
                }
                if changed("hl")
                    && let Some((i, _)) = choice("hl")
                {
                    edit.highlight =
                        Some((i > 0 && i <= HIGHLIGHTS.len()).then(|| HIGHLIGHTS[i - 1].0));
                }
                if changed("sc") {
                    edit.small_caps = choice("sc").map(|c| c.0 == 1);
                }
                if changed("caps") {
                    edit.caps = choice("caps").map(|c| c.0 == 1);
                }
                self.ed.apply_char_edit(&edit);
                for (k, flag) in [
                    ("b", Fmt::BOLD),
                    ("i", Fmt::ITALIC),
                    ("u", Fmt::UNDERLINE),
                    ("s", Fmt::STRIKE),
                ] {
                    if changed(k) {
                        self.ed
                            .set_fmt(flag, choice(k).map(|c| c.0 == 1).unwrap_or(false));
                    }
                }
                if changed("pos") {
                    match choice("pos").map(|c| c.0) {
                        Some(1) => self.ed.set_fmt(Fmt::SUPER, true),
                        Some(2) => self.ed.set_fmt(Fmt::SUB, true),
                        _ => {
                            self.ed.set_fmt(Fmt::SUPER, false);
                            self.ed.set_fmt(Fmt::SUB, false);
                        }
                    }
                }
                self.block_mode = false;
            }
            FormAct::Format => {
                let changed = |k: &str| {
                    f.fields
                        .iter()
                        .find(|x| x.key == k)
                        .is_some_and(Field::changed)
                };
                if changed("style") {
                    let menu = self
                        .ed
                        .doc
                        .styles
                        .menu()
                        .iter()
                        .map(|s| s.id.clone())
                        .collect::<Vec<_>>();
                    if let Some(id) = choice("style").and_then(|(i, _)| menu.get(i).cloned()) {
                        self.ed.apply_style(&id);
                    }
                }
                let align = Align::from_index(choice("align").map(|c| c.0).unwrap_or(0));
                let spacing = if choice("spacing").map(|c| c.0) == Some(1) {
                    2
                } else {
                    1
                };
                let before = choice("before").map(|c| c.0 as u8).unwrap_or(0);
                let after = choice("after").map(|c| c.0 as u8).unwrap_or(0);
                let indent = (num("indent").unwrap_or(0.0) * CPI)
                    .round()
                    .clamp(0.0, 60.0) as u16;
                let first = (num("first").unwrap_or(0.0) * CPI)
                    .round()
                    .clamp(-60.0, 60.0) as i16;
                let keys = ["align", "spacing", "before", "after", "indent", "first"];
                if keys.iter().any(|k| changed(k)) {
                    let (ca, cs, cb, caf, ci, cf) = (
                        changed("align"),
                        changed("spacing"),
                        changed("before"),
                        changed("after"),
                        changed("indent"),
                        changed("first"),
                    );
                    self.ed.set_props(|p| {
                        if ca {
                            p.align = align;
                        }
                        if cs {
                            p.spacing = spacing;
                        }
                        if cb {
                            p.space_before = before;
                        }
                        if caf {
                            p.space_after = after;
                        }
                        if ci {
                            p.indent = indent;
                        }
                        if cf {
                            p.first_indent = first;
                        }
                    });
                }
                let old_pg = self.ed.doc.page.clone();
                let mut pg = old_pg.clone();
                let landscape = choice("orient").map(|c| c.0) == Some(1);
                let (mut w, mut h) = if let Some((_, name)) = choice("paper") {
                    match PAPER_SIZES.iter().find(|(n, _, _)| *n == name) {
                        Some((_, w, h)) => (*w, *h),
                        None => (
                            old_pg.width.min(old_pg.height),
                            old_pg.width.max(old_pg.height),
                        ),
                    }
                } else {
                    (old_pg.width, old_pg.height)
                };
                if landscape {
                    std::mem::swap(&mut w, &mut h);
                }
                pg.width = w;
                pg.height = h;
                pg.margin_left = num("ml").unwrap_or(1.0).clamp(0.0, w / 2.0 - 1.0);
                pg.margin_right = num("mr").unwrap_or(1.0).clamp(0.0, w / 2.0 - 1.0);
                pg.margin_top = num("mt").unwrap_or(1.0).clamp(0.0, h / 2.0 - 1.0);
                pg.margin_bottom = num("mb").unwrap_or(1.0).clamp(0.0, h / 2.0 - 1.0);
                pg.page_numbers =
                    PageNumPos::from_index(choice("pagenum").map(|c| c.0).unwrap_or(0));
                // (Header and footer are saved by their own editor.)
                if pg != old_pg {
                    self.ed.edit_doc(|d| d.page = pg);
                }
            }
        }
    }

    // ----- spell check ---------------------------------------------------

    fn start_spell(&mut self) {
        if self.speller.is_none() {
            match Speller::new(&self.cfg.lang) {
                Ok(s) => self.speller = Some(s),
                Err(e) => return self.say(e),
            }
        }
        self.spell_checked = 0;
        let start = match self.ed.sel_range() {
            Some((a, _)) => a,
            None => self.ed.doc_start(),
        };
        self.block_mode = false;
        self.spell_from(start);
    }

    fn spell_from(&mut self, start: Pos) {
        let refs = self.ed.doc.refs();
        let i0 = refs.iter().position(|r| *r == start.pr).unwrap_or(0);
        let Some(sp) = self.speller.as_mut() else {
            return;
        };
        for &pr in &refs[i0..] {
            let cells = &self.ed.doc.para(pr).cells;
            let n = cells.len();
            let mut k = if pr == start.pr { start.off.min(n) } else { 0 };
            // Don't start in the middle of a word.
            while k > 0
                && k < n
                && spell_word_char(cells[k - 1].ch)
                && cells[k - 1].ch.is_alphabetic()
            {
                k += 1;
            }
            while k < n {
                if !cells[k].ch.is_alphabetic() {
                    k += 1;
                    continue;
                }
                let s = k;
                while k < n && spell_word_char(cells[k].ch) {
                    k += 1;
                }
                let mut e = k;
                while e > s && !cells[e - 1].ch.is_alphabetic() {
                    e -= 1;
                }
                // Skip words glued to digits (e.g. "3rd", "A4").
                if (s > 0 && cells[s - 1].ch.is_numeric()) || (k < n && cells[k].ch.is_numeric()) {
                    while k < n && cells[k].ch.is_alphanumeric() {
                        k += 1;
                    }
                    continue;
                }
                let word: String = cells[s..e].iter().map(|c| c.ch).collect();
                self.spell_checked += 1;
                if let Some(sugg) = sp.check(&word) {
                    self.ed.select_range(Pos::new(pr, s), Pos::new(pr, e));
                    self.follow = true;
                    self.mode = Mode::Spell(SpellSt {
                        pr,
                        s,
                        e,
                        word,
                        sugg,
                    });
                    return;
                }
            }
        }
        self.ed.anchor = None;
        let via = self
            .speller
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        self.say(format!(
            "Spell check complete: {} words checked ({via})",
            self.spell_checked
        ));
    }

    fn spell_key(&mut self, k: KeyEvent, st: &mut SpellSt) -> bool {
        let next_from = |s: usize| Pos::new(st.pr, s);
        match k.code {
            KeyCode::Esc | KeyCode::F(1) | KeyCode::F(7) => {
                self.ed.anchor = None;
                self.say("Spell check stopped");
                false
            }
            KeyCode::Char(c @ 'a'..='z') => {
                let i = c as usize - 'a' as usize;
                if let Some(w) = st.sugg.get(i).cloned() {
                    self.ed.replace_in_para(st.pr, st.s, st.e, &w);
                    self.ed.anchor = None;
                    self.spell_from(next_from(st.s + w.chars().count()));
                    return false;
                }
                true
            }
            KeyCode::Char('1') => {
                self.spell_from(next_from(st.e));
                false
            }
            KeyCode::Char('2') => {
                if let Some(sp) = self.speller.as_mut() {
                    sp.ignore(&st.word);
                }
                self.spell_from(next_from(st.e));
                false
            }
            KeyCode::Char('3') => {
                if let Some(sp) = self.speller.as_mut()
                    && let Err(e) = sp.add(&st.word)
                {
                    self.say(format!("Could not save personal dictionary: {e}"));
                }
                self.spell_from(next_from(st.e));
                false
            }
            KeyCode::Char('4') => {
                let w = st.word.clone();
                self.prompt("Edit word: ", &w, PromptAct::SpellEdit(st.pr, st.s, st.e));
                false
            }
            _ => true,
        }
    }

    // ----- thesaurus -----------------------------------------------------

    fn start_thesaurus(&mut self) {
        let Some((s, e, word)) = self.ed.word_at_cursor() else {
            return self.say("Put the cursor on a word to look it up");
        };
        if self.thesaurus.is_none() {
            match Thesaurus::new() {
                Ok(t) => self.thesaurus = Some(t),
                Err(e) => return self.say(e),
            }
        }
        let pr = self.ed.cur.pr;
        self.show_thesaurus(pr, s, e, word);
    }

    fn show_thesaurus(&mut self, pr: ParaRef, s: usize, e: usize, word: String) {
        let senses = self
            .thesaurus
            .as_ref()
            .map(|t| t.lookup(&word))
            .unwrap_or_default();
        if senses.is_empty() {
            return self.say(format!("Word not found in thesaurus: {word}"));
        }
        let mut rows = Vec::new();
        for (pos, syns) in senses {
            rows.push((
                true,
                if pos.is_empty() {
                    word.clone()
                } else {
                    format!("{word} ({pos})")
                },
            ));
            rows.extend(syns.into_iter().map(|w| (false, w)));
        }
        let sel = rows.iter().position(|r| !r.0).unwrap_or(0);
        self.ed.select_range(Pos::new(pr, s), Pos::new(pr, e));
        self.mode = Mode::Thes(ThesSt {
            pr,
            s,
            e,
            word,
            rows,
            sel,
        });
    }

    fn thes_key(&mut self, k: KeyEvent, t: &mut ThesSt) -> bool {
        let step = |t: &mut ThesSt, dir: i32| {
            let n = t.rows.len() as i32;
            let mut i = t.sel as i32;
            for _ in 0..n {
                i = (i + dir).rem_euclid(n);
                if !t.rows[i as usize].0 {
                    break;
                }
            }
            t.sel = i as usize;
        };
        match k.code {
            KeyCode::Esc | KeyCode::F(1) | KeyCode::F(7) => {
                self.ed.anchor = None;
                false
            }
            KeyCode::Up => {
                step(t, -1);
                true
            }
            KeyCode::Down => {
                step(t, 1);
                true
            }
            KeyCode::Enter => {
                let w = t.rows[t.sel].1.clone();
                self.ed.replace_in_para(t.pr, t.s, t.e, &w);
                self.ed.anchor = None;
                self.say(format!("Replaced \"{}\" with \"{w}\"", t.word));
                false
            }
            KeyCode::Char('l' | 'L') => {
                let w = t.rows[t.sel].1.clone();
                let (pr, s, e) = (t.pr, t.s, t.e);
                self.show_thesaurus(pr, s, e, w);
                false
            }
            _ => true,
        }
    }

    // ----- replace -------------------------------------------------------

    fn replace_next(&mut self, st: ReplaceSt) {
        match self.ed.find(&st.from, true) {
            FindResult::Found => self.mode = Mode::Replace(st),
            _ => {
                self.ed.anchor = None;
                self.say(format!("{} replacement(s) made", st.count));
            }
        }
    }

    fn replace_key(&mut self, k: KeyEvent, st: &mut ReplaceSt) -> bool {
        let take = |st: &mut ReplaceSt| ReplaceSt {
            from: st.from.clone(),
            to: st.to.clone(),
            count: st.count,
        };
        match k.code {
            KeyCode::Char('y' | 'Y') => {
                self.ed.replace_selection(&st.to);
                st.count += 1;
                self.replace_next(take(st));
                false
            }
            KeyCode::Char('n' | 'N') => {
                self.ed.anchor = None;
                self.replace_next(take(st));
                false
            }
            KeyCode::Char('a' | 'A' | '!') => {
                loop {
                    self.ed.replace_selection(&st.to);
                    st.count += 1;
                    if self.ed.find(&st.from, true) != FindResult::Found {
                        break;
                    }
                }
                self.ed.anchor = None;
                self.say(format!("{} replacement(s) made", st.count));
                false
            }
            KeyCode::Esc | KeyCode::F(1) => {
                self.ed.anchor = None;
                self.say(format!("{} replacement(s) made", st.count));
                false
            }
            _ => true,
        }
    }

    // ----- help and preview ----------------------------------------------

    pub fn help_lines(&self) -> Vec<String> {
        let km = self.cfg.keymap;
        let classic = km == Keymap::Classic;
        let mut v = vec![
            format!(
                "WordBench keyboard: {}   (Alt+K switches keyboards)",
                if classic { "Classic" } else { "Modern" }
            ),
            String::new(),
            "Moving and selecting".into(),
            "  Arrows, Ctrl+Left/Right (word), Home/End (line), Ctrl+Home/End (document)".into(),
            if classic {
                "  PgUp/PgDn (page), Home,Home,Up / Home,Home,Down (top/bottom), Ctrl+End delete to end of line".into()
            } else {
                "  PgUp/PgDn (screen)".into()
            },
            "  Shift+movement selects; Block (see below) then move the cursor also selects".into(),
            "  Tab: next table cell, or demote a list item; Shift+Tab: back / promote".into(),
            "  Ctrl+Up/Down: previous/next paragraph; Shift+Enter: line break; Esc: cancel selection".into(),
            "  Enter on an empty list item ends the list; mouse click/drag and wheel work".into(),
            String::new(),
            "Everything without a mouse".into(),
            "  Alt+X (or Ctrl+Shift+P) lists every command: type part of its name, then Enter".into(),
            format!(
                "  Menus: {}; Alt+F File, E Edit, S Search, O Format, I Insert, T Tools, R Review, V View, H Help",
                if classic { "Alt+= or F9" } else { "F10" }
            ),
            "  In a menu: arrows or the underlined letter; Enter runs; Esc closes".into(),
            "  Ctrl+C/Ctrl+V use the desktop clipboard: copy from a web page, paste here (and back)".into(),
            "  Dialogs: Tab/Shift+Tab or arrows move, Enter/Space changes, F10 saves, Esc cancels".into(),
            String::new(),
        ];
        for (name, items) in MENUS {
            v.push(name.to_string());
            for cmd in items.iter().flatten() {
                let key = commands::key_for(km, *cmd).unwrap_or_else(|| "(menu)".into());
                v.push(format!("  {:<34} {key}", cmd.label()));
            }
            v.push(String::new());
        }
        v.push(format!(
            "Menus: {}. Dialogs: Esc or F1 cancels.",
            if classic {
                "Alt+= or F9 opens the menu bar"
            } else {
                "F10 or Alt+letter opens a menu"
            }
        ));
        v
    }

    /// Commands matching a search, with their menu names.
    pub fn palette_items(&self, query: &str) -> Vec<(Cmd, &'static str)> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let menu_of = |c: Cmd| {
            MENUS
                .iter()
                .find(|(_, items)| items.contains(&Some(c)))
                .map(|(n, _)| *n)
                .unwrap_or("")
        };
        commands::ALL
            .iter()
            .filter(|c| !matches!(c, Cmd::CommandPalette))
            .map(|&c| (c, menu_of(c)))
            .filter(|(c, menu)| {
                let hay = format!("{} {menu}", c.label()).to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect()
    }

    fn theme_pick_key(
        &mut self,
        k: KeyEvent,
        sel: &mut usize,
        original: Theme,
        original_outline: bool,
    ) -> bool {
        let n = Theme::ALL.len();
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => {
                self.cfg.theme = original;
                self.cfg.outline = original_outline;
                self.say("Colors unchanged");
                return false;
            }
            KeyCode::Enter | KeyCode::F(10) => {
                let msg = match self.cfg.save() {
                    Ok(()) => format!("Colors: {}", self.cfg.theme.name()),
                    Err(e) => format!("Could not save settings: {e}"),
                };
                self.say(msg);
                return false;
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Left => *sel = (*sel + n - 1) % n,
            KeyCode::Down | KeyCode::Tab | KeyCode::Right => *sel = (*sel + 1) % n,
            KeyCode::Home | KeyCode::PageUp => *sel = 0,
            KeyCode::End | KeyCode::PageDown => *sel = n - 1,
            KeyCode::Char(d @ '1'..='9') if (d as usize - '1' as usize) < n => {
                *sel = d as usize - '1' as usize
            }
            KeyCode::Char('o' | 'O') => self.cfg.outline = !self.cfg.outline,
            _ => {}
        }
        // Preview right away.
        self.cfg.theme = Theme::ALL[*sel];
        true
    }

    fn palette_key(&mut self, k: KeyEvent, query: &mut String, sel: &mut usize) -> bool {
        let n = self.palette_items(query).len();
        let page = self.geo.height.saturating_sub(8).max(1) as usize;
        match k.code {
            KeyCode::Esc | KeyCode::F(1) => return false,
            KeyCode::Enter => {
                if let Some((cmd, _)) = self.palette_items(query).get(*sel).copied() {
                    self.run(cmd);
                }
                return false;
            }
            KeyCode::Up | KeyCode::BackTab => *sel = sel.saturating_sub(1),
            KeyCode::Down | KeyCode::Tab => *sel = (*sel + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => *sel = sel.saturating_sub(page),
            KeyCode::PageDown => *sel = (*sel + page).min(n.saturating_sub(1)),
            KeyCode::Home => *sel = 0,
            KeyCode::End => *sel = n.saturating_sub(1),
            KeyCode::Backspace => {
                query.pop();
                *sel = 0;
            }
            KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                query.clear();
                *sel = 0;
            }
            KeyCode::Char(ch)
                if !k
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                query.push(ch);
                *sel = 0;
            }
            _ => {}
        }
        true
    }

    fn help_key(&mut self, k: KeyEvent, scroll: &mut usize) -> bool {
        let max = self.help_lines().len();
        match k.code {
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = (*scroll + 1).min(max.saturating_sub(1)),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(self.geo.height as usize / 2),
            KeyCode::PageDown => {
                *scroll = (*scroll + self.geo.height as usize / 2).min(max.saturating_sub(1))
            }
            KeyCode::Home => *scroll = 0,
            _ => return false,
        }
        true
    }

    fn preview_key(
        &mut self,
        k: KeyEvent,
        page: &mut usize,
        scroll: &mut u16,
        hscroll: &mut u16,
        n: usize,
    ) -> bool {
        match k.code {
            KeyCode::PageDown => {
                *page = (*page + 1).min(n.saturating_sub(1));
                *scroll = 0;
            }
            KeyCode::PageUp => {
                *page = page.saturating_sub(1);
                *scroll = 0;
            }
            KeyCode::Home => *page = 0,
            KeyCode::End => *page = n.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(2),
            KeyCode::Up => *scroll = scroll.saturating_sub(2),
            KeyCode::Right => *hscroll = hscroll.saturating_add(4),
            KeyCode::Left => *hscroll = hscroll.saturating_sub(4),
            _ => return false,
        }
        true
    }

    // ===================================================================
    // Mouse

    fn doc_pos_at(&self, col: u16, row: u16) -> Option<Pos> {
        // The sheet under the mouse, then the nearest text line at or above
        // the row on that sheet.
        let sheet = |s: &&crate::ui::LineSpot| col >= s.lo && col < s.hi;
        let spots: Vec<&crate::ui::LineSpot> = self.screen_map.iter().filter(sheet).collect();
        let spots = if spots.is_empty() {
            self.screen_map.iter().collect()
        } else {
            spots
        };
        let best = spots
            .iter()
            .filter(|s| s.y <= row && !self.layout.lines[s.li].segs.is_empty())
            .max_by_key(|s| s.y)
            .or_else(|| {
                spots
                    .iter()
                    .filter(|s| !self.layout.lines[s.li].segs.is_empty())
                    .min_by_key(|s| s.y)
            })?;
        let x = (col as i32 - best.ox).max(0) as u16;
        self.layout.hit(best.li, x)
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        let shift = m.modifiers.contains(KeyModifiers::SHIFT);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Mode::Menu { m: mi, item } = self.mode {
                    let (x, y, w, h) = crate::ui::dropdown_rect(mi);
                    let h = h.min(self.geo.height.saturating_sub(y));
                    if m.column >= x
                        && m.column < x + w
                        && m.row > y
                        && m.row < y + h.saturating_sub(1)
                    {
                        let idx = (m.row - y - 1) as usize
                            + crate::ui::menu_first_row(mi, item, self.geo.height);
                        if let Some(Some(cmd)) = MENUS[mi].1.get(idx) {
                            self.mode = Mode::Normal;
                            self.run(*cmd);
                        }
                        return;
                    }
                }
                if self.geo.menu_bar && m.row == 0 {
                    for (i, menu) in MENUS.iter().enumerate() {
                        let x = menu_title_x(i);
                        if m.column >= x && m.column < x + menu.0.len() as u16 + 2 {
                            self.mode = Mode::Menu {
                                m: i,
                                item: first_item(i),
                            };
                            return;
                        }
                    }
                    self.mode = Mode::Normal;
                    return;
                }
                if matches!(self.mode, Mode::Menu { .. }) {
                    self.mode = Mode::Normal;
                    return;
                }
                if !matches!(self.mode, Mode::Normal) {
                    return;
                }
                if let Some(p) = self.doc_pos_at(m.column, m.row) {
                    self.ed.move_to(p, shift || self.block_mode);
                    self.ed.goal_x = None;
                    self.mouse_down = true;
                    self.follow = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.mouse_down => {
                if m.row < self.geo.doc_y && self.scroll > 0 {
                    self.scroll -= 1;
                } else if m.row >= self.geo.doc_y + self.geo.doc_h {
                    self.scroll += 1;
                }
                let row = m.row.clamp(
                    self.geo.doc_y,
                    (self.geo.doc_y + self.geo.doc_h).saturating_sub(1),
                );
                if let Some(p) = self.doc_pos_at(m.column, row) {
                    self.ed.move_to(p, true);
                }
            }
            MouseEventKind::Up(_) => self.mouse_down = false,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let down = m.kind == MouseEventKind::ScrollDown;
                match &mut self.mode {
                    Mode::Help { scroll } => {
                        *scroll = if down {
                            *scroll + 3
                        } else {
                            scroll.saturating_sub(3)
                        }
                    }
                    Mode::Preview { scroll, .. } => {
                        *scroll = if down {
                            scroll.saturating_add(3)
                        } else {
                            scroll.saturating_sub(3)
                        }
                    }
                    Mode::Normal => {
                        let max = self.layout.lines.len().saturating_sub(1);
                        self.scroll = if down {
                            (self.scroll + 3).min(max)
                        } else {
                            self.scroll.saturating_sub(3)
                        };
                        self.follow = false;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Complete a partly typed path to the longest common prefix of matches.
fn complete_path(s: &str) -> Option<String> {
    let p = expand_path(s);
    let (dir, prefix) = if s.ends_with('/') {
        (p.clone(), String::new())
    } else {
        let parent = p
            .parent()
            .map(Path::to_path_buf)
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| ".".into());
        (parent, p.file_name()?.to_string_lossy().into_owned())
    };
    let names: Vec<(String, bool)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.path().is_dir(),
            )
        })
        .filter(|(n, _)| n.starts_with(&prefix))
        .collect();
    let first = names.first()?;
    let mut common = first.0.clone();
    for (n, _) in &names[1..] {
        while !n.starts_with(&common) {
            common.pop();
        }
    }
    let typed_dir = match s.rfind('/') {
        Some(i) => &s[..=i],
        None => "",
    };
    let mut out = format!("{typed_dir}{common}");
    if names.len() == 1 && first.1 {
        out.push('/');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes() {
        assert_eq!(base64(b"hello"), "aGVsbG8=");
        assert_eq!(base64(b"hi!"), "aGkh");
    }

    #[test]
    fn date_is_plausible() {
        let d = today();
        assert!(d.contains(", 20"), "{d}");
    }
}
