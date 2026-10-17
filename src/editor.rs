//! Editing operations on a document: cursor movement, selection, typing,
//! deletion, clipboard, formatting, lists, tables, footnotes, search and
//! undo/redo. Independent of the terminal UI.

use crate::layout::Layout;
use crate::model::*;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditKind {
    None,
    Typing,
    Delete,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FindResult {
    Found,
    Wrapped,
    NotFound,
}

struct Snapshot {
    doc: Document,
    cur: Pos,
}

pub struct Editor {
    pub doc: Document,
    pub cur: Pos,
    pub anchor: Option<Pos>,
    pub typing_fmt: Fmt,
    /// Run properties for new text (font, hyperlink...), from the
    /// neighbouring character.
    pub typing_cp: u32,
    pub path: Option<PathBuf>,
    pub modified: bool,
    /// Incremented on every document change (used to cache the layout).
    pub version: u64,
    pub goal_x: Option<u16>,
    pub clipboard: Vec<Block>,
    /// The store the clipboard's run properties and objects refer to.
    clip_store: Option<SharedStore>,
    /// Record edits as tracked changes, under this author's name.
    pub track: bool,
    pub author: String,
    pub(crate) track_wrap: Option<u32>,
    /// Deletions recorded in this session and the text they hold.
    pub(crate) our_dels: std::collections::HashMap<char, Vec<Cell>>,
    pub(crate) rev_id: u32,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: EditKind,
}

const UNDO_LIMIT: usize = 1000;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '’'
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

impl Default for Editor {
    fn default() -> Self {
        Editor::new()
    }
}

impl Editor {
    pub fn new() -> Editor {
        Editor::with_doc(Document::new(), None)
    }

    pub fn with_doc(mut doc: Document, path: Option<PathBuf>) -> Editor {
        doc.normalize();
        let track = doc.track_changes;
        Editor {
            cur: Pos::new(doc.first_ref(0), 0),
            doc,
            anchor: None,
            typing_fmt: Fmt::default(),
            typing_cp: 0,
            path,
            modified: false,
            version: 0,
            goal_x: None,
            clipboard: Vec::new(),
            clip_store: None,
            track,
            author: crate::platform::user_name(),
            track_wrap: None,
            our_dels: std::collections::HashMap::new(),
            rev_id: 1_000_000,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::None,
        }
    }

    pub fn para(&self) -> &Paragraph {
        self.doc.para(self.cur.pr)
    }

    pub(crate) fn touch(&mut self) {
        self.modified = true;
        self.version += 1;
    }

    /// Record an undo point before an edit. Consecutive typing (or
    /// consecutive deleting) is grouped into one undo step.
    pub fn checkpoint(&mut self, kind: EditKind) {
        let coalesce =
            kind == self.last_edit && matches!(kind, EditKind::Typing | EditKind::Delete);
        if !coalesce {
            self.undo.push(Snapshot {
                doc: self.doc.clone(),
                cur: self.cur,
            });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_edit = kind;
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            doc: std::mem::replace(&mut self.doc, s.doc),
            cur: self.cur,
        });
        self.cur = self.doc.clamp(s.cur);
        self.after_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            doc: std::mem::replace(&mut self.doc, s.doc),
            cur: self.cur,
        });
        self.cur = self.doc.clamp(s.cur);
        self.after_history();
        true
    }

    fn after_history(&mut self) {
        self.anchor = None;
        self.last_edit = EditKind::None;
        self.touch();
        self.update_typing_fmt();
    }

    fn update_typing_fmt(&mut self) {
        let p = self.doc.para(self.cur.pr);
        let before = p.cells[..self.cur.off.min(p.len())]
            .iter()
            .rev()
            .find(|c| !is_special(c.ch));
        let after = p
            .cells
            .iter()
            .skip(self.cur.off)
            .find(|c| !is_special(c.ch));
        if let Some(c) = before.or(after) {
            self.typing_fmt = c.fmt;
            self.typing_cp = c.cp;
            return;
        }
        // An empty paragraph or cell: use the formatting Word saved for its
        // paragraph mark, or keep the current font (as when you set a font
        // and then make a table or a new paragraph).
        let mark = p
            .extra
            .as_ref()
            .and_then(|x| x.ppr.iter().find(|(n, _)| n == "rPr"))
            .map(|(_, x)| x.clone());
        let mut store = self.doc.store.borrow_mut();
        match mark.map(|x| {
            crate::styles::child_items(x.trim_start_matches("<w:rPr>").trim_end_matches("</w:rPr>"))
        }) {
            Some(items) if !items.is_empty() => {
                let items: Vec<(String, String)> = items
                    .into_iter()
                    .filter(|(n, _)| !matches!(n.as_str(), "ins" | "del" | "rPrChange"))
                    .collect();
                self.typing_fmt = crate::props::fmt_from_rpr(&items);
                self.typing_cp = store.intern(CharProps {
                    rpr: items,
                    wraps: Vec::new(),
                });
            }
            _ => {
                // Don't carry a hyperlink or tracked insertion along.
                let cp = self.typing_cp;
                if !store.cprops(cp).wraps.is_empty() {
                    self.typing_cp = store.derive(cp, |c| c.wraps.clear());
                }
            }
        }
    }

    // ----- selection -------------------------------------------------------

    /// The selection as an ordered, non-empty range.
    pub fn sel_range(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        match a.cmp(&self.cur) {
            std::cmp::Ordering::Less => Some((a, self.cur)),
            std::cmp::Ordering::Greater => Some((self.cur, a)),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn select_range(&mut self, a: Pos, b: Pos) {
        self.anchor = Some(a);
        self.cur = b;
        self.last_edit = EditKind::None;
    }

    pub fn select_all(&mut self) {
        let a = Pos::new(self.doc.first_ref(0), 0);
        let b = self.doc.end_pos();
        self.select_range(a, b);
    }

    /// Move the cursor, optionally extending the selection.
    pub fn move_to(&mut self, p: Pos, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.cur = self.doc.clamp(p);
        self.last_edit = EditKind::None;
        self.update_typing_fmt();
    }

    // ----- positions -------------------------------------------------------

    fn hidden_at(&self, pr: ParaRef, off: usize) -> bool {
        self.doc
            .para(pr)
            .cells
            .get(off)
            .is_some_and(|c| self.doc.is_hidden(c.ch))
    }

    pub fn pos_left(&self, p: Pos) -> Pos {
        if p.off > 0 {
            let mut off = p.off - 1;
            while off > 0 && self.hidden_at(p.pr, off) {
                off -= 1;
            }
            return Pos::new(p.pr, off);
        }
        match self.doc.prev_ref(p.pr) {
            Some(pr) => Pos::new(pr, self.doc.para(pr).len()),
            None => p,
        }
    }

    pub fn pos_right(&self, p: Pos) -> Pos {
        let len = self.doc.para(p.pr).len();
        if p.off < len {
            let mut off = p.off;
            while off < len && self.hidden_at(p.pr, off) {
                off += 1;
            }
            return Pos::new(p.pr, (off + 1).min(len));
        }
        match self.doc.next_ref(p.pr) {
            Some(pr) => Pos::new(pr, 0),
            None => p,
        }
    }

    pub fn word_right(&self, p: Pos) -> Pos {
        let cells = &self.doc.para(p.pr).cells;
        if p.off >= cells.len() {
            return self.pos_right(p);
        }
        let mut i = p.off;
        while i < cells.len() && is_word(cells[i].ch) {
            i += 1;
        }
        while i < cells.len() && !is_word(cells[i].ch) {
            i += 1;
        }
        Pos::new(p.pr, i)
    }

    pub fn word_left(&self, p: Pos) -> Pos {
        if p.off == 0 {
            return self.pos_left(p);
        }
        let cells = &self.doc.para(p.pr).cells;
        let mut i = p.off;
        while i > 0 && !is_word(cells[i - 1].ch) {
            i -= 1;
        }
        while i > 0 && is_word(cells[i - 1].ch) {
            i -= 1;
        }
        Pos::new(p.pr, i)
    }

    pub fn doc_start(&self) -> Pos {
        Pos::new(self.doc.first_ref(0), 0)
    }

    pub fn line_start(&self, layout: &Layout) -> Pos {
        Pos::new(self.cur.pr, layout.line_bounds(self.cur).0)
    }

    pub fn line_end(&self, layout: &Layout) -> Pos {
        Pos::new(self.cur.pr, layout.line_bounds(self.cur).1)
    }

    /// Word containing (or just before) the cursor: (start, end, text).
    pub fn word_at_cursor(&self) -> Option<(usize, usize, String)> {
        let cells = &self.para().cells;
        let mut s = self.cur.off.min(cells.len());
        if (s >= cells.len() || !is_word(cells[s].ch)) && s > 0 && is_word(cells[s - 1].ch) {
            s -= 1;
        }
        if s >= cells.len() || !is_word(cells[s].ch) {
            return None;
        }
        let mut e = s;
        while s > 0 && is_word(cells[s - 1].ch) {
            s -= 1;
        }
        while e < cells.len() && is_word(cells[e].ch) {
            e += 1;
        }
        let w: String = cells[s..e].iter().map(|c| c.ch).collect();
        Some((s, e, w.trim_matches(|c| c == '\'' || c == '’').to_string()))
    }

    // ----- low-level edits (no undo checkpoint) ------------------------------

    fn typed(&self, ch: char) -> Cell {
        Cell::with_cp(ch, self.typing_fmt, self.typing_cp)
    }

    pub(crate) fn insert_cell_raw(&mut self, mut c: Cell) {
        if self.track {
            c.cp = self.tracked_cp(c.cp);
        }
        let off = self.cur.off;
        self.doc.para_mut(self.cur.pr).cells_mut().insert(off, c);
        self.cur.off += 1;
        self.touch();
    }

    fn split_raw(&mut self, page_break: bool) {
        let pr = self.cur.pr;
        if pr.cell.is_some() {
            self.insert_object(ObjKind::ParaBreak);
            return;
        }
        let off = self.cur.off;
        let at_end = off >= self.doc.para(pr).len();
        let mut props = self.doc.para(pr).props.clone();
        props.page_break_before = page_break;
        // After a heading, the next paragraph gets the style's "next" style.
        let next = self
            .doc
            .styles
            .get(&props.style)
            .and_then(|s| s.next.clone())
            .filter(|n| *n != props.style);
        if let (true, Some(next)) = (at_end, next) {
            let list = props.list;
            props = self.doc.style_props(&next);
            props.list = list;
            props.page_break_before = page_break;
        }
        let p = self.doc.para_mut(pr);
        let tail = p.cells_mut().split_off(off);
        let (head_x, tail_x) = p.split_extras();
        p.extra = head_x;
        let mut np = Paragraph::with_cells(tail, props);
        np.extra = tail_x;
        self.doc.blocks.insert(pr.block + 1, Block::Para(np));
        self.cur = Pos::new(ParaRef::top(pr.block + 1), 0);
        self.touch();
    }

    /// Endpoints of a range adjusted so both are in top-level paragraphs
    /// (a table touched by the range is included whole).
    fn top_level_range(&self, mut a: Pos, mut b: Pos) -> Option<(Pos, Pos)> {
        if a.pr.cell.is_some() {
            let bi = a.pr.block;
            if bi == 0 || !matches!(self.doc.blocks[bi - 1], Block::Para(_)) {
                return None;
            }
            a = Pos::new(
                ParaRef::top(bi - 1),
                self.doc.para(ParaRef::top(bi - 1)).len(),
            );
        }
        if b.pr.cell.is_some() {
            let bi = b.pr.block;
            if bi + 1 >= self.doc.blocks.len() || !matches!(self.doc.blocks[bi + 1], Block::Para(_))
            {
                return None;
            }
            b = Pos::new(ParaRef::top(bi + 1), 0);
        }
        Some((a, b))
    }

    pub(crate) fn refs_between(&self, a: ParaRef, b: ParaRef) -> Vec<ParaRef> {
        self.doc
            .refs()
            .into_iter()
            .filter(|r| *r >= a && *r <= b)
            .collect()
    }

    fn delete_range_raw(&mut self, a: Pos, b: Pos) -> bool {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return false;
        }
        if self.track {
            // Tracked: mark the text deleted in each paragraph, keep the
            // paragraphs themselves.
            let refs = self.refs_between(a.pr, b.pr);
            for r in refs.iter().rev() {
                let len = self.doc.para(*r).len();
                let s = if *r == a.pr { a.off } else { 0 };
                let e = if *r == b.pr { b.off } else { len };
                if s < e {
                    self.track_delete(*r, s, e);
                }
            }
            self.cur = a;
            return true;
        }
        if a.pr == b.pr {
            self.doc.para_mut(a.pr).cells_mut().drain(a.off..b.off);
            self.cur = a;
            self.touch();
            return true;
        }
        if a.pr.cell.is_some() && a.pr.block == b.pr.block {
            for r in self.refs_between(a.pr, b.pr) {
                let p = self.doc.para_mut(r);
                let s = if r == a.pr { a.off } else { 0 };
                let e = if r == b.pr { b.off } else { p.len() };
                p.cells_mut().drain(s..e);
            }
            self.cur = a;
            self.touch();
            return true;
        }
        let Some((a, b)) = self.top_level_range(a, b) else {
            return false;
        };
        let tail: Vec<Cell> = self.doc.para(b.pr).cells[b.off..].to_vec();
        {
            let c = self.doc.para_mut(a.pr).cells_mut();
            c.truncate(a.off);
            c.extend(tail);
        }
        self.doc.blocks.drain(a.pr.block + 1..=b.pr.block);
        self.cur = a;
        self.touch();
        true
    }

    pub(crate) fn delete_selection_raw(&mut self) -> bool {
        let Some((a, b)) = self.sel_range() else {
            self.anchor = None;
            return false;
        };
        self.anchor = None;
        self.delete_range_raw(a, b)
    }

    // ----- typing --------------------------------------------------------

    pub fn insert_char(&mut self, ch: char) {
        if self.sel_range().is_some() {
            self.checkpoint(EditKind::Other);
            self.delete_selection_raw();
            self.last_edit = EditKind::Typing;
        } else {
            self.checkpoint(EditKind::Typing);
        }
        self.anchor = None;
        self.insert_cell_raw(self.typed(ch));
    }

    /// Insert a string (paste or generated text); newlines split paragraphs.
    pub fn insert_text(&mut self, s: &str) {
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        for ch in s.chars() {
            match ch {
                '\r' => {}
                '\n' => self.split_raw(false),
                c if c.is_control() && c != '\t' => {}
                c => self.insert_cell_raw(self.typed(c)),
            }
        }
    }

    fn insert_object(&mut self, kind: ObjKind) {
        let label = match kind {
            ObjKind::LineBreak => "Ln Brk",
            ObjKind::PageBreak => "HPg",
            _ => "",
        };
        let xml = match kind {
            ObjKind::LineBreak => "<w:br/>",
            ObjKind::PageBreak => r#"<w:br w:type="page"/>"#,
            _ => "",
        };
        let ch = self.doc.new_obj(InlineObj {
            kind,
            xml: xml.into(),
            label: label.into(),
            in_run: kind != ObjKind::ParaBreak,
        });
        self.insert_cell_raw(Cell::with_cp(ch, Fmt::default(), self.typing_cp));
    }

    /// Shift+Enter: new line within the paragraph.
    pub fn line_break(&mut self) {
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        self.insert_object(ObjKind::LineBreak);
    }

    pub fn newline(&mut self) {
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        if self.cur.pr.cell.is_some() {
            // A new paragraph inside the cell, as in Word.
            self.insert_object(ObjKind::ParaBreak);
            return;
        }
        let p = self.doc.para_mut(self.cur.pr);
        if p.props.list.is_some() && p.is_empty() {
            p.props.list = None;
            self.touch();
            return;
        }
        self.split_raw(false);
    }

    pub fn page_break(&mut self) -> bool {
        if self.cur.pr.cell.is_some() {
            return false;
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        if self.cur.off == 0 && !self.para().is_empty() {
            self.doc.para_mut(self.cur.pr).props.page_break_before = true;
            self.touch();
        } else {
            self.split_raw(true);
        }
        true
    }

    pub fn backspace(&mut self) {
        if self.sel_range().is_some() {
            self.checkpoint(EditKind::Other);
            self.delete_selection_raw();
            return;
        }
        let Pos { pr, off } = self.cur;
        let mut k = off;
        while k > 0 && self.hidden_at(pr, k - 1) {
            k -= 1;
        }
        if k > 0 && self.track {
            self.checkpoint(EditKind::Delete);
            let before = self.doc.para(pr).len();
            let end = self.track_delete(pr, k - 1, k);
            // Stay before the deletion marker.
            let shrink = before - self.doc.para(pr).len();
            self.cur.off = end.saturating_sub(1).min(off - shrink.min(off));
            return;
        }
        if k > 0 {
            self.checkpoint(EditKind::Delete);
            self.doc.para_mut(pr).cells_mut().remove(k - 1);
            self.cur.off -= 1;
            self.touch();
            return;
        }
        if pr.cell.is_some() {
            return;
        }
        let props = self.para().props.clone();
        if props.page_break_before {
            self.checkpoint(EditKind::Other);
            self.doc.para_mut(pr).props.page_break_before = false;
            self.touch();
            return;
        }
        if props.list.is_some() {
            self.checkpoint(EditKind::Other);
            self.doc.para_mut(pr).props.list = None;
            self.touch();
            return;
        }
        if pr.block == 0 {
            return;
        }
        let prev_len = match &self.doc.blocks[pr.block - 1] {
            Block::Para(prev) => Some(prev.len()),
            Block::Table(_) => None,
        };
        match prev_len {
            Some(prev_len) => {
                self.checkpoint(EditKind::Other);
                let cells: Vec<Cell> = self.para().cells.to_vec();
                if let Block::Para(prev) = &mut self.doc.blocks[pr.block - 1] {
                    prev.cells_mut().extend(cells);
                }
                self.doc.blocks.remove(pr.block);
                self.cur = Pos::new(ParaRef::top(pr.block - 1), prev_len);
                self.touch();
            }
            None => {
                let last = self.doc.last_ref(pr.block - 1);
                self.move_to(Pos::new(last, self.doc.para(last).len()), false);
            }
        }
    }

    pub fn delete(&mut self) {
        if self.sel_range().is_some() {
            self.checkpoint(EditKind::Other);
            self.delete_selection_raw();
            return;
        }
        let Pos { pr, off } = self.cur;
        let mut k = off;
        while k < self.para().len() && self.hidden_at(pr, k) {
            k += 1;
        }
        if k < self.para().len() && self.track {
            self.checkpoint(EditKind::Delete);
            let end = self.track_delete(pr, k, k + 1);
            self.cur.off = end;
            return;
        }
        if k < self.para().len() {
            self.checkpoint(EditKind::Delete);
            self.doc.para_mut(pr).cells_mut().remove(k);
            self.touch();
            return;
        }
        if pr.cell.is_some() || pr.block + 1 >= self.doc.blocks.len() {
            return;
        }
        let next = match &self.doc.blocks[pr.block + 1] {
            Block::Para(next) => Some(next.cells.to_vec()),
            Block::Table(_) => None,
        };
        if let Some(cells) = next {
            self.checkpoint(EditKind::Other);
            self.doc.para_mut(pr).cells_mut().extend(cells);
            self.doc.blocks.remove(pr.block + 1);
            self.touch();
        }
    }

    pub fn delete_word_back(&mut self) {
        let a = self.word_left(self.cur);
        if a.pr == self.cur.pr && a.off < self.cur.off {
            self.checkpoint(EditKind::Other);
            self.delete_range_raw(a, self.cur);
        } else {
            self.backspace();
        }
    }

    pub fn delete_to_line_end(&mut self, layout: &Layout) {
        let (_, e) = layout.line_bounds(self.cur);
        let s = layout.seg_at(self.cur);
        let e = if s.map(|s| s.last).unwrap_or(true) {
            e
        } else {
            e + 1
        };
        if e > self.cur.off {
            self.checkpoint(EditKind::Other);
            self.delete_range_raw(self.cur, Pos::new(self.cur.pr, e));
        }
    }

    // ----- clipboard -----------------------------------------------------

    pub fn copy_range(&self, a: Pos, b: Pos) -> Vec<Block> {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return Vec::new();
        }
        if a.pr == b.pr {
            let p = self.doc.para(a.pr);
            return vec![Block::Para(Paragraph::with_cells(
                p.cells[a.off..b.off].to_vec(),
                p.props.clone(),
            ))];
        }
        if a.pr.cell.is_some() && a.pr.block == b.pr.block {
            let mut cells = Vec::new();
            for r in self.refs_between(a.pr, b.pr) {
                let p = self.doc.para(r);
                let s = if r == a.pr { a.off } else { 0 };
                let e = if r == b.pr { b.off } else { p.len() };
                if !cells.is_empty() {
                    cells.push(Cell::new('\t', Fmt::default()));
                }
                cells.extend_from_slice(&p.cells[s..e]);
            }
            return vec![Block::Para(Paragraph::with_cells(
                cells,
                ParaProps::default(),
            ))];
        }
        let Some((a, b)) = self.top_level_range(a, b) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let pa = self.doc.para(a.pr);
        out.push(Block::Para(Paragraph::with_cells(
            pa.cells[a.off..].to_vec(),
            pa.props.clone(),
        )));
        for bi in a.pr.block + 1..b.pr.block {
            out.push(self.doc.blocks[bi].clone());
        }
        let pb = self.doc.para(b.pr);
        out.push(Block::Para(Paragraph::with_cells(
            pb.cells[..b.off].to_vec(),
            pb.props.clone(),
        )));
        out
    }

    pub fn copy(&mut self) -> bool {
        match self.sel_range() {
            Some((a, b)) => {
                self.clipboard = self.copy_range(a, b);
                self.clip_store = Some(self.doc.store.clone());
                true
            }
            None => false,
        }
    }

    pub fn cut(&mut self) -> bool {
        if !self.copy() {
            return false;
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        true
    }

    pub fn delete_selection(&mut self) -> bool {
        if self.sel_range().is_none() {
            return false;
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw()
    }

    pub fn paste(&mut self) -> bool {
        if self.clipboard.is_empty() {
            return false;
        }
        let mut clip = self.clipboard.clone();
        if !self
            .clip_store
            .as_ref()
            .is_some_and(|s| std::rc::Rc::ptr_eq(s, &self.doc.store))
        {
            // Copied from another document: its objects and run properties
            // don't exist here, so keep only plain formatted text.
            for b in clip.iter_mut() {
                let paras: Vec<&mut Paragraph> = match b {
                    Block::Para(p) => vec![p],
                    Block::Table(t) => t.rows.iter_mut().flatten().collect(),
                };
                for p in paras {
                    p.extra = None;
                    let cells = p.cells_mut();
                    cells.retain(|c| !is_special(c.ch));
                    for c in cells.iter_mut() {
                        c.cp = 0;
                    }
                }
                if let Block::Table(t) = b {
                    t.extra = None;
                }
            }
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        self.paste_blocks(&clip);
        true
    }

    /// Paste blocks from outside (another program's clipboard).
    pub fn paste_foreign(&mut self, blocks: &[Block]) {
        if blocks.is_empty() {
            return;
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        self.paste_blocks(blocks);
    }

    fn paste_blocks(&mut self, clip: &[Block]) {
        let flat =
            self.cur.pr.cell.is_some() || (clip.len() == 1 && matches!(clip[0], Block::Para(_)));
        if flat {
            let mut cells: Vec<Cell> = Vec::new();
            for (i, b) in clip.iter().enumerate() {
                if i > 0 {
                    cells.push(Cell::new(' ', Fmt::default()));
                }
                match b {
                    Block::Para(p) => cells.extend(p.cells.iter().copied()),
                    Block::Table(t) => {
                        for p in t.rows.iter().flatten() {
                            cells.extend(p.cells.iter().copied());
                            cells.push(Cell::new(' ', Fmt::default()));
                        }
                    }
                }
            }
            let n = cells.len();
            let off = self.cur.off;
            self.doc
                .para_mut(self.cur.pr)
                .cells_mut()
                .splice(off..off, cells);
            self.cur.off += n;
            self.touch();
            return;
        }
        let pr = self.cur.pr;
        let bi = pr.block;
        let off = self.cur.off;
        let tail = self.doc.para_mut(pr).cells_mut().split_off(off);
        let tail_props = self.para().props.clone();
        let rest = match &clip[0] {
            Block::Para(first) => {
                self.doc
                    .para_mut(pr)
                    .cells_mut()
                    .extend(first.cells.iter().copied());
                &clip[1..]
            }
            Block::Table(_) => clip,
        };
        let mut at = bi + 1;
        for b in rest {
            self.doc.blocks.insert(at, b.clone());
            at += 1;
        }
        match self.doc.blocks.get_mut(at - 1) {
            Some(Block::Para(p)) if at - 1 > bi => {
                let len = p.len();
                p.cells_mut().extend(tail);
                self.cur = Pos::new(ParaRef::top(at - 1), len);
            }
            _ => {
                let mut props = tail_props;
                props.page_break_before = false;
                self.doc
                    .blocks
                    .insert(at, Block::Para(Paragraph::with_cells(tail, props)));
                self.cur = Pos::new(ParaRef::top(at), 0);
            }
        }
        self.doc.normalize();
        self.cur = self.doc.clamp(self.cur);
        self.touch();
    }

    // ----- formatting ----------------------------------------------------

    pub(crate) fn for_each_cell_in(&mut self, a: Pos, b: Pos, mut f: impl FnMut(&mut Cell)) {
        for r in self.refs_between(a.pr, b.pr) {
            let p = self.doc.para_mut(r);
            let s = if r == a.pr { a.off } else { 0 };
            let e = if r == b.pr { b.off } else { p.len() };
            for c in &mut p.cells_mut()[s..e] {
                if fn_id(c.ch).is_none() {
                    f(c);
                }
            }
        }
    }

    /// Formatting a character shows, with its paragraph and character
    /// styles applied.
    pub fn effective_fmt(&self, pr: ParaRef, c: &Cell) -> Fmt {
        let doc = &self.doc;
        let ps = doc
            .styles
            .get(&doc.para(pr).props.style)
            .map(|s| s.fmt)
            .unwrap_or_default();
        let cs = doc
            .store
            .borrow()
            .look(c.cp)
            .char_style
            .as_deref()
            .and_then(|id| doc.styles.get(id))
            .map(|s| s.fmt)
            .unwrap_or_default();
        Fmt::effective(ps.union(cs), c.fmt)
    }

    /// Toggle a character attribute on the selection, or for new typing.
    /// Returns the new state.
    pub fn toggle_fmt(&mut self, flag: u16) -> bool {
        match self.sel_range() {
            Some((a, b)) => {
                let mut all = true;
                for r in self.refs_between(a.pr, b.pr) {
                    let p = self.doc.para(r);
                    let s = if r == a.pr { a.off } else { 0 };
                    let e = if r == b.pr { b.off } else { p.len() };
                    if p.cells[s..e]
                        .iter()
                        .any(|c| !is_special(c.ch) && !self.effective_fmt(r, c).has(flag))
                    {
                        all = false;
                    }
                }
                self.checkpoint(EditKind::Other);
                self.for_each_cell_in(a, b, |c| c.fmt = c.fmt.with(flag, !all));
                self.touch();
                !all
            }
            None => {
                let probe = Cell::with_cp(' ', self.typing_fmt, self.typing_cp);
                let on = !self.effective_fmt(self.cur.pr, &probe).has(flag);
                self.typing_fmt = self.typing_fmt.with(flag, on);
                on
            }
        }
    }

    /// Set (rather than toggle) a character attribute.
    pub fn set_fmt(&mut self, flag: u16, on: bool) {
        match self.sel_range() {
            Some((a, b)) => {
                self.checkpoint(EditKind::Other);
                self.for_each_cell_in(a, b, |c| c.fmt = c.fmt.with(flag, on));
                self.touch();
            }
            None => self.typing_fmt = self.typing_fmt.with(flag, on),
        }
    }

    pub fn clear_fmt(&mut self) {
        match self.sel_range() {
            Some((a, b)) => {
                self.checkpoint(EditKind::Other);
                self.for_each_cell_in(a, b, |c| c.fmt = Fmt::default());
                self.touch();
            }
            None => self.typing_fmt = Fmt::default(),
        }
    }

    /// Paragraphs affected by a paragraph-level command.
    pub fn target_paras(&self) -> Vec<ParaRef> {
        match self.sel_range() {
            Some((a, b)) => self.refs_between(a.pr, b.pr),
            None => vec![self.cur.pr],
        }
    }

    /// Apply a paragraph style: the style's own paragraph formatting
    /// replaces direct formatting (the list is kept).
    pub fn apply_style(&mut self, id: &str) {
        let props = self.doc.style_props(id);
        self.set_props(|p| {
            let list = p.list;
            *p = props.clone();
            p.list = list;
        });
    }

    /// Change run properties (font, size, colour...) of the selection, or of
    /// new typing when nothing is selected.
    pub fn apply_char_edit(&mut self, edit: &crate::props::CharEdit) {
        if edit.is_empty() {
            return;
        }
        let Some((a, b)) = self.sel_range() else {
            let cp = self.typing_cp;
            self.typing_cp = self
                .doc
                .store
                .borrow_mut()
                .derive(cp, |c| edit.apply(&mut c.rpr));
            return;
        };
        self.checkpoint(EditKind::Other);
        let store = self.doc.store.clone();
        let mut memo: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        self.for_each_cell_in(a, b, |c| {
            let cp = c.cp;
            c.cp = *memo
                .entry(cp)
                .or_insert_with(|| store.borrow_mut().derive(cp, |p| edit.apply(&mut p.rpr)));
        });
        self.touch();
    }

    pub fn set_props(&mut self, mut f: impl FnMut(&mut ParaProps)) {
        self.checkpoint(EditKind::Other);
        for r in self.target_paras() {
            f(&mut self.doc.para_mut(r).props);
        }
        self.touch();
    }

    pub fn toggle_list(&mut self, kind: ListKind) {
        let targets = self.target_paras();
        let all = targets
            .iter()
            .all(|&r| self.doc.para(r).props.list.map(|l| l.kind) == Some(kind));
        self.set_props(|p| {
            p.list = if all {
                None
            } else {
                Some(ListInfo::new(kind, p.list.map(|l| l.level).unwrap_or(0)))
            };
        });
    }

    /// Change list level (or indent for non-list paragraphs).
    pub fn change_level(&mut self, delta: i32) {
        self.set_props(|p| match &mut p.list {
            Some(li) => li.level = (li.level as i32 + delta).clamp(0, 8) as u8,
            None => p.indent = (p.indent as i32 + delta * 5).clamp(0, 60) as u16,
        });
    }

    // ----- tables --------------------------------------------------------

    pub fn insert_table(&mut self, rows: usize, cols: usize) -> bool {
        if self.cur.pr.cell.is_some() {
            return false;
        }
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        let t = Table::new(rows, cols, self.doc.page.text_cols());
        let bi = self.cur.pr.block;
        let mut ti = if self.para().is_empty() || self.cur.off == 0 {
            bi
        } else {
            self.split_raw(false);
            bi + 1
        };
        if ti == 0 {
            self.doc.blocks.insert(0, Block::Para(Paragraph::new()));
            ti = 1;
        }
        self.doc.blocks.insert(ti, Block::Table(t));
        self.doc.normalize();
        self.cur = Pos::new(ParaRef::cell(ti, 0, 0), 0);
        self.anchor = None;
        self.touch();
        true
    }

    fn table_pos(&self) -> Option<(usize, usize, usize)> {
        let (r, c) = self.cur.pr.cell?;
        Some((self.cur.pr.block, r as usize, c as usize))
    }

    pub fn next_cell(&mut self, add_row: bool) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        let t = self.doc.table(bi).unwrap();
        let (nr, nc) = (t.rows.len(), t.ncols());
        let target = self
            .doc
            .next_ref(self.cur.pr)
            .filter(|p| p.block == bi)
            .and_then(|p| p.cell)
            .map(|(r, c)| (r as usize, c as usize));
        let _ = (r, c);
        match target {
            Some((r, c)) => self.move_to(Pos::new(ParaRef::cell(bi, r, c), 0), false),
            None if add_row => {
                self.checkpoint(EditKind::Other);
                self.doc
                    .table_mut(bi)
                    .unwrap()
                    .rows
                    .push(vec![Paragraph::new(); nc]);
                self.cur = Pos::new(ParaRef::cell(bi, nr, 0), 0);
                self.touch();
            }
            None => {
                let next = self.doc.first_ref(bi + 1);
                self.move_to(Pos::new(next, 0), false);
            }
        }
        true
    }

    pub fn prev_cell(&mut self) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        let Some(target) = self
            .doc
            .prev_ref(self.cur.pr)
            .filter(|p| p.block == bi)
            .and_then(|p| p.cell)
            .map(|(r, c)| (r as usize, c as usize))
        else {
            return true;
        };
        let _ = (r, c);
        self.move_to(Pos::new(ParaRef::cell(bi, target.0, target.1), 0), false);
        true
    }

    pub fn table_add_row(&mut self) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        self.checkpoint(EditKind::Other);
        let t = self.doc.table_mut(bi).unwrap();
        let n = t.ncols();
        t.rows.insert(r + 1, vec![Paragraph::new(); n]);
        self.cur = Pos::new(ParaRef::cell(bi, r + 1, c), 0);
        self.touch();
        true
    }

    pub fn table_add_col(&mut self) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        self.checkpoint(EditKind::Other);
        let width = self.doc.page.text_cols();
        let t = self.doc.table_mut(bi).unwrap();
        for row in t.rows.iter_mut() {
            row.insert(c + 1, Paragraph::new());
        }
        let n = t.ncols() + 1;
        t.cols = Table::new(1, n, width).cols;
        self.cur = Pos::new(ParaRef::cell(bi, r, c + 1), 0);
        self.touch();
        true
    }

    pub fn table_del_row(&mut self) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        if self.doc.table(bi).unwrap().rows.len() <= 1 {
            return self.table_delete();
        }
        self.checkpoint(EditKind::Other);
        let t = self.doc.table_mut(bi).unwrap();
        t.rows.remove(r);
        let r = r.min(t.rows.len() - 1);
        self.cur = Pos::new(ParaRef::cell(bi, r, c), 0);
        self.touch();
        true
    }

    pub fn table_del_col(&mut self) -> bool {
        let Some((bi, r, c)) = self.table_pos() else {
            return false;
        };
        if self.doc.table(bi).unwrap().ncols() <= 1 {
            return self.table_delete();
        }
        self.checkpoint(EditKind::Other);
        let width = self.doc.page.text_cols();
        let t = self.doc.table_mut(bi).unwrap();
        for row in t.rows.iter_mut() {
            row.remove(c);
        }
        t.cols.remove(c);
        t.cols = Table::new(1, t.cols.len(), width).cols;
        let c = c.min(t.ncols() - 1);
        self.cur = Pos::new(ParaRef::cell(bi, r, c), 0);
        self.touch();
        true
    }

    pub fn table_delete(&mut self) -> bool {
        let Some((bi, _, _)) = self.table_pos() else {
            return false;
        };
        self.checkpoint(EditKind::Other);
        self.doc.blocks.remove(bi);
        self.doc.normalize();
        let bi = bi.min(self.doc.blocks.len() - 1);
        self.cur = Pos::new(self.doc.first_ref(bi), 0);
        self.anchor = None;
        self.touch();
        true
    }

    // ----- footnotes -----------------------------------------------------

    pub fn insert_footnote(&mut self, text: String) {
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        let id = self.doc.new_footnote(text);
        self.insert_cell_raw(Cell::new(fn_char(id), Fmt::default()));
    }

    /// Footnote reference just before or at the cursor.
    pub fn footnote_near_cursor(&self) -> Option<u32> {
        let cells = &self.para().cells;
        let off = self.cur.off;
        [off.checked_sub(1), Some(off)]
            .into_iter()
            .flatten()
            .filter_map(|i| cells.get(i))
            .find_map(|c| fn_id(c.ch))
    }

    pub fn set_footnote(&mut self, id: u32, text: String) {
        self.checkpoint(EditKind::Other);
        self.doc.footnotes.insert(id, text);
        self.touch();
    }

    // ----- document-level ------------------------------------------------

    /// Apply a change to page setup, header/footer and so on.
    pub fn edit_doc(&mut self, f: impl FnOnce(&mut Document)) {
        self.checkpoint(EditKind::Other);
        f(&mut self.doc);
        self.touch();
    }

    // ----- search --------------------------------------------------------

    fn match_at(cells: &[Cell], needle: &[char], i: usize) -> bool {
        i + needle.len() <= cells.len()
            && needle
                .iter()
                .enumerate()
                .all(|(k, &n)| lower(cells[i + k].ch) == n)
    }

    pub fn find(&mut self, query: &str, forward: bool) -> FindResult {
        let needle: Vec<char> = query.chars().map(lower).collect();
        if needle.is_empty() {
            return FindResult::NotFound;
        }
        let refs = self.doc.refs();
        let n = refs.len();
        let start_i = refs.iter().position(|r| *r == self.cur.pr).unwrap_or(0);
        let from = match (forward, self.sel_range()) {
            (false, Some((a, _))) => a.off,
            _ => self.cur.off,
        };
        for step in 0..=n {
            let i = if forward {
                (start_i + step) % n
            } else {
                (start_i + n * 2 - step) % n
            };
            let cells = &self.doc.para(refs[i]).cells;
            let found = if forward {
                let s = if step == 0 { from } else { 0 };
                (s..cells.len()).find(|&k| Self::match_at(cells, &needle, k))
            } else {
                let before = if step == 0 { from } else { cells.len() + 1 };
                (0..cells.len().min(before))
                    .rev()
                    .find(|&k| Self::match_at(cells, &needle, k))
            };
            if let Some(k) = found {
                let a = Pos::new(refs[i], k);
                let b = Pos::new(refs[i], k + needle.len());
                if forward {
                    self.select_range(a, b);
                } else {
                    self.select_range(b, a);
                }
                let wrapped = step > 0 && (if forward { i <= start_i } else { i >= start_i });
                return if wrapped {
                    FindResult::Wrapped
                } else {
                    FindResult::Found
                };
            }
        }
        FindResult::NotFound
    }

    /// Replace the selection with text, keeping the selection's formatting.
    pub fn replace_selection(&mut self, text: &str) {
        let Some((a, _)) = self.sel_range() else {
            return;
        };
        let fmt = self
            .doc
            .para(a.pr)
            .cells
            .get(a.off)
            .map(|c| c.fmt)
            .unwrap_or(self.typing_fmt);
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        let cp = self
            .doc
            .para(a.pr)
            .cells
            .get(a.off)
            .map(|c| c.cp)
            .unwrap_or(self.typing_cp);
        for ch in text.chars() {
            self.insert_cell_raw(Cell::with_cp(ch, fmt, cp));
        }
    }

    /// Replace a range in the current paragraph (spell check, thesaurus).
    pub fn replace_in_para(&mut self, pr: ParaRef, s: usize, e: usize, text: &str) {
        self.select_range(Pos::new(pr, s), Pos::new(pr, e));
        self.replace_selection(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::new();
        e.insert_text(s);
        e
    }

    fn text(e: &Editor) -> String {
        e.doc.to_plain_text()
    }

    #[test]
    fn typing_and_newlines() {
        let e = typed("Hello\nWorld");
        assert_eq!(text(&e), "Hello\nWorld\n");
        assert_eq!(e.cur, Pos::new(ParaRef::top(1), 5));
    }

    #[test]
    fn backspace_merges_paragraphs_and_undo_restores() {
        let mut e = typed("ab\ncd");
        e.move_to(Pos::new(ParaRef::top(1), 0), false);
        e.backspace();
        assert_eq!(text(&e), "abcd\n");
        assert_eq!(e.cur.off, 2);
        e.undo();
        assert_eq!(text(&e), "ab\ncd\n");
        e.redo();
        assert_eq!(text(&e), "abcd\n");
    }

    #[test]
    fn typing_coalesces_into_one_undo() {
        let mut e = Editor::new();
        for c in "hello".chars() {
            e.insert_char(c);
        }
        e.undo();
        assert_eq!(text(&e), "\n");
    }

    #[test]
    fn cut_paste_across_paragraphs() {
        let mut e = typed("one\ntwo\nthree");
        e.select_range(Pos::new(ParaRef::top(0), 1), Pos::new(ParaRef::top(2), 2));
        assert!(e.cut());
        assert_eq!(text(&e), "oree\n");
        e.move_to(Pos::new(ParaRef::top(0), 4), false);
        e.paste();
        assert_eq!(text(&e), "oreene\ntwo\nth\n");
    }

    #[test]
    fn bold_selection_toggles() {
        let mut e = typed("hello world");
        e.select_range(Pos::new(ParaRef::top(0), 0), Pos::new(ParaRef::top(0), 5));
        assert!(e.toggle_fmt(Fmt::BOLD));
        assert!(e.doc.para(ParaRef::top(0)).cells[0].fmt.has(Fmt::BOLD));
        assert!(!e.doc.para(ParaRef::top(0)).cells[6].fmt.has(Fmt::BOLD));
        assert!(!e.toggle_fmt(Fmt::BOLD));
        assert!(!e.doc.para(ParaRef::top(0)).cells[0].fmt.has(Fmt::BOLD));
    }

    #[test]
    fn tables_insert_and_navigate() {
        let mut e = typed("before");
        e.insert_table(2, 3);
        assert!(e.cur.pr.cell.is_some());
        e.insert_text("A");
        e.next_cell(true);
        e.insert_text("B");
        for _ in 0..4 {
            e.next_cell(true);
        }
        e.next_cell(true); // adds a row
        assert_eq!(e.doc.table(1).unwrap().rows.len(), 3);
        assert_eq!(text(&e), "before\nA\tB\t\n\t\t\n\t\t\n\n");
    }

    #[test]
    fn find_wraps() {
        let mut e = typed("cat dog cat");
        e.move_to(e.doc_start(), false);
        assert_eq!(e.find("CAT", true), FindResult::Found);
        assert_eq!(e.sel_range().unwrap().0.off, 0);
        assert_eq!(e.find("CAT", true), FindResult::Found);
        assert_eq!(e.sel_range().unwrap().0.off, 8);
        assert_eq!(e.find("cat", false), FindResult::Found);
        assert_eq!(e.sel_range().unwrap().0.off, 0);
        e.move_to(Pos::new(ParaRef::top(0), 11), false);
        assert_eq!(e.find("cat", true), FindResult::Wrapped);
        assert_eq!(e.sel_range().unwrap().0.off, 0);
        assert_eq!(e.find("zebra", true), FindResult::NotFound);
    }

    #[test]
    fn list_enter_on_empty_item_ends_list() {
        let mut e = typed("item");
        e.toggle_list(ListKind::Bullet);
        e.newline();
        assert!(e.para().props.list.is_some());
        e.newline();
        assert!(e.para().props.list.is_none());
    }
}
