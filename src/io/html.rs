//! HTML for the clipboard: turn what a web browser (or another program)
//! copies into document blocks, and document blocks into HTML for pasting
//! elsewhere. A forgiving parser: web pages leave tags open, nest them
//! oddly and use entities freely.

use crate::model::*;
use crate::props::CharLook;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Open(String, Vec<(String, String)>),
    Close(String),
    Text(String),
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let v = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(v).map(|c| if c == '\u{a0}' { ' ' } else { c });
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" | "ensp" | "emsp" | "thinsp" => ' ',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "laquo" => '«',
        "raquo" => '»',
        "bull" => '•',
        "middot" => '·',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "deg" => '°',
        "times" => '×',
        "divide" => '÷',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sect" => '§',
        "para" => '¶',
        "shy" => '\u{ad}',
        _ => return None,
    })
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after
            .find(';')
            .filter(|&j| j <= 10)
            .and_then(|j| entity(&after[..j]).map(|c| (j, c)))
        {
            Some((j, c)) => {
                if c != '\u{ad}' {
                    out.push(c);
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn tokenize(html: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let flush = |out: &mut Vec<Tok>, from: usize, to: usize| {
        if to > from {
            out.push(Tok::Text(decode(&html[from..to])));
        }
    };
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        if rest.starts_with("<!--") {
            flush(&mut out, text_start, i);
            i += rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
            text_start = i;
            continue;
        }
        let next = b.get(i + 1).copied().unwrap_or(b' ');
        if !(next.is_ascii_alphabetic() || next == b'/' || next == b'!' || next == b'?') {
            i += 1;
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        flush(&mut out, text_start, i);
        let inner = &rest[1..end];
        i += end + 1;
        text_start = i;
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        if let Some(name) = inner.strip_prefix('/') {
            out.push(Tok::Close(name.trim().to_ascii_lowercase()));
            continue;
        }
        let inner = inner.trim_end_matches('/');
        let name_end = inner
            .find(|c: char| c.is_whitespace())
            .unwrap_or(inner.len());
        let name = inner[..name_end].to_ascii_lowercase();
        let mut attrs = Vec::new();
        let mut a = &inner[name_end..];
        loop {
            a = a.trim_start();
            if a.is_empty() {
                break;
            }
            let k_end = a
                .find(|c: char| c == '=' || c.is_whitespace())
                .unwrap_or(a.len());
            let key = a[..k_end].to_ascii_lowercase();
            a = a[k_end..].trim_start();
            let mut val = String::new();
            if let Some(r) = a.strip_prefix('=') {
                let r = r.trim_start();
                if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                    let body = &r[1..];
                    let e = body.find(q).unwrap_or(body.len());
                    val = decode(&body[..e]);
                    a = body.get(e + 1..).unwrap_or("");
                } else {
                    let e = r.find(char::is_whitespace).unwrap_or(r.len());
                    val = decode(&r[..e]);
                    a = &r[e..];
                }
            }
            if !key.is_empty() {
                attrs.push((key, val));
            }
        }
        out.push(Tok::Open(name, attrs));
    }
    flush(&mut out, text_start, b.len());
    out
}

const BLOCKS: &[&str] = &[
    "p",
    "div",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "blockquote",
    "pre",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
    "figure",
    "figcaption",
    "dt",
    "dd",
    "address",
    "tr",
    "table",
    "ul",
    "ol",
    "dl",
    "hr",
    "title",
    "body",
    "html",
];

struct Conv<'a> {
    doc: &'a mut Document,
    blocks: Vec<Block>,
    para: Vec<Cell>,
    props: ParaProps,
    fmt: Vec<(String, u16)>,
    link: Option<u32>,
    lists: Vec<ListKind>,
    skip: usize,
    pre: usize,
    /// A space seen but not written yet, with its formatting.
    space: Option<(Fmt, u32)>,
    table: Option<(Vec<Vec<Paragraph>>, Vec<Paragraph>, Option<Vec<Cell>>)>,
}

impl Conv<'_> {
    fn cur_fmt(&self) -> Fmt {
        self.fmt
            .iter()
            .fold(Fmt::default(), |f, (_, flag)| f.with(*flag, true))
    }

    fn text(&mut self, t: &str) {
        if self.skip > 0 {
            return;
        }
        let fmt = self.cur_fmt();
        let cp = self.link.unwrap_or(0);
        let cells: &mut Vec<Cell> = match &mut self.table {
            Some((_, _, Some(cell))) => cell,
            _ => &mut self.para,
        };
        if self.pre > 0 {
            for ch in t.chars() {
                if ch == '\n' {
                    // Each line of preformatted text is its own paragraph.
                    let done = std::mem::take(&mut self.para);
                    self.blocks
                        .push(Block::Para(Paragraph::with_cells(done, self.props.clone())));
                    return self.text(&t[t.find('\n').unwrap() + 1..]);
                }
                if ch != '\r' {
                    self.para.push(Cell::with_cp(ch, fmt, cp));
                }
            }
            return;
        }
        for ch in t.chars() {
            if ch.is_whitespace() {
                if self.space.is_none() {
                    self.space = Some((fmt, cp));
                }
                continue;
            }
            if let Some((f, c)) = self.space.take()
                && !cells.is_empty()
            {
                cells.push(Cell::with_cp(' ', f, c));
            }
            cells.push(Cell::with_cp(ch, fmt, cp));
        }
    }

    fn flush(&mut self) {
        self.space = None;
        if self.para.is_empty() {
            return;
        }
        let cells = std::mem::take(&mut self.para);
        let p = Paragraph::with_cells(cells, self.props.clone());
        self.blocks.push(Block::Para(p));
    }

    fn style_id(&self, name: &str) -> String {
        self.doc
            .styles
            .find(name)
            .map(|s| s.id.clone())
            .unwrap_or_default()
    }

    fn open(&mut self, name: &str, attrs: &[(String, String)]) {
        if matches!(
            name,
            "script" | "style" | "head" | "noscript" | "template" | "svg" | "button" | "select"
        ) {
            self.skip += 1;
            return;
        }
        if self.skip > 0 {
            return;
        }
        if BLOCKS.contains(&name) && self.table.as_ref().is_none_or(|t| t.2.is_none()) {
            self.flush();
            self.props = ParaProps::default();
            match name {
                "h1" => self.props = self.doc.style_props(&self.style_id("heading 1")),
                "h2" => self.props = self.doc.style_props(&self.style_id("heading 2")),
                "h3" | "h4" | "h5" | "h6" => {
                    self.props = self.doc.style_props(&self.style_id("heading 3"))
                }
                "blockquote" => self.props = self.doc.style_props(&self.style_id("Quote")),
                "figcaption" => self.props = self.doc.style_props(&self.style_id("caption")),
                "li" => {
                    let kind = self.lists.last().copied().unwrap_or(ListKind::Bullet);
                    let level = self.lists.len().saturating_sub(1).min(8) as u8;
                    self.props.list = Some(ListInfo::new(kind, level));
                }
                "pre" => self.pre += 1,
                "ul" => self.lists.push(ListKind::Bullet),
                "ol" => self.lists.push(ListKind::Number),
                "table" if self.table.is_none() => {
                    self.table = Some((Vec::new(), Vec::new(), None))
                }
                "tr" => {
                    if let Some((rows, row, _)) = &mut self.table
                        && !row.is_empty()
                    {
                        rows.push(std::mem::take(row));
                    }
                }
                _ => {}
            }
        }
        let flag = match name {
            "b" | "strong" => Some(Fmt::BOLD),
            "i" | "em" | "cite" | "var" | "dfn" => Some(Fmt::ITALIC),
            "u" | "ins" => Some(Fmt::UNDERLINE),
            "s" | "strike" | "del" => Some(Fmt::STRIKE),
            "sup" => Some(Fmt::SUPER),
            "sub" => Some(Fmt::SUB),
            "th" => Some(Fmt::BOLD),
            _ => None,
        };
        if let Some(f) = flag {
            self.fmt.push((name.to_string(), f));
        }
        match name {
            "td" | "th" => {
                if let Some((_, _, cell)) = &mut self.table {
                    *cell = Some(Vec::new());
                }
            }
            "br" => {
                if let Some((_, _, Some(cell))) = &mut self.table {
                    cell.push(Cell::new(' ', Fmt::default()));
                } else {
                    self.flush();
                }
            }
            "a" => {
                let href = attrs
                    .iter()
                    .find(|(k, _)| k == "href")
                    .map(|(_, v)| v.trim().to_string())
                    .unwrap_or_default();
                if href.starts_with("http") || href.starts_with("mailto:") {
                    let id = format!("rIdWbLink{}", self.doc.new_rels.len() + 1);
                    self.doc.new_rels.push(NewRel {
                        id: id.clone(),
                        ty: "hyperlink".into(),
                        target: href.clone(),
                        external: true,
                    });
                    self.doc.added_styles.insert("Hyperlink".into());
                    let mut st = self.doc.store.borrow_mut();
                    let w = st.add_wrap(WrapDef {
                        open: format!(r#"<w:hyperlink r:id="{id}" w:history="1">"#),
                        close: "</w:hyperlink>".into(),
                        label: "Link".into(),
                        kind: WrapKind::Link,
                        target: href,
                    });
                    self.link = Some(st.intern(CharProps {
                        rpr: vec![("rStyle".into(), r#"<w:rStyle w:val="Hyperlink"/>"#.into())],
                        wraps: vec![w],
                    }));
                }
            }
            "img" => {
                if let Some((_, alt)) = attrs
                    .iter()
                    .find(|(k, v)| k == "alt" && !v.trim().is_empty())
                {
                    let alt = alt.clone();
                    self.text(&format!("[{alt}]"));
                }
            }
            _ => {}
        }
    }

    fn close(&mut self, name: &str) {
        if matches!(
            name,
            "script" | "style" | "head" | "noscript" | "template" | "svg" | "button" | "select"
        ) {
            self.skip = self.skip.saturating_sub(1);
            return;
        }
        if self.skip > 0 {
            return;
        }
        if let Some(i) = self.fmt.iter().rposition(|(n, _)| n == name) {
            self.fmt.remove(i);
        }
        match name {
            "a" => self.link = None,
            "td" | "th" => {
                if let Some((_, row, cell)) = &mut self.table
                    && let Some(c) = cell.take()
                {
                    let c: Vec<Cell> = trim_cells(c);
                    row.push(Paragraph::with_cells(c, ParaProps::default()));
                }
            }
            "table" => {
                if let Some((mut rows, row, _)) = self.table.take() {
                    if !row.is_empty() {
                        rows.push(row);
                    }
                    let n = rows.iter().map(Vec::len).max().unwrap_or(0);
                    if n > 0 {
                        for r in rows.iter_mut() {
                            r.resize(n, Paragraph::new());
                        }
                        let width = self.doc.page.text_cols();
                        let mut t = Table::new(rows.len(), n, width);
                        t.rows = rows;
                        self.flush();
                        self.blocks.push(Block::Table(t));
                    }
                }
            }
            "ul" | "ol" => {
                self.flush();
                self.lists.pop();
                self.props = ParaProps::default();
            }
            "pre" => {
                self.flush();
                self.pre = self.pre.saturating_sub(1);
                self.props = ParaProps::default();
            }
            n if BLOCKS.contains(&n) && self.table.as_ref().is_none_or(|t| t.2.is_none()) => {
                self.flush();
                self.props = ParaProps::default();
                if let Some(&kind) = self.lists.last() {
                    // Text after a list item but still inside the list.
                    let _ = kind;
                }
            }
            _ => {}
        }
    }
}

fn trim_cells(mut c: Vec<Cell>) -> Vec<Cell> {
    while c.last().is_some_and(|c| c.ch == ' ') {
        c.pop();
    }
    while c.first().is_some_and(|c| c.ch == ' ') {
        c.remove(0);
    }
    c
}

/// Convert HTML (as browsers put it on the clipboard) into blocks, using
/// the document's styles; links become hyperlinks.
pub fn to_blocks(html: &str, doc: &mut Document) -> Vec<Block> {
    // Only the copied fragment, when the clipboard marks it.
    let html = match (
        html.find("<!--StartFragment-->"),
        html.find("<!--EndFragment-->"),
    ) {
        (Some(a), Some(b)) if b > a => &html[a + 20..b],
        _ => html,
    };
    let mut c = Conv {
        doc,
        blocks: Vec::new(),
        para: Vec::new(),
        props: ParaProps::default(),
        fmt: Vec::new(),
        link: None,
        lists: Vec::new(),
        skip: 0,
        pre: 0,
        space: None,
        table: None,
    };
    for t in tokenize(html) {
        match t {
            Tok::Open(n, a) => c.open(&n, &a),
            Tok::Close(n) => c.close(&n),
            Tok::Text(s) => c.text(&s),
        }
    }
    if let Some((mut rows, row, _)) = c.table.take() {
        if !row.is_empty() {
            rows.push(row);
        }
        for r in rows {
            let text: Vec<Cell> = r
                .iter()
                .flat_map(|p| {
                    p.cells
                        .iter()
                        .copied()
                        .chain(std::iter::once(Cell::new('\t', Fmt::default())))
                })
                .collect();
            c.para.extend(text);
            c.flush();
        }
    }
    c.flush();
    c.blocks
        .into_iter()
        .map(|b| match b {
            Block::Para(mut p) => {
                let cells = trim_cells(p.cells.to_vec());
                p.cells = Rc::new(cells);
                Block::Para(p)
            }
            t => t,
        })
        .collect()
}

// ---------------------------------------------------------------- export

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn runs_html(p: &Paragraph, doc: &Document, style_fmt: Fmt) -> String {
    let st = doc.store.borrow();
    let mut out = String::new();
    let mut i = 0;
    let cells = &p.cells;
    while i < cells.len() {
        let c = cells[i];
        if is_special(c.ch) {
            i += 1;
            continue;
        }
        let key = (c.fmt, c.cp);
        let mut text = String::new();
        while i < cells.len() && (cells[i].fmt, cells[i].cp) == key {
            if !is_special(cells[i].ch) {
                text.push(cells[i].ch);
            }
            i += 1;
        }
        let f = Fmt::effective(style_fmt, c.fmt);
        let look: &CharLook = st.look(c.cp);
        let mut s = esc(&text).replace('\t', "&emsp;");
        let mut css = Vec::new();
        if let Some(fnt) = &look.font {
            css.push(format!("font-family:'{}'", doc.font_name(fnt)));
        }
        if let Some(sz) = look.size {
            css.push(format!("font-size:{}pt", sz as f32 / 2.0));
        }
        if let Some(col) = look.rgb() {
            css.push(format!("color:#{col:06X}"));
        }
        if let Some(bg) = look.background() {
            css.push(format!("background-color:#{bg:06X}"));
        }
        if !css.is_empty() {
            s = format!(r#"<span style="{}">{s}</span>"#, css.join(";"));
        }
        for (flag, tag) in [
            (Fmt::SUB, "sub"),
            (Fmt::SUPER, "sup"),
            (Fmt::STRIKE, "s"),
            (Fmt::UNDERLINE, "u"),
            (Fmt::ITALIC, "i"),
            (Fmt::BOLD, "b"),
        ] {
            if f.has(flag) {
                s = format!("<{tag}>{s}</{tag}>");
            }
        }
        let link = st.cprops(c.cp).wraps.iter().find_map(|&w| {
            st.wraps
                .get(w as usize)
                .filter(|d| d.kind == WrapKind::Link)
        });
        if let Some(l) = link {
            s = format!(r#"<a href="{}">{s}</a>"#, esc(&l.target));
        }
        out.push_str(&s);
    }
    out
}

/// HTML for copied blocks (for pasting into browsers, mail and office
/// programs).
pub fn from_blocks(blocks: &[Block], doc: &Document) -> String {
    let mut out = String::from("<html><body>");
    let mut open_list: Option<ListKind> = None;
    let close_list = |out: &mut String, l: &mut Option<ListKind>| {
        if let Some(k) = l.take() {
            out.push_str(if k == ListKind::Bullet {
                "</ul>"
            } else {
                "</ol>"
            });
        }
    };
    for b in blocks {
        match b {
            Block::Para(p) => {
                let sd = doc.styles.get(&p.props.style);
                let sfmt = sd.map(|s| s.fmt).unwrap_or_default();
                let body = runs_html(p, doc, sfmt);
                if let Some(li) = p.props.list {
                    if open_list != Some(li.kind) {
                        close_list(&mut out, &mut open_list);
                        out.push_str(if li.kind == ListKind::Bullet {
                            "<ul>"
                        } else {
                            "<ol>"
                        });
                        open_list = Some(li.kind);
                    }
                    out.push_str(&format!("<li>{body}</li>"));
                    continue;
                }
                close_list(&mut out, &mut open_list);
                let tag = match sd.and_then(|s| s.outline) {
                    Some(0) => "h1",
                    Some(1) => "h2",
                    Some(_) => "h3",
                    None => "p",
                };
                let align = match p.props.align {
                    Align::Center => r#" style="text-align:center""#,
                    Align::Right => r#" style="text-align:right""#,
                    Align::Full => r#" style="text-align:justify""#,
                    Align::Left => "",
                };
                out.push_str(&format!("<{tag}{align}>{body}</{tag}>"));
            }
            Block::Table(t) => {
                close_list(&mut out, &mut open_list);
                out.push_str(r#"<table border="1" cellspacing="0" cellpadding="4">"#);
                for row in &t.rows {
                    out.push_str("<tr>");
                    for p in row {
                        out.push_str(&format!("<td>{}</td>", runs_html(p, doc, Fmt::default())));
                    }
                    out.push_str("</tr>");
                }
                out.push_str("</table>");
            }
        }
    }
    close_list(&mut out, &mut open_list);
    out.push_str("</body></html>");
    out
}

/// Plain text for copied blocks.
pub fn plain(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|b| match b {
            Block::Para(p) => p.plain_text(),
            Block::Table(t) => t
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|p| p.plain_text())
                        .collect::<Vec<_>>()
                        .join("\t")
                })
                .collect::<Vec<_>>()
                .join("\n"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(blocks: &[Block]) -> String {
        plain(blocks)
    }

    #[test]
    fn browser_html_becomes_formatted_text() {
        let mut doc = Document::new();
        let html = r#"<html><body><!--StartFragment--><h2>News &amp; views</h2><p>Some <b>bold</b>, <em>italic</em> and a <a href="https://example.org/x">link</a>.<br>Second&nbsp;line</p><ul><li>One</li><li>Two<ul><li>Nested</li></ul></li></ul><ol><li>First</ol><table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table><script>var x = 1;</script><!--EndFragment--></body></html>"#;
        let blocks = to_blocks(html, &mut doc);
        assert_eq!(
            text_of(&blocks),
            "News & views\nSome bold, italic and a link.\nSecond line\nOne\nTwo\nNested\nFirst\nA\tB\n1\t2"
        );
        let Block::Para(h) = &blocks[0] else { panic!() };
        assert_eq!(
            doc.styles.get(&h.props.style).map(|s| s.name.as_str()),
            Some("heading 2")
        );
        let Block::Para(p) = &blocks[1] else { panic!() };
        assert!(p.cells[5].fmt.has(Fmt::BOLD) && !p.cells[4].fmt.has(Fmt::BOLD));
        let link = p.cells.iter().find(|c| c.ch == 'k').unwrap();
        let st = doc.store.borrow();
        let w = st.cprops(link.cp).wraps[0];
        assert_eq!(st.wraps[w as usize].target, "https://example.org/x");
        drop(st);
        let lists: Vec<Option<ListInfo>> = blocks
            .iter()
            .filter_map(|b| {
                if let Block::Para(p) = b {
                    Some(p.props.list)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            lists[3].map(|l| (l.kind, l.level)),
            Some((ListKind::Bullet, 0))
        );
        assert_eq!(
            lists[5].map(|l| (l.kind, l.level)),
            Some((ListKind::Bullet, 1))
        );
        assert_eq!(lists[6].map(|l| l.kind), Some(ListKind::Number));
        assert!(matches!(blocks.last(), Some(Block::Table(t)) if t.rows.len() == 2));
    }

    #[test]
    fn document_to_html() {
        let mut e = crate::editor::Editor::new();
        e.insert_text("Hello world");
        e.select_range(Pos::new(ParaRef::top(0), 0), Pos::new(ParaRef::top(0), 5));
        e.toggle_fmt(Fmt::BOLD);
        let html = from_blocks(&e.doc.blocks, &e.doc);
        assert!(html.contains("<p><b>Hello</b> world</p>"), "{html}");
    }
}
