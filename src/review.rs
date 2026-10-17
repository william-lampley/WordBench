//! Word review and linking features on the editor: hyperlinks, comments,
//! tracked changes (showing, accepting/rejecting, and recording your own
//! edits) and pictures.

use crate::editor::{EditKind, Editor};
use crate::io::xml::{XNode, esc, parse};
use crate::model::*;
use std::collections::HashMap;
use std::path::Path;

/// Current time as ISO 8601 UTC (Word's revision and comment dates).
pub fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn parse_one(xml: &str) -> Option<XNode> {
    parse(xml)
        .ok()?
        .children
        .into_iter()
        .find(|c| c.name != "#text")
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase()
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// A deleted-text object (tracked deletion or move).
pub fn is_deletion(o: &InlineObj) -> bool {
    o.xml.starts_with("<w:del ") || o.xml.starts_with("<w:del>") || o.xml.starts_with("<w:moveFrom")
}

/// Width and height in pixels and the file extension of an image.
pub fn image_size(b: &[u8]) -> Option<(u32, u32, &'static str)> {
    let be16 = |i: usize| Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]) as u32);
    if b.starts_with(b"\x89PNG") && b.len() >= 24 {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h, "png"));
    }
    if b.starts_with(b"GIF8") && b.len() >= 10 {
        return Some((
            u16::from_le_bytes([b[6], b[7]]) as u32,
            u16::from_le_bytes([b[8], b[9]]) as u32,
            "gif",
        ));
    }
    if b.starts_with(b"BM") && b.len() >= 26 {
        let w = i32::from_le_bytes(b[18..22].try_into().ok()?).unsigned_abs();
        let h = i32::from_le_bytes(b[22..26].try_into().ok()?).unsigned_abs();
        return Some((w, h, "bmp"));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                return Some((be16(i + 7)?, be16(i + 5)?, "jpeg"));
            }
            i += 2 + be16(i + 2)? as usize;
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    Inserted,
    Deleted,
    Formatted,
}

impl Editor {
    // ----- hyperlinks ----------------------------------------------------

    /// Make the selection a hyperlink to `url` (a web address or #bookmark).
    pub fn add_link(&mut self, url: &str) -> Result<(), &'static str> {
        let Some((a, b)) = self.sel_range() else {
            return Err("Select the text to link first");
        };
        if a.pr != b.pr {
            return Err("A link can't cross paragraphs");
        }
        let url = url.trim();
        let (open, rel) = match url.strip_prefix('#') {
            Some(anchor) => (
                format!(r#"<w:hyperlink w:anchor="{}" w:history="1">"#, esc(anchor)),
                None,
            ),
            None => {
                let id = format!("rIdWbLink{}", self.doc.new_rels.len() + 1);
                (
                    format!(r#"<w:hyperlink r:id="{id}" w:history="1">"#,),
                    Some(NewRel {
                        id,
                        ty: "hyperlink".into(),
                        target: url.into(),
                        external: true,
                    }),
                )
            }
        };
        self.checkpoint(EditKind::Other);
        if let Some(r) = rel {
            self.doc.new_rels.push(r);
        }
        self.doc.added_styles.insert("Hyperlink".into());
        let w = self.doc.store.borrow_mut().add_wrap(WrapDef {
            open,
            close: "</w:hyperlink>".into(),
            label: "Link".into(),
            kind: WrapKind::Link,
            target: url.into(),
        });
        let store = self.doc.store.clone();
        let mut memo: HashMap<u32, u32> = HashMap::new();
        self.for_each_cell_in(a, b, |c| {
            let cp = c.cp;
            c.cp = *memo.entry(cp).or_insert_with(|| {
                store.borrow_mut().derive(cp, |p| {
                    p.wraps.push(w);
                    if !p.rpr.iter().any(|(n, _)| n == "rStyle") {
                        p.rpr
                            .push(("rStyle".into(), r#"<w:rStyle w:val="Hyperlink"/>"#.into()));
                    }
                })
            });
        });
        self.touch();
        Ok(())
    }

    /// The hyperlink at (or just before) the cursor: wrapper id and target.
    pub fn link_at_cursor(&self) -> Option<(u32, String)> {
        let p = self.para();
        let st = self.doc.store.borrow();
        [Some(self.cur.off), self.cur.off.checked_sub(1)]
            .into_iter()
            .flatten()
            .filter_map(|i| p.cells.get(i))
            .find_map(|c| {
                st.cprops(c.cp).wraps.iter().rev().find_map(|&w| {
                    let d = st.wraps.get(w as usize)?;
                    (d.kind == WrapKind::Link).then(|| (w, d.target.clone()))
                })
            })
    }

    pub fn remove_link(&mut self) -> bool {
        let Some((w, _)) = self.link_at_cursor() else {
            return false;
        };
        self.checkpoint(EditKind::Other);
        let store = self.doc.store.clone();
        let pr = self.cur.pr;
        let cells = self.doc.para_mut(pr).cells_mut();
        for c in cells.iter_mut() {
            if store.borrow().cprops(c.cp).wraps.contains(&w) {
                c.cp = store.borrow_mut().derive(c.cp, |p| {
                    p.wraps.retain(|x| *x != w);
                    p.rpr
                        .retain(|(n, x)| !(n == "rStyle" && x.contains("Hyperlink")));
                });
            }
        }
        self.touch();
        true
    }

    // ----- comments ------------------------------------------------------

    fn comment_obj(&mut self, tag: &str, id: &str, kind: ObjKind, label: String) -> Cell {
        let ch = self.doc.new_obj(InlineObj {
            kind,
            xml: format!(r#"<w:{tag} w:id="{}"/>"#, esc(id)),
            label,
            in_run: tag == "commentReference",
        });
        Cell::new(ch, Fmt::default())
    }

    /// Add a comment on the selection (or at the cursor).
    pub fn add_comment(&mut self, text: &str) -> String {
        let id = self
            .doc
            .comments
            .keys()
            .filter_map(|k| k.parse::<u32>().ok())
            .chain(
                self.comment_positions()
                    .iter()
                    .filter_map(|(k, _)| k.parse::<u32>().ok()),
            )
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
            .to_string();
        let (a, b) = self.sel_range().unwrap_or((self.cur, self.cur));
        self.checkpoint(EditKind::Other);
        let author = self.author.clone();
        let label = format!("[Comment: {}]", truncate(&author, 16));
        let end = self.comment_obj(
            "commentRangeEnd",
            &id,
            ObjKind::Hidden,
            "Comment end".into(),
        );
        let reference = self.comment_obj("commentReference", &id, ObjKind::Visible, label);
        let start = self.comment_obj("commentRangeStart", &id, ObjKind::Hidden, "Comment".into());
        self.doc
            .para_mut(b.pr)
            .cells_mut()
            .splice(b.off..b.off, [end, reference]);
        self.doc.para_mut(a.pr).cells_mut().insert(a.off, start);
        self.doc.comments.insert(
            id.clone(),
            Comment {
                initials: initials(&author),
                author,
                date: utc_now(),
                text: text.into(),
                xml: String::new(),
                orig_text: String::new(),
            },
        );
        self.anchor = None;
        self.cur = Pos::new(b.pr, if a.pr == b.pr { b.off + 3 } else { b.off + 2 });
        self.touch();
        id
    }

    /// Every comment reference in the document with its position.
    pub fn comment_positions(&self) -> Vec<(String, Pos)> {
        let st = self.doc.store.borrow();
        let mut out = Vec::new();
        for pr in self.doc.refs() {
            for (k, c) in self.doc.para(pr).cells.iter().enumerate() {
                if let Some(o) = st
                    .obj(c.ch)
                    .filter(|o| o.xml.starts_with("<w:commentReference"))
                    && let Some(id) =
                        parse_one(&o.xml).and_then(|n| n.attr("id").map(str::to_string))
                {
                    out.push((id, Pos::new(pr, k)));
                }
            }
        }
        out
    }

    /// The comment whose reference is at or just before the cursor.
    pub fn comment_at_cursor(&self) -> Option<String> {
        let here = self.cur;
        self.comment_positions()
            .into_iter()
            .find(|(_, p)| p.pr == here.pr && (p.off == here.off || p.off + 1 == here.off))
            .map(|(id, _)| id)
    }

    pub fn edit_comment(&mut self, id: &str, text: String) {
        self.checkpoint(EditKind::Other);
        if let Some(c) = self.doc.comments.get_mut(id) {
            c.text = text;
        }
        self.touch();
    }

    /// Remove a comment and its anchors from the text.
    pub fn delete_comment(&mut self, id: &str) {
        self.checkpoint(EditKind::Other);
        let st = self.doc.store.clone();
        let mine = |c: &Cell| {
            st.borrow().obj(c.ch).is_some_and(|o| {
                o.xml.starts_with("<w:comment")
                    && parse_one(&o.xml).is_some_and(|n| n.attr("id") == Some(id))
            })
        };
        for pr in self.doc.refs() {
            if self.doc.para(pr).cells.iter().any(mine) {
                self.doc.para_mut(pr).cells_mut().retain(|c| !mine(c));
            }
        }
        self.doc.comments.remove(id);
        self.cur = self.doc.clamp(self.cur);
        self.touch();
    }

    // ----- tracked changes -------------------------------------------------

    /// Run properties for text typed while tracking changes.
    pub(crate) fn tracked_cp(&mut self, cp: u32) -> u32 {
        let author = self.author.clone();
        let w = match self.track_wrap {
            Some(w) => w,
            None => {
                self.rev_id += 1;
                let open = format!(
                    r#"<w:ins w:id="{}" w:author="{}" w:date="{}">"#,
                    self.rev_id,
                    esc(&author),
                    utc_now()
                );
                let w = self.doc.store.borrow_mut().add_wrap(WrapDef {
                    open,
                    close: "</w:ins>".into(),
                    label: "Insert".into(),
                    kind: WrapKind::Insert,
                    target: author,
                });
                self.track_wrap = Some(w);
                w
            }
        };
        if self.doc.store.borrow().cprops(cp).wraps.contains(&w) {
            return cp;
        }
        self.doc.store.borrow_mut().derive(cp, |p| p.wraps.push(w))
    }

    fn is_inserted(&self, c: &Cell) -> bool {
        let st = self.doc.store.borrow();
        st.cprops(c.cp).wraps.iter().any(|&w| {
            st.wraps
                .get(w as usize)
                .is_some_and(|d| d.kind == WrapKind::Insert)
        })
    }

    fn change_of(&self, c: &Cell) -> Change {
        let st = self.doc.store.borrow();
        if st.obj(c.ch).is_some_and(is_deletion) {
            return Change::Deleted;
        }
        drop(st);
        if self.is_inserted(c) {
            return Change::Inserted;
        }
        if self
            .doc
            .store
            .borrow()
            .cprops(c.cp)
            .rpr
            .iter()
            .any(|(n, _)| n == "rPrChange")
        {
            return Change::Formatted;
        }
        Change::None
    }

    /// A tracked-deletion object holding `cells`.
    fn deletion_obj(&mut self, cells: Vec<Cell>) -> Cell {
        self.rev_id += 1;
        let st = self.doc.store.clone();
        let mut xml = format!(
            r#"<w:del w:id="{}" w:author="{}" w:date="{}">"#,
            self.rev_id,
            esc(&self.author),
            utc_now()
        );
        let mut text = String::new();
        let mut i = 0;
        while i < cells.len() {
            let key = (cells[i].fmt, cells[i].cp);
            let mut j = i;
            let mut run = String::new();
            while j < cells.len() && (cells[j].fmt, cells[j].cp) == key {
                match cells[j].ch {
                    '\t' => run.push_str("</w:delText><w:tab/><w:delText xml:space=\"preserve\">"),
                    ch => run.push_str(&esc(&ch.to_string())),
                }
                text.push(cells[j].ch);
                j += 1;
            }
            let rpr = crate::io::docx::rpr_xml(&st.borrow(), key.0, key.1, Fmt::default(), false);
            xml.push_str(&format!(
                r#"<w:r>{rpr}<w:delText xml:space="preserve">{run}</w:delText></w:r>"#
            ));
            i = j;
        }
        xml.push_str("</w:del>");
        let label = format!("[Deleted: {}]", truncate(&text, 24));
        let ch = self.doc.new_obj(InlineObj {
            kind: ObjKind::Visible,
            xml,
            label,
            in_run: false,
        });
        self.our_dels.insert(ch, cells);
        Cell::new(ch, Fmt(Fmt::STRIKE))
    }

    /// Delete `s..e` of a paragraph as a tracked change. Text that was
    /// itself a tracked insertion is simply removed. Returns where the
    /// deletion marker ends up.
    pub(crate) fn track_delete(&mut self, pr: ParaRef, mut s: usize, mut e: usize) -> usize {
        let cells = self.doc.para(pr).cells.to_vec();
        let st = self.doc.store.clone();
        let mut group: Vec<Cell> = Vec::new();
        let mut kept: Vec<Cell> = Vec::new();
        for c in &cells[s..e] {
            let kind = st.borrow().obj(c.ch).map(|o| (o.kind, is_deletion(o)));
            match kind {
                Some((ObjKind::Hidden, _)) | Some((_, true)) => kept.push(*c),
                _ if self.is_inserted(c) => {}
                Some(_) => {} // pictures etc. are deleted outright
                None => group.push(*c),
            }
        }
        // Join with our own neighbouring deletions.
        if s > 0
            && let Some(prev) = self.our_dels.get(&cells[s - 1].ch).cloned()
        {
            group.splice(0..0, prev);
            s -= 1;
        }
        if e < cells.len()
            && let Some(next) = self.our_dels.get(&cells[e].ch).cloned()
        {
            group.extend(next);
            e += 1;
        }
        let mut out = Vec::new();
        if !group.is_empty() {
            out.push(self.deletion_obj(group));
        }
        let marker_end = s + out.len();
        out.extend(kept);
        self.doc.para_mut(pr).cells_mut().splice(s..e, out);
        self.touch();
        marker_end
    }

    /// Restore the text inside a deletion object.
    fn undeleted_cells(&mut self, o: &InlineObj, around: &Cell) -> Vec<Cell> {
        if let Some(cells) = self.our_dels.get(&around.ch) {
            return cells.clone();
        }
        let wraps = self.doc.store.borrow().cprops(around.cp).wraps.clone();
        let Some(node) = parse_one(&o.xml) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for r in node.children.iter().filter(|c| c.name == "r") {
            let rpr: Vec<(String, String)> = r
                .child("rPr")
                .map(|p| {
                    p.children
                        .iter()
                        .filter(|c| c.name != "#text")
                        .map(|c| (c.name.clone(), c.to_xml()))
                        .collect()
                })
                .unwrap_or_default();
            let fmt = crate::props::fmt_from_rpr(&rpr);
            let cp = self.doc.store.borrow_mut().intern(CharProps {
                rpr,
                wraps: wraps.clone(),
            });
            for c in &r.children {
                match c.name.as_str() {
                    "delText" | "t" => out.extend(
                        c.all_text()
                            .chars()
                            .filter(|ch| !ch.is_control())
                            .map(|ch| Cell::with_cp(ch, fmt, cp)),
                    ),
                    "tab" => out.push(Cell::with_cp('\t', fmt, cp)),
                    _ => {}
                }
            }
        }
        out
    }

    /// Accept or reject tracked changes in the selection (or everywhere).
    /// Returns the number of changes handled.
    pub fn review(&mut self, accept: bool, range: Option<(Pos, Pos)>) -> usize {
        let refs = match range {
            Some((a, b)) => self.refs_between(a.pr, b.pr),
            None => self.doc.refs(),
        };
        self.checkpoint(EditKind::Other);
        let st = self.doc.store.clone();
        let ins_wraps: std::collections::HashSet<u32> = st
            .borrow()
            .wraps
            .iter()
            .enumerate()
            .filter(|(_, d)| d.kind == WrapKind::Insert)
            .map(|(i, _)| i as u32)
            .collect();
        let mut count = 0;
        for pr in refs {
            let cells = self.doc.para(pr).cells.to_vec();
            let (s, e) = match range {
                Some((a, b)) => (
                    if pr == a.pr { a.off } else { 0 },
                    if pr == b.pr { b.off } else { cells.len() },
                ),
                None => (0, cells.len()),
            };
            let mut out: Vec<Cell> = cells[..s].to_vec();
            let mut last = Change::None;
            for c in &cells[s..e] {
                let ch = self.change_of(c);
                if ch != Change::None && ch != last {
                    count += 1;
                }
                last = ch;
                match ch {
                    Change::None => out.push(*c),
                    Change::Deleted => {
                        if !accept {
                            let o = st.borrow().obj(c.ch).cloned();
                            if let Some(o) = o {
                                out.extend(self.undeleted_cells(&o, c));
                            }
                        }
                    }
                    Change::Inserted => {
                        if accept {
                            let mut c = *c;
                            c.cp = st
                                .borrow_mut()
                                .derive(c.cp, |p| p.wraps.retain(|w| !ins_wraps.contains(w)));
                            out.push(c);
                        }
                    }
                    Change::Formatted => {
                        let mut c = *c;
                        let props = st.borrow().cprops(c.cp).clone();
                        let mut np = props.clone();
                        let change = np
                            .rpr
                            .iter()
                            .position(|(n, _)| n == "rPrChange")
                            .map(|i| np.rpr.remove(i));
                        if let (false, Some((_, xml))) = (accept, change)
                            && let Some(old) = parse_one(&xml).and_then(|n| n.child("rPr").cloned())
                        {
                            np.rpr = old
                                .children
                                .iter()
                                .filter(|c| c.name != "#text")
                                .map(|c| (c.name.clone(), c.to_xml()))
                                .collect();
                            c.fmt = crate::props::fmt_from_rpr(&np.rpr);
                        }
                        c.cp = st.borrow_mut().intern(np);
                        out.push(c);
                    }
                }
            }
            out.extend_from_slice(&cells[e..]);
            if out != cells {
                *self.doc.para_mut(pr).cells_mut() = out;
            }
            // Paragraph-level changes: paragraph marks and formatting.
            let p = self.doc.para(pr);
            if let Some(x) = p.extra.clone() {
                let mut nx = (*x).clone();
                let mut changed = false;
                for item in nx.ppr.iter_mut() {
                    if item.0 == "rPr"
                        && (item.1.contains("<w:ins ") || item.1.contains("<w:del "))
                        && let Some(mut n) = parse_one(&item.1)
                    {
                        n.children.retain(|c| c.name != "ins" && c.name != "del");
                        item.1 = n.to_xml();
                        count += 1;
                        changed = true;
                    }
                }
                if let Some(i) = nx.ppr.iter().position(|(n, _)| n == "pPrChange") {
                    let (_, xml) = nx.ppr.remove(i);
                    count += 1;
                    changed = true;
                    if !accept
                        && let Some(old) = parse_one(&xml).and_then(|n| n.child("pPr").cloned())
                    {
                        let keep: Vec<(String, String)> = nx
                            .ppr
                            .iter()
                            .filter(|(n, _)| n == "rPr" || n == "sectPr")
                            .cloned()
                            .collect();
                        nx.ppr = old
                            .children
                            .iter()
                            .filter(|c| c.name != "#text")
                            .map(|c| (c.name.clone(), c.to_xml()))
                            .collect();
                        nx.ppr.extend(keep);
                        let style = nx
                            .ppr
                            .iter()
                            .find(|(n, _)| n == "pStyle")
                            .and_then(|(_, x)| parse_one(x))
                            .and_then(|n| n.attr("val").map(str::to_string))
                            .unwrap_or_default();
                        let mut props = self.doc.style_props(&style);
                        crate::props::apply_ppr(&mut props, &nx.ppr, &self.doc.numbering);
                        nx.at_read = Some(props.clone());
                        self.doc.para_mut(pr).props = props;
                    }
                }
                if changed {
                    self.doc.para_mut(pr).extra = Some(std::rc::Rc::new(nx));
                }
            }
        }
        self.anchor = None;
        self.cur = self.doc.clamp(self.cur);
        self.touch();
        count
    }

    /// Select the next tracked change after the cursor (wrapping around).
    pub fn next_change(&mut self) -> bool {
        let refs = self.doc.refs();
        let start = refs.iter().position(|r| *r == self.cur.pr).unwrap_or(0);
        for step in 0..=refs.len() {
            let pr = refs[(start + step) % refs.len()];
            let cells = self.doc.para(pr).cells.to_vec();
            let from = if step == 0 { self.cur.off } else { 0 };
            if let Some(k) =
                (from..cells.len()).find(|&k| self.change_of(&cells[k]) != Change::None)
            {
                let kind = self.change_of(&cells[k]);
                let mut e = k + 1;
                while e < cells.len() && self.change_of(&cells[e]) == kind {
                    e += 1;
                }
                self.select_range(Pos::new(pr, k), Pos::new(pr, e));
                return true;
            }
        }
        false
    }

    pub fn change_count(&self) -> usize {
        let mut n = 0;
        for pr in self.doc.refs() {
            let mut last = Change::None;
            for c in self.doc.para(pr).cells.iter() {
                let ch = self.change_of(c);
                if ch != Change::None && ch != last {
                    n += 1;
                }
                last = ch;
            }
        }
        n
    }

    // ----- captions and fields ---------------------------------------------

    fn field_obj(&mut self, xml: String, label: String) -> Cell {
        let ch = self.doc.new_obj(InlineObj {
            kind: ObjKind::Hidden,
            xml,
            label,
            in_run: true,
        });
        Cell::with_cp(ch, Fmt::default(), 0)
    }

    /// Labels used by caption numbers (SEQ fields) in the document.
    pub fn caption_labels(&self) -> Vec<String> {
        let st = self.doc.store.borrow();
        let mut v: Vec<String> = Vec::new();
        for o in &st.objects {
            if let Some(rest) = o.label.strip_prefix("Field:SEQ ")
                && let Some(l) = rest.split_whitespace().next()
            {
                let l = l.trim_matches('"').replace('_', " ");
                if !v.contains(&l) {
                    v.push(l);
                }
            }
        }
        v
    }

    /// Insert a caption ("Figure 3: text") above or below the table or
    /// picture at the cursor, numbered with a Word SEQ field.
    /// `format` is a Word number format switch: ARABIC, alphabetic,
    /// ALPHABETIC, roman, ROMAN.
    pub fn insert_caption(
        &mut self,
        label: &str,
        text: &str,
        above: bool,
        format: &str,
        exclude_label: bool,
    ) -> ParaRef {
        self.checkpoint(EditKind::Other);
        self.anchor = None;
        let block = self.cur.pr.block;
        let at = if above { block } else { block + 1 };
        let style = self
            .doc
            .styles
            .find("caption")
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "Caption".into());
        self.doc.added_styles.insert(style.clone());
        let ident = label.trim().replace(' ', "_");
        let mut cells: Vec<Cell> = Vec::new();
        let plain = |s: &str| {
            s.chars()
                .map(|c| Cell::new(c, Fmt::default()))
                .collect::<Vec<_>>()
        };
        if !exclude_label {
            cells.extend(plain(&format!("{} ", label.trim())));
        }
        let instr = format!(" SEQ {ident} \\* {format} ");
        cells.push(self.field_obj(
            r#"<w:fldChar w:fldCharType="begin"/>"#.into(),
            "Field".into(),
        ));
        cells.push(self.field_obj(
            format!(
                r#"<w:instrText xml:space="preserve">{}</w:instrText>"#,
                esc(&instr)
            ),
            format!("Field:{}", instr.trim()),
        ));
        cells.push(self.field_obj(
            r#"<w:fldChar w:fldCharType="separate"/>"#.into(),
            "Field result".into(),
        ));
        cells.extend(plain("1"));
        cells.push(self.field_obj(
            r#"<w:fldChar w:fldCharType="end"/>"#.into(),
            "Field end".into(),
        ));
        if !text.trim().is_empty() {
            cells.extend(plain(&format!(": {}", text.trim())));
        }
        let mut p = Paragraph::with_cells(cells, self.doc.style_props(&style));
        p.props.page_break_before = false;
        let at = at.min(self.doc.blocks.len());
        self.doc.blocks.insert(at, Block::Para(p));
        self.doc.normalize();
        let pr = ParaRef::top(at);
        self.update_fields();
        self.cur = Pos::new(pr, self.doc.para(pr).len());
        self.typing_fmt = Fmt::default();
        self.typing_cp = 0;
        self.touch();
        pr
    }

    /// Renumber caption (SEQ) fields in document order, as Word's Update
    /// Fields does. Returns how many were updated.
    pub fn update_fields(&mut self) -> usize {
        let st = self.doc.store.clone();
        let mut counters: HashMap<String, u32> = HashMap::new();
        let mut updated = 0;
        for pr in self.doc.refs() {
            let cells = self.doc.para(pr).cells.to_vec();
            let mut out: Vec<Cell> = Vec::with_capacity(cells.len());
            let mut i = 0;
            let mut changed = false;
            while i < cells.len() {
                let c = cells[i];
                out.push(c);
                i += 1;
                let instr = st
                    .borrow()
                    .obj(c.ch)
                    .and_then(|o| o.label.strip_prefix("Field:SEQ ").map(str::to_string));
                let Some(instr) = instr else { continue };
                let mut words = instr.split_whitespace();
                let ident = words.next().unwrap_or("").trim_matches('"').to_string();
                let rest: Vec<&str> = words.collect();
                let fmt = rest
                    .windows(2)
                    .find(|w| w[0] == "\\*")
                    .map(|w| w[1])
                    .unwrap_or("ARABIC");
                let n = counters.entry(ident).and_modify(|n| *n += 1).or_insert(1);
                let n = if let Some(r) = rest.windows(2).find(|w| w[0] == "\\r") {
                    let v = r[1].parse().unwrap_or(*n);
                    *n = v;
                    v
                } else {
                    *n
                };
                let num = match fmt {
                    "alphabetic" => crate::props::format_num("lowerLetter", n),
                    "ALPHABETIC" => crate::props::format_num("upperLetter", n),
                    "roman" => crate::props::format_num("lowerRoman", n),
                    "ROMAN" => crate::props::format_num("upperRoman", n),
                    _ => n.to_string(),
                };
                // Copy up to the separator, then replace the old result.
                while i < cells.len() {
                    let lbl = st.borrow().obj(cells[i].ch).map(|o| o.label.clone());
                    out.push(cells[i]);
                    i += 1;
                    if lbl.as_deref() == Some("Field result") {
                        break;
                    }
                }
                let start = i;
                while i < cells.len()
                    && st
                        .borrow()
                        .obj(cells[i].ch)
                        .map(|o| o.label.as_str() != "Field end")
                        .unwrap_or(true)
                {
                    i += 1;
                }
                let fmt_cell = cells
                    .get(start)
                    .filter(|c| obj_id(c.ch).is_none())
                    .copied()
                    .unwrap_or(Cell::new(' ', Fmt::default()));
                let new: Vec<Cell> = num
                    .chars()
                    .map(|ch| Cell::with_cp(ch, fmt_cell.fmt, fmt_cell.cp))
                    .collect();
                if cells[start..i]
                    .iter()
                    .map(|c| c.ch)
                    .ne(new.iter().map(|c| c.ch))
                {
                    changed = true;
                    updated += 1;
                }
                out.extend(new);
            }
            if changed {
                *self.doc.para_mut(pr).cells_mut() = out;
            }
        }
        if updated > 0 {
            self.cur = self.doc.clamp(self.cur);
            self.touch();
        }
        updated
    }

    // ----- pictures ------------------------------------------------------

    /// Insert a picture file at the cursor (saved into the .docx).
    pub fn insert_picture(&mut self, path: &Path) -> Result<String, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (w, h, ext) = image_size(&bytes).ok_or("Not a PNG, JPEG, GIF or BMP picture")?;
        let n = self.doc.new_parts.len() + 1;
        let rid = format!("rIdWbImg{n}");
        let dir = self
            .doc
            .docx
            .as_ref()
            .map(|p| {
                p.main
                    .rsplit_once('/')
                    .map(|x| x.0.to_string())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| "word".into());
        let file = format!("wb_image{n}.{ext}");
        let part = if dir.is_empty() {
            format!("media/{file}")
        } else {
            format!("{dir}/media/{file}")
        };
        // 96 pixels per inch, at most the width of the text.
        let max_in = self.doc.page.text_cols() as f32 / CPI;
        let mut win = w.max(1) as f32 / 96.0;
        let mut hin = h.max(1) as f32 / 96.0;
        if win > max_in {
            hin *= max_in / win;
            win = max_in;
        }
        let (cx, cy) = ((win * 914400.0) as u64, (hin * 914400.0) as u64);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.clone());
        let ename = esc(&name);
        let xml = format!(
            r#"<w:drawing><wp:inline xmlns:classic="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{}" name="{ename}"/><wp:cNvGraphicFramePr><a:graphicFrameLocks xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" noChangeAspect="1"/></wp:cNvGraphicFramePr><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="0" name="{ename}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#,
            50000 + n
        );
        self.checkpoint(EditKind::Other);
        self.delete_selection_raw();
        self.doc.new_parts.push((part, bytes));
        self.doc.new_rels.push(NewRel {
            id: rid,
            ty: "image".into(),
            target: format!("media/{file}"),
            external: false,
        });
        let label = format!("[Image: {}]", truncate(&name, 30));
        let ch = self.doc.new_obj(InlineObj {
            kind: ObjKind::Visible,
            xml,
            label,
            in_run: true,
        });
        self.insert_cell_raw(Cell::with_cp(ch, Fmt::default(), self.typing_cp));
        Ok(format!(
            "{name}: {w}x{h} pixels, {:.1}\" x {:.1}\"",
            win, hin
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&40u32.to_be_bytes());
        png.extend_from_slice(&20u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((40, 20, "png")));
        let gif = b"GIF89a\x0a\x00\x05\x00";
        assert_eq!(image_size(gif), Some((10, 5, "gif")));
    }

    #[test]
    fn iso_date() {
        let d = utc_now();
        assert_eq!(d.len(), 20);
        assert!(d.ends_with('Z'));
    }
}
