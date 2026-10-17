//! Document model: paragraphs of formatted characters, tables, page setup,
//! headers/footers and footnotes.
//!
//! Paragraph text is stored as a vector of `Cell`s (a character plus its
//! character formatting) behind an `Rc`, so cloning a whole document for undo
//! only copies pointers until a paragraph is actually modified.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

/// Character formatting flags. The low byte holds attributes turned on;
/// the high byte attributes explicitly turned off (overriding a style that
/// turns them on, like Word's `<w:b w:val="0"/>`).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub struct Fmt(pub u16);

impl Fmt {
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 2;
    pub const UNDERLINE: u16 = 4;
    pub const STRIKE: u16 = 8;
    pub const SUPER: u16 = 16;
    pub const SUB: u16 = 32;
    const OFF: u16 = 8;

    pub const ALL: [(u16, char, &'static str); 6] = [
        (Fmt::BOLD, 'b', "BOLD"),
        (Fmt::ITALIC, 'i', "ITALC"),
        (Fmt::UNDERLINE, 'u', "UND"),
        (Fmt::STRIKE, 's', "STKOUT"),
        (Fmt::SUPER, '^', "SUPRSCPT"),
        (Fmt::SUB, '_', "SUBSCPT"),
    ];

    pub fn has(self, f: u16) -> bool {
        self.0 & f != 0
    }

    /// Explicitly turned off.
    pub fn off(self, f: u16) -> bool {
        self.0 & (f << Fmt::OFF) != 0
    }

    pub fn with(self, f: u16, on: bool) -> Fmt {
        let mut v = if on {
            (self.0 | f) & !(f << Fmt::OFF)
        } else {
            (self.0 & !f) | (f << Fmt::OFF)
        };
        if on && f == Fmt::SUPER {
            v &= !Fmt::SUB;
        }
        if on && f == Fmt::SUB {
            v &= !Fmt::SUPER;
        }
        Fmt(v)
    }

    /// Only the attributes turned on.
    pub fn on_bits(self) -> Fmt {
        Fmt(self.0 & 0xFF)
    }

    pub fn union(self, o: Fmt) -> Fmt {
        Fmt(self.0 | o.0)
    }

    /// Formatting shown for text with formatting `own` in a style with
    /// formatting `style`.
    pub fn effective(style: Fmt, own: Fmt) -> Fmt {
        Fmt(((style.0 | own.0) & 0xFF) & !(own.0 >> Fmt::OFF))
    }

    pub fn to_code(self) -> String {
        let mut s: String = Fmt::ALL
            .iter()
            .filter(|(f, _, _)| self.has(*f))
            .map(|(_, c, _)| *c)
            .collect();
        s.extend(
            Fmt::ALL[..4]
                .iter()
                .filter(|(f, _, _)| self.off(*f))
                .map(|(_, c, _)| c.to_ascii_uppercase()),
        );
        s
    }

    pub fn from_code(s: &str) -> Fmt {
        let mut f = Fmt::default();
        for c in s.chars() {
            if let Some((flag, _, _)) = Fmt::ALL.iter().find(|(_, k, _)| *k == c) {
                f = f.with(*flag, true);
            } else if let Some((flag, _, _)) = Fmt::ALL[..4]
                .iter()
                .find(|(_, k, _)| k.to_ascii_uppercase() == c)
            {
                f = f.with(*flag, false);
            }
        }
        f
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Cell {
    pub ch: char,
    pub fmt: Fmt,
    /// Further run properties: index into the document's `Store::cprops`
    /// (0 = none).
    pub cp: u32,
}

impl Cell {
    pub fn new(ch: char, fmt: Fmt) -> Cell {
        Cell { ch, fmt, cp: 0 }
    }
    pub fn with_cp(ch: char, fmt: Fmt, cp: u32) -> Cell {
        Cell { ch, fmt, cp }
    }
}

/// Inline objects (pictures, bookmarks, field codes, breaks...) are stored
/// inline as private-use characters (Unicode plane 16) that index
/// `Store::objects`.
const OBJ_BASE: u32 = 0x100000;
const OBJ_MAX: u32 = 0xFFFD;

pub fn obj_char(id: u32) -> char {
    char::from_u32(OBJ_BASE + id.min(OBJ_MAX - 1)).unwrap_or('?')
}

pub fn obj_id(c: char) -> Option<u32> {
    let v = c as u32;
    (OBJ_BASE..OBJ_BASE + OBJ_MAX)
        .contains(&v)
        .then(|| v - OBJ_BASE)
}

/// A footnote reference or an inline object rather than text.
pub fn is_special(c: char) -> bool {
    fn_id(c).is_some() || obj_id(c).is_some()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObjKind {
    /// Zero-width marker kept for Word: bookmark, field code, comment anchor.
    Hidden,
    /// Shown as a placeholder: picture, chart, equation, deleted text.
    Visible,
    /// Block-level content inside a table cell (a nested table).
    Block,
    LineBreak,
    PageBreak,
    /// Paragraph boundary inside a table cell; `xml` is the next
    /// paragraph's `<w:pPr>`.
    ParaBreak,
}

impl ObjKind {
    pub fn is_break(self) -> bool {
        matches!(
            self,
            ObjKind::LineBreak | ObjKind::PageBreak | ObjKind::ParaBreak
        )
    }
    pub fn width_label(self) -> bool {
        matches!(self, ObjKind::Visible | ObjKind::Block)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct InlineObj {
    pub kind: ObjKind,
    /// Original WordprocessingML for the object.
    pub xml: String,
    /// What the editor shows (placeholder text or Reveal Codes name).
    pub label: String,
    /// The XML goes inside a `<w:r>` run (otherwise directly in the paragraph).
    pub in_run: bool,
}

/// Run properties beyond the quick formatting flags: the original `w:rPr`
/// children (by local name) and the inline wrappers (hyperlink, content
/// control, insertion...) the run sits in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CharProps {
    pub rpr: Vec<(String, String)>,
    pub wraps: Vec<u32>,
}

/// An element that encloses runs or blocks, kept verbatim: `open` is the
/// start tag (plus any properties child), `close` the end tag(s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapKind {
    /// Hyperlink; `target` is the URL (or #bookmark).
    Link,
    /// Tracked insertion; `target` is the author.
    Insert,
    /// Content control.
    Control,
    Field,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WrapDef {
    pub open: String,
    pub close: String,
    pub label: String,
    pub kind: WrapKind,
    pub target: String,
}

/// Append-only tables shared by a document and its undo snapshots.
#[derive(Debug, PartialEq)]
pub struct Store {
    pub cprops: Vec<CharProps>,
    /// What WordBench understands of each `cprops` entry.
    pub looks: Vec<crate::props::CharLook>,
    ix: HashMap<CharProps, u32>,
    pub objects: Vec<InlineObj>,
    pub wraps: Vec<WrapDef>,
}

impl Default for Store {
    fn default() -> Self {
        let mut s = Store {
            cprops: Vec::new(),
            looks: Vec::new(),
            ix: HashMap::new(),
            objects: Vec::new(),
            wraps: Vec::new(),
        };
        s.intern(CharProps::default());
        s
    }
}

impl Store {
    pub fn intern(&mut self, cp: CharProps) -> u32 {
        if let Some(&i) = self.ix.get(&cp) {
            return i;
        }
        let i = self.cprops.len() as u32;
        self.ix.insert(cp.clone(), i);
        self.looks.push(crate::props::look_from_rpr(&cp.rpr));
        self.cprops.push(cp);
        i
    }
    pub fn look(&self, id: u32) -> &crate::props::CharLook {
        self.looks.get(id as usize).unwrap_or(&self.looks[0])
    }
    /// The run properties of `cp` changed by `f`, interned.
    pub fn derive(&mut self, cp: u32, f: impl FnOnce(&mut CharProps)) -> u32 {
        let mut c = self.cprops(cp).clone();
        f(&mut c);
        self.intern(c)
    }
    pub fn cprops(&self, id: u32) -> &CharProps {
        self.cprops.get(id as usize).unwrap_or(&self.cprops[0])
    }
    pub fn obj(&self, c: char) -> Option<&InlineObj> {
        self.objects.get(obj_id(c)? as usize)
    }
    pub fn add_obj(&mut self, o: InlineObj) -> char {
        self.objects.push(o);
        obj_char(self.objects.len() as u32 - 1)
    }
    pub fn add_wrap(&mut self, w: WrapDef) -> u32 {
        self.wraps.push(w);
        self.wraps.len() as u32 - 1
    }
    /// Is this a zero-width marker the cursor should step over?
    pub fn is_hidden(&self, c: char) -> bool {
        self.obj(c).is_some_and(|o| o.kind == ObjKind::Hidden)
    }
}

pub type SharedStore = Rc<RefCell<Store>>;

/// Word XML kept for a paragraph so saving a .docx loses nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaExtra {
    /// Attributes of `<w:p>` (paragraph ids, revision ids).
    pub p_attrs: String,
    /// All original `w:pPr` children as (local name, XML).
    pub ppr: Vec<(String, String)>,
    /// The modelled properties as read, to detect what the user changed.
    pub at_read: Option<ParaProps>,
    /// Enclosing block-level wrappers (content controls, custom XML).
    pub bwraps: Vec<u32>,
    /// Body-level XML that came just before this paragraph (bookmarks...).
    pub pre: String,
    /// Table cell properties (`w:tcPr`) for a cell's paragraph.
    pub tc_pr: Option<String>,
    /// Filler cell under a horizontally merged cell: not written.
    pub covered: bool,
    /// Row XML kept on the first cell: `<w:tr>` attributes and the
    /// `w:tblPrEx`/`w:trPr` children.
    pub tr: Option<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TableExtra {
    pub tbl_pr: Option<String>,
    /// Column widths when read, and the original `w:tblGrid`.
    pub grid: Option<(Vec<u16>, String)>,
    pub bwraps: Vec<u32>,
    pub pre: String,
}

/// A Word comment (`word/comments.xml`), keyed by its id.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Comment {
    pub author: String,
    pub initials: String,
    pub date: String,
    pub text: String,
    /// Original XML, written back while the text is unchanged.
    pub xml: String,
    pub orig_text: String,
}

/// Document properties (`docProps/core.xml`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct CoreProps {
    pub title: String,
    pub subject: String,
    pub author: String,
    pub keywords: String,
    pub description: String,
}

/// A package part or relationship added while editing (pictures, links).
#[derive(Clone, Debug, PartialEq)]
pub struct NewRel {
    pub id: String,
    /// Relationship type suffix ("hyperlink", "image").
    pub ty: String,
    pub target: String,
    pub external: bool,
}

/// Footnote/endnote details from a .docx, so unchanged notes keep their
/// original formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteMeta {
    pub endnote: bool,
    pub orig_text: String,
    pub xml: String,
}

/// The original .docx package, so a save writes back every part WordBench
/// does not edit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocxPkg {
    /// All parts in their original order.
    pub parts: Vec<(String, Vec<u8>)>,
    /// Main document part path (normally word/document.xml).
    pub main: String,
    /// Original `<w:document ...>` start tag (namespace declarations).
    pub root_open: String,
    /// `w:document` children before `w:body` (page background).
    pub doc_pre: String,
    /// Body-level XML after the last block.
    pub body_tail: String,
    /// Final `w:sectPr`.
    pub sect: Option<String>,
    /// Values when read, to detect changes.
    pub page: PageSetup,
    pub header: String,
    pub footer: String,
    /// Default header/footer parts of the final section.
    pub header_part: Option<String>,
    pub footer_part: Option<String>,
    /// Separator footnotes/endnotes (`w:type` set), kept verbatim.
    pub note_seps: Vec<String>,
    pub endnote_seps: Vec<String>,
    /// Document properties as read.
    pub core: CoreProps,
    pub track_changes: bool,
}

/// Footnote references are stored inline as private-use characters
/// (Unicode plane 15) that encode the footnote's id.
const FN_BASE: u32 = 0xF0000;
const FN_MAX: u32 = 0xFFF0;

pub fn fn_char(id: u32) -> char {
    char::from_u32(FN_BASE + id.min(FN_MAX - 1)).unwrap_or('?')
}

pub fn fn_id(c: char) -> Option<u32> {
    let v = c as u32;
    (FN_BASE..FN_BASE + FN_MAX)
        .contains(&v)
        .then(|| v - FN_BASE)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Full,
}

impl Align {
    pub const NAMES: [&'static str; 4] = ["Left", "Center", "Right", "Full"];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Align {
        [Align::Left, Align::Center, Align::Right, Align::Full][i.min(3)]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListKind {
    Bullet,
    Number,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ListInfo {
    pub kind: ListKind,
    pub level: u8,
    /// Word numbering instance (`w:numId`), 0 for lists made in WordBench.
    pub num: u32,
}

impl ListInfo {
    pub fn new(kind: ListKind, level: u8) -> ListInfo {
        ListInfo {
            kind,
            level,
            num: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind {
    Left,
    Center,
    Right,
    Decimal,
}

/// A tab stop: position in columns from the left of the text area.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TabStop {
    pub pos: u16,
    pub kind: TabKind,
    /// Fill character (' ' for none).
    pub leader: char,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ParaProps {
    pub align: Align,
    /// Paragraph style id ("" = the default style).
    pub style: String,
    pub list: Option<ListInfo>,
    /// Left indent in character columns (10 per inch).
    pub indent: u16,
    /// First-line indent relative to `indent`, in columns (may be negative).
    pub first_indent: i16,
    /// Line spacing: 1 or 2.
    pub spacing: u8,
    /// Blank lines before/after the paragraph (0-3).
    pub space_before: u8,
    pub space_after: u8,
    /// Hard page break before this paragraph.
    pub page_break_before: bool,
    /// No space between paragraphs of the same style.
    pub contextual: bool,
    pub tabs: Vec<TabStop>,
}

impl Default for ParaProps {
    fn default() -> Self {
        ParaProps {
            align: Align::Left,
            style: String::new(),
            list: None,
            indent: 0,
            first_indent: 0,
            spacing: 1,
            space_before: 0,
            space_after: 0,
            page_break_before: false,
            contextual: false,
            tabs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    pub cells: Rc<Vec<Cell>>,
    pub props: ParaProps,
    pub extra: Option<Rc<ParaExtra>>,
}

impl Paragraph {
    pub fn new() -> Paragraph {
        Paragraph::default()
    }

    pub fn from_text(s: &str, fmt: Fmt) -> Paragraph {
        Paragraph {
            cells: Rc::new(s.chars().map(|c| Cell::new(c, fmt)).collect()),
            props: ParaProps::default(),
            extra: None,
        }
    }

    pub fn with_cells(cells: Vec<Cell>, props: ParaProps) -> Paragraph {
        Paragraph {
            cells: Rc::new(cells),
            props,
            extra: None,
        }
    }

    /// Extras for the two halves when this paragraph is split: a section
    /// break stays with the second half, preceding XML with the first.
    pub fn split_extras(&self) -> (Option<Rc<ParaExtra>>, Option<Rc<ParaExtra>>) {
        let Some(x) = &self.extra else {
            return (None, None);
        };
        let mut head = (**x).clone();
        head.ppr.retain(|(n, _)| n != "sectPr");
        let mut tail = (**x).clone();
        tail.pre.clear();
        tail.p_attrs.clear();
        (Some(Rc::new(head)), Some(Rc::new(tail)))
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cells_mut(&mut self) -> &mut Vec<Cell> {
        Rc::make_mut(&mut self.cells)
    }

    /// Text without footnote markers and inline objects.
    pub fn plain_text(&self) -> String {
        self.cells
            .iter()
            .filter(|c| !is_special(c.ch))
            .map(|c| c.ch)
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// Content width of each column in character cells.
    pub cols: Vec<u16>,
    pub rows: Vec<Vec<Paragraph>>,
    pub extra: Option<Rc<TableExtra>>,
}

impl Table {
    pub fn new(rows: usize, cols: usize, text_width: u16) -> Table {
        let cols = cols.max(1);
        let w = ((text_width as usize).saturating_sub(1) / cols)
            .saturating_sub(3)
            .max(3) as u16;
        Table {
            cols: vec![w; cols],
            rows: (0..rows.max(1))
                .map(|_| vec![Paragraph::new(); cols])
                .collect(),
            extra: None,
        }
    }

    pub fn ncols(&self) -> usize {
        self.cols.len()
    }

    /// Effective column widths that fit within `width` columns.
    pub fn fitted_cols(&self, width: u16) -> Vec<u16> {
        let n = self.cols.len().max(1);
        let overhead = (3 * n + 1) as u32;
        let total: u32 = self.cols.iter().map(|&c| c as u32).sum::<u32>() + overhead;
        if total <= width as u32 {
            return self.cols.clone();
        }
        let avail = (width as u32).saturating_sub(overhead).max(n as u32 * 3);
        let sum: u32 = self
            .cols
            .iter()
            .map(|&c| c.max(1) as u32)
            .sum::<u32>()
            .max(1);
        self.cols
            .iter()
            .map(|&c| ((c.max(1) as u32 * avail) / sum).max(3) as u16)
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Paragraph),
    Table(Table),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PageNumPos {
    #[default]
    None,
    BottomCenter,
    BottomRight,
    TopRight,
}

impl PageNumPos {
    pub const NAMES: [&'static str; 4] = ["None", "Bottom center", "Bottom right", "Top right"];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> PageNumPos {
        [
            PageNumPos::None,
            PageNumPos::BottomCenter,
            PageNumPos::BottomRight,
            PageNumPos::TopRight,
        ][i.min(3)]
    }
}

/// Page geometry in inches. The layout uses a 10 characters-per-inch,
/// 6 lines-per-inch grid (12pt Courier), just like a typewriter-era printer.
#[derive(Clone, Debug, PartialEq)]
pub struct PageSetup {
    pub width: f32,
    pub height: f32,
    pub margin_left: f32,
    pub margin_right: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub page_numbers: PageNumPos,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup {
            width: 8.5,
            height: 11.0,
            margin_left: 1.0,
            margin_right: 1.0,
            margin_top: 1.0,
            margin_bottom: 1.0,
            page_numbers: PageNumPos::None,
        }
    }
}

pub const CPI: f32 = 10.0;
pub const LPI: f32 = 6.0;

impl PageSetup {
    pub fn text_cols(&self) -> u16 {
        (((self.width - self.margin_left - self.margin_right) * CPI).round() as i32).clamp(20, 400)
            as u16
    }
    pub fn body_lines(&self) -> usize {
        (((self.height - self.margin_top - self.margin_bottom) * LPI).round() as i32).clamp(10, 400)
            as usize
    }
}

pub const PAPER_SIZES: [(&str, f32, f32); 3] = [
    ("Letter", 8.5, 11.0),
    ("A4", 8.27, 11.69),
    ("Legal", 8.5, 14.0),
];

/// Identifies a paragraph: either a top-level paragraph block or the
/// paragraph inside a table cell (row, column).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct ParaRef {
    pub block: usize,
    pub cell: Option<(u16, u16)>,
}

impl ParaRef {
    pub fn top(block: usize) -> ParaRef {
        ParaRef { block, cell: None }
    }
    pub fn cell(block: usize, r: usize, c: usize) -> ParaRef {
        ParaRef {
            block,
            cell: Some((r as u16, c as u16)),
        }
    }
}

/// A cursor position: a paragraph and a character offset within it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Pos {
    pub pr: ParaRef,
    pub off: usize,
}

impl Pos {
    pub fn new(pr: ParaRef, off: usize) -> Pos {
        Pos { pr, off }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    pub page: PageSetup,
    /// Header and footer text; `{page}` and `{pages}` are substituted.
    pub header: String,
    pub footer: String,
    pub footnotes: BTreeMap<u32, String>,
    pub note_meta: BTreeMap<u32, NoteMeta>,
    /// Run properties and inline objects (shared with undo snapshots).
    pub store: SharedStore,
    /// The .docx this document was read from.
    pub docx: Option<Rc<DocxPkg>>,
    pub styles: Rc<crate::styles::StyleSheet>,
    pub numbering: Rc<crate::styles::Numbering>,
    pub comments: BTreeMap<String, Comment>,
    pub core: CoreProps,
    /// Relationships and parts added while editing.
    pub new_rels: Vec<NewRel>,
    pub new_parts: Vec<(String, Vec<u8>)>,
    /// Styles WordBench applied that the file may lack (written if missing).
    pub added_styles: std::collections::BTreeSet<String>,
    /// Word's "track changes" setting (edits are recorded as revisions).
    pub track_changes: bool,
    /// Theme fonts (major for headings, minor for body text).
    pub theme_fonts: (String, String),
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

impl Document {
    pub fn new() -> Document {
        Document {
            blocks: vec![Block::Para(Paragraph::new())],
            page: PageSetup::default(),
            header: String::new(),
            footer: String::new(),
            footnotes: BTreeMap::new(),
            note_meta: BTreeMap::new(),
            store: Rc::new(RefCell::new(Store::default())),
            docx: None,
            styles: Rc::new(crate::styles::StyleSheet::default()),
            numbering: Rc::new(crate::styles::Numbering::default()),
            comments: BTreeMap::new(),
            core: CoreProps::default(),
            new_rels: Vec::new(),
            new_parts: Vec::new(),
            added_styles: Default::default(),
            track_changes: false,
            theme_fonts: ("Calibri Light".into(), "Calibri".into()),
        }
        .with_builtin_styles()
    }

    fn with_builtin_styles(mut self) -> Document {
        let sheet = crate::styles::StyleSheet::builtin(&mut self.store.borrow_mut());
        self.styles = Rc::new(sheet);
        self
    }

    /// Paragraph properties of a style (with inheritance), ready to use.
    pub fn style_props(&self, id: &str) -> ParaProps {
        let mut p = self
            .styles
            .get(id)
            .map(|s| s.props.clone())
            .unwrap_or_default();
        p.style = if self.styles.is_default(id) {
            String::new()
        } else {
            id.to_string()
        };
        p
    }

    /// A font name with theme fonts resolved.
    pub fn font_name(&self, f: &str) -> String {
        match f {
            "+major" => self.theme_fonts.0.clone(),
            "+minor" => self.theme_fonts.1.clone(),
            f => f.to_string(),
        }
    }

    pub fn new_obj(&self, o: InlineObj) -> char {
        self.store.borrow_mut().add_obj(o)
    }

    pub fn is_hidden(&self, c: char) -> bool {
        self.store.borrow().is_hidden(c)
    }

    /// Ensure the invariants the editor relies on: at least one block, the
    /// first and last blocks are paragraphs, and tables are not empty.
    pub fn normalize(&mut self) {
        self.blocks.retain(|b| match b {
            Block::Table(t) => !t.rows.is_empty() && !t.cols.is_empty(),
            Block::Para(_) => true,
        });
        for b in self.blocks.iter_mut() {
            if let Block::Table(t) = b {
                let n = t.cols.len();
                for row in t.rows.iter_mut() {
                    row.resize(n, Paragraph::new());
                }
            }
        }
        if !matches!(self.blocks.first(), Some(Block::Para(_))) {
            self.blocks.insert(0, Block::Para(Paragraph::new()));
        }
        if !matches!(self.blocks.last(), Some(Block::Para(_))) {
            self.blocks.push(Block::Para(Paragraph::new()));
        }
    }

    pub fn para(&self, pr: ParaRef) -> &Paragraph {
        match (&self.blocks[pr.block], pr.cell) {
            (Block::Para(p), _) => p,
            (Block::Table(t), Some((r, c))) => &t.rows[r as usize][c as usize],
            (Block::Table(t), None) => &t.rows[0][0],
        }
    }

    pub fn para_mut(&mut self, pr: ParaRef) -> &mut Paragraph {
        match (&mut self.blocks[pr.block], pr.cell) {
            (Block::Para(p), _) => p,
            (Block::Table(t), Some((r, c))) => &mut t.rows[r as usize][c as usize],
            (Block::Table(t), None) => &mut t.rows[0][0],
        }
    }

    pub fn table(&self, block: usize) -> Option<&Table> {
        match self.blocks.get(block) {
            Some(Block::Table(t)) => Some(t),
            _ => None,
        }
    }

    pub fn table_mut(&mut self, block: usize) -> Option<&mut Table> {
        match self.blocks.get_mut(block) {
            Some(Block::Table(t)) => Some(t),
            _ => None,
        }
    }

    /// First paragraph reference of a block.
    pub fn first_ref(&self, block: usize) -> ParaRef {
        match &self.blocks[block] {
            Block::Para(_) => ParaRef::top(block),
            Block::Table(_) => ParaRef::cell(block, 0, 0),
        }
    }

    /// Last paragraph reference of a block.
    pub fn last_ref(&self, block: usize) -> ParaRef {
        match &self.blocks[block] {
            Block::Para(_) => ParaRef::top(block),
            Block::Table(t) => ParaRef::cell(block, t.rows.len() - 1, t.ncols() - 1),
        }
    }

    /// A filler cell under a horizontally merged cell (not shown).
    pub fn is_covered(&self, pr: ParaRef) -> bool {
        pr.cell.is_some_and(|(_, c)| c > 0)
            && self.para(pr).extra.as_ref().is_some_and(|x| x.covered)
    }

    pub fn next_ref(&self, pr: ParaRef) -> Option<ParaRef> {
        let mut r = self.next_ref_raw(pr)?;
        while self.is_covered(r) {
            r = self.next_ref_raw(r)?;
        }
        Some(r)
    }

    pub fn prev_ref(&self, pr: ParaRef) -> Option<ParaRef> {
        let mut r = self.prev_ref_raw(pr)?;
        while self.is_covered(r) {
            r = self.prev_ref_raw(r)?;
        }
        Some(r)
    }

    fn next_ref_raw(&self, pr: ParaRef) -> Option<ParaRef> {
        if let (Some((r, c)), Some(t)) = (pr.cell, self.table(pr.block)) {
            let (r, c) = (r as usize, c as usize);
            if c + 1 < t.ncols() {
                return Some(ParaRef::cell(pr.block, r, c + 1));
            }
            if r + 1 < t.rows.len() {
                return Some(ParaRef::cell(pr.block, r + 1, 0));
            }
        }
        (pr.block + 1 < self.blocks.len()).then(|| self.first_ref(pr.block + 1))
    }

    fn prev_ref_raw(&self, pr: ParaRef) -> Option<ParaRef> {
        if let (Some((r, c)), Some(t)) = (pr.cell, self.table(pr.block)) {
            let (r, c) = (r as usize, c as usize);
            if c > 0 {
                return Some(ParaRef::cell(pr.block, r, c - 1));
            }
            if r > 0 {
                return Some(ParaRef::cell(pr.block, r - 1, t.ncols() - 1));
            }
        }
        (pr.block > 0).then(|| self.last_ref(pr.block - 1))
    }

    /// All paragraph references in reading order.
    pub fn refs(&self) -> Vec<ParaRef> {
        let mut out = Vec::new();
        for (i, b) in self.blocks.iter().enumerate() {
            match b {
                Block::Para(_) => out.push(ParaRef::top(i)),
                Block::Table(t) => {
                    for r in 0..t.rows.len() {
                        for c in 0..t.ncols() {
                            let pr = ParaRef::cell(i, r, c);
                            if !self.is_covered(pr) {
                                out.push(pr);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    pub fn end_pos(&self) -> Pos {
        let pr = self.last_ref(self.blocks.len() - 1);
        Pos::new(pr, self.para(pr).len())
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        if p.pr.block >= self.blocks.len() {
            return self.end_pos();
        }
        let pr = match (&self.blocks[p.pr.block], p.pr.cell) {
            (Block::Para(_), _) => ParaRef::top(p.pr.block),
            (Block::Table(t), Some((r, c))) => ParaRef::cell(
                p.pr.block,
                (r as usize).min(t.rows.len() - 1),
                (c as usize).min(t.ncols() - 1),
            ),
            (Block::Table(_), None) => ParaRef::cell(p.pr.block, 0, 0),
        };
        Pos::new(pr, p.off.min(self.para(pr).len()))
    }

    pub fn new_footnote(&mut self, text: String) -> u32 {
        let id = self
            .footnotes
            .keys()
            .next_back()
            .map(|k| k + 1)
            .unwrap_or(1);
        self.footnotes.insert(id, text);
        id
    }

    /// Footnote ids in order of first appearance, numbered from 1.
    pub fn footnote_numbers(&self) -> Vec<u32> {
        let mut seen = Vec::new();
        for pr in self.refs() {
            for c in self.para(pr).cells.iter() {
                if let Some(id) = fn_id(c.ch)
                    && !seen.contains(&id)
                {
                    seen.push(id);
                }
            }
        }
        seen
    }

    pub fn word_count(&self) -> usize {
        self.refs()
            .iter()
            .map(|&pr| self.para(pr).plain_text().split_whitespace().count())
            .sum::<usize>()
            + self
                .footnotes
                .values()
                .map(|t| t.split_whitespace().count())
                .sum::<usize>()
    }

    pub fn char_count(&self) -> usize {
        self.refs()
            .iter()
            .map(|&pr| self.para(pr).plain_text().chars().count())
            .sum()
    }

    /// Build a document from plain text: one paragraph per line.
    pub fn from_plain_text(s: &str) -> Document {
        let mut d = Document::new();
        d.blocks = s
            .replace("\r\n", "\n")
            .split('\n')
            .map(|l| Block::Para(Paragraph::from_text(l, Fmt::default())))
            .collect();
        if s.ends_with('\n') && d.blocks.len() > 1 {
            d.blocks.pop();
        }
        d.normalize();
        d
    }

    pub fn to_plain_text(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            match b {
                Block::Para(p) => {
                    out.push_str(&p.plain_text());
                    out.push('\n');
                }
                Block::Table(t) => {
                    for row in &t.rows {
                        let cells: Vec<String> = row.iter().map(|p| p.plain_text()).collect();
                        out.push_str(&cells.join("\t"));
                        out.push('\n');
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_codes_roundtrip() {
        let f = Fmt::default().with(Fmt::BOLD, true).with(Fmt::SUPER, true);
        assert_eq!(Fmt::from_code(&f.to_code()), f);
        assert!(!f.with(Fmt::SUB, true).has(Fmt::SUPER));
    }

    #[test]
    fn footnote_chars() {
        assert_eq!(fn_id(fn_char(7)), Some(7));
        assert_eq!(fn_id('a'), None);
    }

    #[test]
    fn ref_navigation_through_table() {
        let mut d = Document::new();
        d.blocks.push(Block::Table(Table::new(2, 2, 65)));
        d.normalize();
        let refs = d.refs();
        assert_eq!(refs.len(), 6);
        for w in refs.windows(2) {
            assert_eq!(d.next_ref(w[0]), Some(w[1]));
            assert_eq!(d.prev_ref(w[1]), Some(w[0]));
        }
    }
}
