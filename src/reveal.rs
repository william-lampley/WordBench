//! Reveal Codes: the classic view of the text with its hidden
//! formatting codes ([BOLD]...[bold], [HRt], [SRt], [Tab], [Just:Center]...).

use crate::layout::Layout;
use crate::model::*;

#[derive(Clone, Debug)]
pub enum Tok {
    /// A text character; the cursor sits on it when at this position.
    Ch(char, Pos),
    /// A code; `Some(pos)` when the cursor can sit on it.
    Code(String, Option<Pos>),
    /// End of a line in the codes window.
    Break,
}

fn inches(cols: i32) -> String {
    let v = cols as f32 / CPI;
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}\"")
}

fn para_codes(p: &ParaProps, doc: &Document, out: &mut Vec<Tok>) {
    let mut code = |s: String| out.push(Tok::Code(s, None));
    if p.page_break_before {
        code("[HPg]".into());
    }
    if !doc.styles.is_default(&p.style) {
        code(format!(
            "[Style:{}]",
            crate::app::display_style_name(&doc.styles.name(&p.style))
        ));
    }
    if p.align != Align::Left {
        code(format!("[Just:{}]", Align::NAMES[p.align.index()]));
    }
    if p.spacing >= 2 {
        code("[Ln Spacing:2]".into());
    }
    if p.indent > 0 {
        code(format!("[Indent:{}]", inches(p.indent as i32)));
    }
    if p.first_indent != 0 {
        code(format!("[First Ln:{}]", inches(p.first_indent as i32)));
    }
    if let Some(li) = p.list {
        let kind = if li.kind == ListKind::Bullet {
            "Bullet"
        } else {
            "Par Num"
        };
        code(format!("[{kind}:{}]", li.level + 1));
    }
    if p.space_before > 0 || p.space_after > 0 {
        code(format!(
            "[Par Spacing:{}/{}]",
            p.space_before, p.space_after
        ));
    }
}

fn fmt_switch(from: Fmt, to: Fmt, out: &mut Vec<Tok>) {
    for (flag, _, name) in Fmt::ALL {
        if from.has(flag) && !to.has(flag) {
            out.push(Tok::Code(format!("[{}]", name.to_lowercase()), None));
        }
    }
    for (flag, _, name) in Fmt::ALL {
        if !from.has(flag) && to.has(flag) {
            out.push(Tok::Code(format!("[{name}]"), None));
        }
    }
}

/// Codes for changes in font, size, colour, highlight and character style.
fn look_codes(
    from: &crate::props::CharLook,
    to: &crate::props::CharLook,
    doc: &Document,
    out: &mut Vec<Tok>,
) {
    use crate::props::{color_name, highlight_name};
    let mut code = |s: String| out.push(Tok::Code(s, None));
    if (from.font.as_ref(), from.size) != (to.font.as_ref(), to.size) {
        let font = to.font.as_deref().map(|f| doc.font_name(f));
        let size = to
            .size
            .map(|h| format!("{}pt", crate::app::trim_num(h as f32 / 2.0)));
        match (font, size) {
            (None, None) => code("[font]".into()),
            (f, s) => code(format!(
                "[Font:{}]",
                [f, s].into_iter().flatten().collect::<Vec<_>>().join(" ")
            )),
        }
    }
    if from.color != to.color {
        code(match to.color {
            Some(c) => format!("[Color:{}]", color_name(c)),
            None => "[color]".into(),
        });
    }
    if from.background() != to.background() {
        code(match to.background() {
            Some(c) => format!("[Highlight:{}]", highlight_name(c)),
            None => "[highlight]".into(),
        });
    }
    if from.char_style != to.char_style {
        code(match &to.char_style {
            Some(id) => format!(
                "[Char Style:{}]",
                crate::app::display_style_name(&doc.styles.name(id))
            ),
            None => "[char style]".into(),
        });
    }
    if (from.caps, from.small_caps) != (to.caps, to.small_caps) {
        code(match (to.caps, to.small_caps) {
            (Some(true), _) => "[CAPS]".into(),
            (_, Some(true)) => "[SMALL CAPS]".into(),
            _ => "[caps]".into(),
        });
    }
}

/// Codes where hyperlinks, tracked insertions and content controls start
/// and end.
fn wrap_codes(from: &[u32], to: &[u32], st: &Store, out: &mut Vec<Tok>) {
    let common = from.iter().zip(to).take_while(|(a, b)| a == b).count();
    let name = |w: u32, open: bool| -> Option<String> {
        let d = st.wraps.get(w as usize)?;
        Some(match (d.kind, open) {
            (WrapKind::Link, true) => format!("[Link:{}]", d.target),
            (WrapKind::Link, false) => "[link]".into(),
            (WrapKind::Insert, true) => format!("[Insert:{}]", d.target),
            (WrapKind::Insert, false) => "[insert]".into(),
            (WrapKind::Control, true) => "[Content Ctl]".into(),
            (WrapKind::Control, false) => "[content ctl]".into(),
            (WrapKind::Field, true) => format!("[Field:{}]", d.target),
            (WrapKind::Field, false) => "[field]".into(),
            (WrapKind::Other, _) => return None,
        })
    };
    for &w in from[common..].iter().rev() {
        if let Some(s) = name(w, false) {
            out.push(Tok::Code(s, None));
        }
    }
    for &w in &to[common..] {
        if let Some(s) = name(w, true) {
            out.push(Tok::Code(s, None));
        }
    }
}

/// Tokens for paragraphs around the cursor.
pub fn tokens(doc: &Document, layout: &Layout, cur: Pos, before: usize, after: usize) -> Vec<Tok> {
    let refs = doc.refs();
    let i = refs.iter().position(|r| *r == cur.pr).unwrap_or(0);
    let lo = i.saturating_sub(before);
    let hi = (i + after + 1).min(refs.len());
    let mut out = Vec::new();
    for &pr in &refs[lo..hi] {
        let p = doc.para(pr);
        if let (Some((r, c)), Some(t)) = (pr.cell, doc.table(pr.block)) {
            if r == 0 && c == 0 {
                out.push(Tok::Code(
                    format!("[Tbl Def:{}x{}]", t.ncols(), t.rows.len()),
                    None,
                ));
            } else if c == 0 {
                out.push(Tok::Code("[Row]".into(), None));
            } else {
                out.push(Tok::Code("[Cell]".into(), None));
            }
        }
        para_codes(&p.props, doc, &mut out);
        // Soft line ends from the layout.
        let mut soft: Vec<usize> = layout
            .lines
            .iter()
            .flat_map(|l| l.segs.iter())
            .filter(|s| s.pr == pr && !s.pad && !s.last)
            .map(|s| s.end)
            .collect();
        soft.sort_unstable();
        let mut fmt = Fmt::default();
        let st = doc.store.borrow();
        let mut look = crate::props::CharLook::default();
        let mut wraps: Vec<u32> = Vec::new();
        for (k, c) in p.cells.iter().enumerate() {
            if soft.binary_search(&k).is_ok() {
                out.push(Tok::Code("[SRt]".into(), None));
                out.push(Tok::Break);
            }
            let pos = Pos::new(pr, k);
            if let Some(o) = st.obj(c.ch) {
                let label = match o.kind {
                    ObjKind::LineBreak => "[Ln Brk]".to_string(),
                    ObjKind::PageBreak => "[HPg]".to_string(),
                    ObjKind::ParaBreak => "[HRt]".to_string(),
                    _ => format!("[{}]", o.label.trim_matches(|c| c == '[' || c == ']')),
                };
                out.push(Tok::Code(label, Some(pos)));
                if o.kind.is_break() {
                    out.push(Tok::Break);
                }
                continue;
            }
            if let Some(id) = fn_id(c.ch) {
                let num = layout.fn_num.get(&id).cloned().unwrap_or_default();
                let text: String = doc
                    .footnotes
                    .get(&id)
                    .map(String::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(24)
                    .collect();
                out.push(Tok::Code(format!("[Footnote:{num};{text}…]"), Some(pos)));
                continue;
            }
            if c.fmt.on_bits() != fmt {
                fmt_switch(fmt, c.fmt.on_bits(), &mut out);
                fmt = c.fmt.on_bits();
            }
            let w = &st.cprops(c.cp).wraps;
            if *w != wraps {
                wrap_codes(&wraps, w, &st, &mut out);
                wraps = w.clone();
            }
            let l = st.look(c.cp);
            if *l != look {
                look_codes(&look, l, doc, &mut out);
                look = l.clone();
            }
            match c.ch {
                '\t' => out.push(Tok::Code("[Tab]".into(), Some(pos))),
                ch => out.push(Tok::Ch(ch, pos)),
            }
        }
        fmt_switch(fmt, Fmt::default(), &mut out);
        if look != crate::props::CharLook::default() {
            look_codes(&look, &crate::props::CharLook::default(), doc, &mut out);
        }
        if !wraps.is_empty() {
            wrap_codes(&wraps, &[], &st, &mut out);
        }
        let end = Pos::new(pr, p.len());
        match pr.cell {
            None => out.push(Tok::Code("[HRt]".into(), Some(end))),
            Some((r, c)) => {
                out.push(Tok::Code(" ".into(), Some(end)));
                let t = doc.table(pr.block).unwrap();
                if r as usize + 1 == t.rows.len() && c as usize + 1 == t.ncols() {
                    out.push(Tok::Code("[Tbl Off]".into(), None));
                } else {
                    continue;
                }
            }
        }
        out.push(Tok::Break);
    }
    out
}
