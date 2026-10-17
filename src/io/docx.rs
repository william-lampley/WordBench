//! Microsoft Word (.docx, Office Open XML) import and export with lossless
//! round trips.
//!
//! Like WordCraft, the reader turns what WordBench understands into the
//! document model and keeps everything else:
//!
//! * every package part is kept and written back byte for byte unless the
//!   edit touched it (images, settings, themes, comments, macros...);
//! * paragraph and run properties keep all original `w:pPr`/`w:rPr`
//!   children; on save only the properties the user changed are rewritten,
//!   merged in schema order;
//! * wrappers around runs or blocks (hyperlinks, content controls, tracked
//!   insertions, custom XML) are kept as start/end tags around the text;
//! * anything else inside a paragraph (pictures, bookmarks, field codes,
//!   equations, deleted text) becomes an inline object holding its XML;
//! * table, row and cell properties, merged cells, several paragraphs per
//!   cell and nested tables are kept.

use super::xml::{XNode, esc, parse};
use crate::model::*;
use crate::styles::{LevelDef, Numbering, RawStyle, StyleSheet};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::Path;
use std::rc::Rc;

const TWIPS_PER_COL: f32 = 144.0; // 1/10 inch
const TWIPS_PER_IN: f32 = 1440.0;
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT_MAIN: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml";

/// Limits against hostile files.
const MAX_PART: u64 = 256 << 20;
const MAX_TOTAL: u64 = 1 << 30;

/// Schema order of `w:pPr` children (ECMA-376 CT_PPr).
const PPR_ORDER: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// Schema order of `w:rPr` children (CT_RPr).
const RPR_ORDER: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
    "rPrChange",
];

/// Join (name, xml) pairs in schema order; unknown names go last in their
/// original order.
fn ordered(order: &[&str], mut items: Vec<(String, String)>) -> String {
    items.sort_by_key(|(n, _)| order.iter().position(|o| o == n).unwrap_or(order.len()));
    items.into_iter().map(|(_, x)| x).collect()
}

fn on(n: &XNode) -> bool {
    !matches!(n.attr("val"), Some("0" | "false" | "off" | "none"))
}

fn num_attr(n: &XNode, key: &str) -> Option<f32> {
    n.attr(key).and_then(|v| v.trim().parse::<f32>().ok())
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

fn parse_one(xml: &str) -> Option<XNode> {
    parse(xml)
        .ok()?
        .children
        .into_iter()
        .find(|c| c.name != "#text")
}

fn truncate(s: &str, n: usize) -> String {
    let t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{t}…")
    } else {
        t
    }
}

/// Resolve a relationship target against the folder of the source part.
fn resolve(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn dir_of(part: &str) -> &str {
    part.rfind('/').map(|i| &part[..i]).unwrap_or("")
}

fn rels_path(part: &str) -> String {
    let (dir, name) = match part.rfind('/') {
        Some(i) => (&part[..i], &part[i + 1..]),
        None => ("", part),
    };
    if dir.is_empty() {
        format!("_rels/{name}.rels")
    } else {
        format!("{dir}/_rels/{name}.rels")
    }
}

#[derive(Clone, Debug)]
struct Rel {
    id: String,
    ty: String,
    target: String,
    mode: Option<String>,
}

fn read_rels(bytes: Option<&[u8]>) -> Vec<Rel> {
    let Some(b) = bytes else { return Vec::new() };
    let Ok(x) = parse(&String::from_utf8_lossy(b)) else {
        return Vec::new();
    };
    let Some(root) = x.child("Relationships") else {
        return Vec::new();
    };
    root.children_named("Relationship")
        .filter_map(|n| {
            Some(Rel {
                id: n.attr("Id")?.to_string(),
                ty: n.attr("Type")?.to_string(),
                target: n.attr("Target")?.to_string(),
                mode: n.attr("TargetMode").map(str::to_string),
            })
        })
        .collect()
}

fn rels_xml(rels: &[Rel]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for r in rels {
        let mode = r
            .mode
            .as_ref()
            .map(|m| format!(r#" TargetMode="{}""#, esc(m)))
            .unwrap_or_default();
        s.push_str(&format!(
            r#"<Relationship Id="{}" Type="{}" Target="{}"{mode}/>"#,
            esc(&r.id),
            esc(&r.ty),
            esc(&r.target)
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// Package path of the part a relationship of the main document points
/// to (pictures referenced by `r:embed`), looking at relationships added
/// while editing first. None for external targets.
pub fn rel_part(doc: &Document, id: &str) -> Option<String> {
    let main = doc
        .docx
        .as_ref()
        .map(|p| p.main.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let dir = dir_of(&main);
    if let Some(r) = doc.new_rels.iter().find(|r| r.id == id) {
        return (!r.external).then(|| resolve(dir, &r.target));
    }
    let pkg = doc.docx.as_ref()?;
    let rels_name = rels_path(&main);
    let rels = read_rels(
        pkg.parts
            .iter()
            .find(|(n, _)| *n == rels_name)
            .map(|(_, b)| b.as_slice()),
    );
    let r = rels.into_iter().find(|r| r.id == id)?;
    (r.mode.as_deref() != Some("External")).then(|| resolve(dir, &r.target))
}

// ===================================================================== import

struct Rd<'a> {
    doc: &'a mut Document,
    notes: HashMap<String, (String, String)>,
    endnotes: HashMap<String, (String, String)>,
    rels: Vec<Rel>,
}

fn child_items(n: Option<&XNode>) -> Vec<(String, String)> {
    n.map(|n| {
        n.children
            .iter()
            .filter(|c| c.name != "#text")
            .map(|c| (c.name.clone(), c.to_xml()))
            .collect()
    })
    .unwrap_or_default()
}

/// Styles part -> raw styles and document defaults (pPr, rPr items).
fn read_styles(x: &XNode) -> (Vec<RawStyle>, Vec<(String, String)>, Vec<(String, String)>) {
    let Some(root) = x.child("styles") else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let defaults = root.child("docDefaults");
    let doc_ppr = child_items(
        defaults
            .and_then(|d| d.child("pPrDefault"))
            .and_then(|d| d.child("pPr")),
    );
    let doc_rpr = child_items(
        defaults
            .and_then(|d| d.child("rPrDefault"))
            .and_then(|d| d.child("rPr")),
    );
    let raw = root
        .children_named("style")
        .filter_map(|s| {
            let ty = s.attr("type").unwrap_or("paragraph");
            if ty != "paragraph" && ty != "character" {
                return None;
            }
            let id = s.attr("styleId")?.to_string();
            let val = |n: &str| s.child(n).and_then(|c| c.attr("val")).map(str::to_string);
            Some(RawStyle {
                name: val("name").unwrap_or_else(|| id.clone()),
                id,
                para: ty == "paragraph",
                based_on: val("basedOn"),
                next: val("next"),
                hidden: s.child("semiHidden").is_some() || s.child("hidden").is_some(),
                default: matches!(s.attr("default"), Some("1" | "true")),
                ppr: child_items(s.child("pPr")),
                rpr: child_items(s.child("rPr")),
                xml: s.to_xml(),
            })
        })
        .collect();
    (raw, doc_ppr, doc_rpr)
}

fn read_numbering(x: &XNode) -> Numbering {
    let mut n = Numbering::default();
    let Some(root) = x.child("numbering") else {
        return n;
    };
    let val = |e: &XNode, name: &str| {
        e.child(name)
            .and_then(|c| c.attr("val"))
            .map(str::to_string)
    };
    for a in root.children_named("abstractNum") {
        let Some(id) = a.attr("abstractNumId").and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let mut levels = vec![
            LevelDef {
                fmt: "decimal".into(),
                text: String::new(),
                start: 1,
                left: None,
                hanging: None
            };
            9
        ];
        for l in a.children_named("lvl") {
            let Some(i) = l
                .attr("ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&i| i < 9)
            else {
                continue;
            };
            let ind = l.child("pPr").and_then(|p| p.child("ind"));
            let tw = |k: &str| {
                ind.and_then(|n| num_attr(n, k))
                    .map(|v| (v / TWIPS_PER_COL).round().clamp(0.0, 60.0) as u16)
            };
            levels[i] = LevelDef {
                fmt: val(l, "numFmt").unwrap_or_else(|| "decimal".into()),
                text: val(l, "lvlText").unwrap_or_default(),
                start: val(l, "start").and_then(|v| v.parse().ok()).unwrap_or(1),
                left: tw("left").or_else(|| tw("start")),
                hanging: tw("hanging"),
            };
        }
        n.abstracts.insert(id, levels);
    }
    for num in root.children_named("num") {
        let (Some(id), Some(abs)) = (
            num.attr("numId").and_then(|v| v.parse::<u32>().ok()),
            val(num, "abstractNumId").and_then(|v| v.parse::<u32>().ok()),
        ) else {
            continue;
        };
        let mut overrides = [None; 9];
        for o in num.children_named("lvlOverride") {
            if let Some(i) = o
                .attr("ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|&i| i < 9)
            {
                overrides[i] = val(o, "startOverride").and_then(|v| v.parse().ok());
            }
        }
        n.nums.insert(id, (abs, overrides));
    }
    n
}

/// Major and minor theme fonts.
fn read_theme_fonts(x: &XNode) -> Option<(String, String)> {
    let scheme = x.find("fontScheme")?;
    let face = |n: &str| {
        scheme
            .child(n)?
            .child("latin")?
            .attr("typeface")
            .map(str::to_string)
    };
    Some((face("majorFont")?, face("minorFont")?))
}

/// Notes by id: (plain text, original XML); separators separately.
fn read_notes(x: &XNode, tag: &str) -> (HashMap<String, (String, String)>, Vec<String>) {
    let mut out = HashMap::new();
    let mut seps = Vec::new();
    let Some(root) = x.children.iter().find(|c| c.name == format!("{tag}s")) else {
        return (out, seps);
    };
    for n in root.children_named(tag) {
        if n.attr("type").is_some_and(|t| t != "normal") {
            seps.push(n.to_xml());
            continue;
        }
        let paras: Vec<String> = n
            .children_named("p")
            .map(|p| note_text(p).trim().to_string())
            .collect();
        out.insert(
            n.attr("id").unwrap_or("").to_string(),
            (paras.join(" ").trim().to_string(), n.to_xml()),
        );
    }
    (out, seps)
}

/// Text of a note paragraph: `w:t` and tabs only.
fn note_text(n: &XNode) -> String {
    let mut s = String::new();
    fn walk(n: &XNode, s: &mut String) {
        match n.name.as_str() {
            "t" => s.push_str(&clean(&n.all_text())),
            "tab" => s.push(' '),
            "delText" | "instrText" => {}
            _ => n.children.iter().for_each(|c| walk(c, s)),
        }
    }
    walk(n, &mut s);
    s
}

/// What the editor shows for a picture or other drawing.
fn drawing_label(n: &XNode) -> String {
    if n.find("chart").is_some() {
        return "[Chart]".into();
    }
    if let Some(t) = n.find("txbxContent") {
        let text = note_text(t);
        return format!("[Text box: {}]", truncate(text.trim(), 24));
    }
    let name = n
        .find("docPr")
        .or_else(|| n.find("cNvPr"))
        .and_then(|d| d.attr("descr").filter(|s| !s.is_empty()).or(d.attr("name")))
        .or_else(|| n.find("shape").and_then(|s| s.attr("alt")))
        .unwrap_or("");
    if name.is_empty() {
        "[Image]".into()
    } else {
        format!("[Image: {}]", truncate(name, 30))
    }
}

impl Rd<'_> {
    fn obj(&mut self, kind: ObjKind, xml: String, label: String, in_run: bool) -> char {
        self.doc.new_obj(InlineObj {
            kind,
            xml,
            label,
            in_run,
        })
    }

    fn intern(&mut self, cp: CharProps) -> u32 {
        self.doc.store.borrow_mut().intern(cp)
    }

    /// A wrapper element: `content` is where its text/blocks live, `skip`
    /// names properties children that go in the start tag.
    fn wrap(&mut self, n: &XNode, label: &str) -> u32 {
        let mut open = n.start_tag();
        let mut close = n.end_tag();
        for c in &n.children {
            if matches!(
                c.name.as_str(),
                "sdtPr" | "sdtEndPr" | "customXmlPr" | "smartTagPr"
            ) {
                open.push_str(&c.to_xml());
            }
        }
        if n.name == "sdt"
            && let Some(content) = n.child("sdtContent")
        {
            open.push_str(&content.start_tag());
            close = format!("{}{close}", content.end_tag());
        }
        let (kind, target) = match n.name.as_str() {
            "hyperlink" => {
                let url = n
                    .attrs
                    .iter()
                    .find(|(k, _)| k.ends_with(":id") && !k.starts_with("w:"))
                    .and_then(|(_, id)| self.rels.iter().find(|r| r.id == *id))
                    .map(|r| r.target.clone());
                let anchor = n.attr("anchor").map(|a| format!("#{a}"));
                (WrapKind::Link, url.or(anchor).unwrap_or_default())
            }
            "ins" | "moveTo" => (WrapKind::Insert, n.attr("author").unwrap_or("").to_string()),
            "sdt" => (WrapKind::Control, String::new()),
            "fldSimple" => (
                WrapKind::Field,
                n.attr("instr").unwrap_or("").trim().to_string(),
            ),
            _ => (WrapKind::Other, String::new()),
        };
        self.doc.store.borrow_mut().add_wrap(WrapDef {
            open,
            close,
            label: label.into(),
            kind,
            target,
        })
    }

    /// Effective paragraph properties: style, then direct formatting.
    fn read_ppr(&self, ppr: Option<&XNode>) -> (ParaProps, Vec<(String, String)>) {
        let raw = child_items(ppr);
        let style_id = raw
            .iter()
            .find(|(n, _)| n == "pStyle")
            .and_then(|(_, x)| parse_one(x))
            .and_then(|n| n.attr("val").map(str::to_string))
            .unwrap_or_default();
        let sheet = &self.doc.styles;
        let mut p = sheet
            .get(&style_id)
            .map(|s| s.props.clone())
            .unwrap_or_default();
        p.style = if sheet.get(&style_id).is_some() {
            style_id
        } else {
            String::new()
        };
        if sheet.is_default(&p.style) {
            p.style.clear();
        }
        crate::props::apply_ppr(&mut p, &raw, &self.doc.numbering);
        // List items without their own indent use the list level's.
        if p.list.is_some_and(|l| l.num != 0) && !raw.iter().any(|(n, _)| n == "ind") {
            p.indent = 0;
            p.first_indent = 0;
        }
        (p, raw)
    }

    fn read_rpr(rpr: Option<&XNode>) -> (Fmt, Vec<(String, String)>) {
        let raw = child_items(rpr);
        (crate::props::fmt_from_rpr(&raw), raw)
    }

    fn read_para(&mut self, p: &XNode, bwraps: &[u32], pre: String) -> Paragraph {
        let (props, ppr) = self.read_ppr(p.child("pPr"));
        let mut cells = Vec::new();
        self.read_inline(p, &[], &mut cells);
        let extra = ParaExtra {
            p_attrs: p.attrs_xml(),
            ppr,
            at_read: Some(props.clone()),
            bwraps: bwraps.to_vec(),
            pre,
            ..Default::default()
        };
        Paragraph {
            cells: Rc::new(cells),
            props,
            extra: Some(Rc::new(extra)),
        }
    }

    fn read_inline(&mut self, node: &XNode, wraps: &[u32], cells: &mut Vec<Cell>) {
        let plain = self.intern(CharProps {
            rpr: Vec::new(),
            wraps: wraps.to_vec(),
        });
        for c in &node.children {
            let name = c.name.as_str();
            match name {
                "#text" | "pPr" | "rPr" | "sdtPr" | "sdtEndPr" | "customXmlPr" | "smartTagPr" => {}
                // Proofing marks are regenerated by Word.
                "proofErr" | "lastRenderedPageBreak" => {}
                "r" => self.read_run(c, wraps, cells),
                "hyperlink" | "ins" | "smartTag" | "fldSimple" | "customXml" | "dir" | "bdo"
                | "moveTo" => {
                    let label = match name {
                        "hyperlink" => "Link",
                        "ins" | "moveTo" => "Insert",
                        "fldSimple" => "Field",
                        _ => name,
                    };
                    let w = self.wrap(c, label);
                    let mut inner = wraps.to_vec();
                    inner.push(w);
                    self.read_inline(c, &inner, cells);
                }
                "sdt" => {
                    let w = self.wrap(c, "Content control");
                    let mut inner = wraps.to_vec();
                    inner.push(w);
                    if let Some(content) = c.child("sdtContent") {
                        self.read_inline(content, &inner, cells);
                    }
                }
                "del" | "moveFrom" => {
                    let text = c.find_all_text_named("delText");
                    let what = if name == "del" { "Deleted" } else { "Moved" };
                    let ch = self.obj(
                        ObjKind::Visible,
                        c.to_xml(),
                        format!("[{what}: {}]", truncate(&text, 24)),
                        false,
                    );
                    cells.push(Cell::with_cp(ch, Fmt(Fmt::STRIKE), plain));
                }
                "oMath" | "oMathPara" => {
                    let ch = self.obj(ObjKind::Visible, c.to_xml(), "[Equation]".into(), false);
                    cells.push(Cell::with_cp(ch, Fmt::default(), plain));
                }
                _ => {
                    let label = match name {
                        "bookmarkStart" => format!("Bookmark:{}", c.attr("name").unwrap_or("")),
                        "bookmarkEnd" => "Bookmark end".into(),
                        "commentRangeStart" => "Comment".into(),
                        "commentRangeEnd" => "Comment end".into(),
                        other => other.to_string(),
                    };
                    let ch = self.obj(ObjKind::Hidden, c.to_xml(), label, false);
                    cells.push(Cell::with_cp(ch, Fmt::default(), plain));
                }
            }
        }
    }

    fn read_run(&mut self, r: &XNode, wraps: &[u32], cells: &mut Vec<Cell>) {
        let (fmt, rpr) = Self::read_rpr(r.child("rPr"));
        let cp = self.intern(CharProps {
            rpr,
            wraps: wraps.to_vec(),
        });
        for c in &r.children {
            let name = c.name.as_str();
            let mut push_obj = |rd: &mut Self, kind: ObjKind, label: String| {
                let ch = rd.obj(kind, c.to_xml(), label, true);
                cells.push(Cell::with_cp(ch, fmt, cp));
            };
            match name {
                "#text" | "rPr" | "lastRenderedPageBreak" => {}
                "t" => cells.extend(
                    clean(&c.all_text())
                        .chars()
                        .map(|ch| Cell::with_cp(ch, fmt, cp)),
                ),
                "tab" => cells.push(Cell::with_cp('\t', fmt, cp)),
                "noBreakHyphen" => cells.push(Cell::with_cp('\u{2011}', fmt, cp)),
                "br" | "cr" => match c.attr("type") {
                    Some("page") => push_obj(self, ObjKind::PageBreak, "HPg".into()),
                    Some("column") => push_obj(self, ObjKind::LineBreak, "Col Brk".into()),
                    _ => push_obj(self, ObjKind::LineBreak, "Ln Brk".into()),
                },
                "footnoteReference" | "endnoteReference" => {
                    let id = c.attr("id").unwrap_or("").to_string();
                    let endnote = name == "endnoteReference";
                    let (text, xml) = if endnote { &self.endnotes } else { &self.notes }
                        .get(&id)
                        .cloned()
                        .unwrap_or_default();
                    let nid = self.doc.new_footnote(text.clone());
                    self.doc.note_meta.insert(
                        nid,
                        NoteMeta {
                            endnote,
                            orig_text: text,
                            xml,
                        },
                    );
                    cells.push(Cell::with_cp(fn_char(nid), fmt, cp));
                }
                "drawing" | "pict" | "object" => push_obj(self, ObjKind::Visible, drawing_label(c)),
                "AlternateContent" => push_obj(self, ObjKind::Visible, drawing_label(c)),
                "sym" => push_obj(self, ObjKind::Visible, "[Symbol]".into()),
                "ptab" => push_obj(self, ObjKind::Visible, "→".into()),
                "commentReference" => {
                    let id = c.attr("id").unwrap_or("");
                    let who = self
                        .doc
                        .comments
                        .get(id)
                        .map(|c| c.author.clone())
                        .unwrap_or_default();
                    let label = if who.is_empty() {
                        "[Comment]".to_string()
                    } else {
                        format!("[Comment: {}]", truncate(&who, 16))
                    };
                    push_obj(self, ObjKind::Visible, label)
                }
                "fldChar" => {
                    let label = match c.attr("fldCharType") {
                        Some("begin") => "Field",
                        Some("separate") => "Field result",
                        _ => "Field end",
                    };
                    push_obj(self, ObjKind::Hidden, label.into());
                }
                "instrText" => push_obj(
                    self,
                    ObjKind::Hidden,
                    format!("Field:{}", c.all_text().trim()),
                ),
                other => push_obj(self, ObjKind::Hidden, other.to_string()),
            }
        }
    }

    fn read_cell(&mut self, tc: &XNode) -> Paragraph {
        let mut base: Option<Paragraph> = None;
        for b in &tc.children {
            match b.name.as_str() {
                "#text" | "tcPr" => {}
                "p" => {
                    let p = self.read_para(b, &[], String::new());
                    match &mut base {
                        None => base = Some(p),
                        Some(bp) => {
                            let ppr = b.child("pPr").map(XNode::to_xml).unwrap_or_default();
                            let ch = self.obj(ObjKind::ParaBreak, ppr, String::new(), false);
                            let cells = bp.cells_mut();
                            cells.push(Cell::new(ch, Fmt::default()));
                            cells.extend(p.cells.iter().copied());
                        }
                    }
                }
                other => {
                    let label = if other == "tbl" {
                        "[Table]".to_string()
                    } else {
                        format!("[{other}]")
                    };
                    let ch = self.obj(ObjKind::Block, b.to_xml(), label, false);
                    base.get_or_insert_with(Paragraph::new)
                        .cells_mut()
                        .push(Cell::new(ch, Fmt::default()));
                }
            }
        }
        base.unwrap_or_default()
    }

    fn read_table(&mut self, t: &XNode, bwraps: &[u32], pre: String) -> Table {
        let grid_node = t.child("tblGrid");
        let mut cols: Vec<u16> = grid_node
            .map(|g| {
                g.children_named("gridCol")
                    .map(|c| {
                        ((num_attr(c, "w").unwrap_or(1440.0) / TWIPS_PER_COL).round() as i32 - 3)
                            .clamp(3, 120) as u16
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut rows: Vec<Vec<Paragraph>> = Vec::new();
        let covered = || Paragraph {
            extra: Some(Rc::new(ParaExtra {
                covered: true,
                ..Default::default()
            })),
            ..Default::default()
        };
        let mut tr_nodes: Vec<&XNode> = Vec::new();
        for c in &t.children {
            match c.name.as_str() {
                "tr" => tr_nodes.push(c),
                // Rows inside content controls: keep the rows (the wrapper is dropped).
                "sdt" | "customXml" => {
                    let inner = c.child("sdtContent").unwrap_or(c);
                    tr_nodes.extend(inner.children_named("tr"));
                }
                _ => {}
            }
        }
        for tr in tr_nodes {
            let trpr = tr.child("trPr");
            let grid_before = trpr
                .and_then(|p| p.child("gridBefore"))
                .and_then(|g| g.attr("val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
                .min(50);
            let mut row: Vec<Paragraph> = (0..grid_before).map(|_| covered()).collect();
            let mut tcs: Vec<&XNode> = Vec::new();
            for c in &tr.children {
                match c.name.as_str() {
                    "tc" => tcs.push(c),
                    "sdt" | "customXml" => {
                        let inner = c.child("sdtContent").unwrap_or(c);
                        tcs.extend(inner.children_named("tc"));
                    }
                    _ => {}
                }
            }
            for tc in tcs {
                let tcpr = tc.child("tcPr");
                let span = tcpr
                    .and_then(|p| p.child("gridSpan"))
                    .and_then(|g| g.attr("val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 50);
                let mut cell = self.read_cell(tc);
                let mut x = cell.extra.as_deref().cloned().unwrap_or_default();
                x.tc_pr = tcpr.map(XNode::to_xml);
                x.at_read.get_or_insert_with(|| cell.props.clone());
                cell.extra = Some(Rc::new(x));
                row.push(cell);
                for _ in 1..span {
                    row.push(covered());
                }
            }
            if row.is_empty() {
                row.push(Paragraph::new());
            }
            let inner: String = tr
                .children
                .iter()
                .filter(|c| matches!(c.name.as_str(), "tblPrEx" | "trPr"))
                .map(XNode::to_xml)
                .collect();
            let mut x0 = row[0].extra.as_deref().cloned().unwrap_or_default();
            x0.tr = Some((tr.attrs_xml(), inner));
            row[0].extra = Some(Rc::new(x0));
            rows.push(row);
        }
        let n = rows
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .max(cols.len())
            .max(1);
        for r in rows.iter_mut() {
            while r.len() < n {
                r.push(covered());
            }
        }
        let grid_ok = cols.len() == n;
        if !grid_ok {
            cols = Table::new(1, n, 65).cols;
        }
        if rows.is_empty() {
            rows.push(vec![Paragraph::new(); n]);
        }
        let extra = TableExtra {
            tbl_pr: t.child("tblPr").map(XNode::to_xml),
            grid: grid_node
                .filter(|_| grid_ok)
                .map(|g| (cols.clone(), g.to_xml())),
            bwraps: bwraps.to_vec(),
            pre,
        };
        Table {
            cols,
            rows,
            extra: Some(Rc::new(extra)),
        }
    }

    fn read_body(
        &mut self,
        body: &XNode,
        bwraps: &[u32],
        out: &mut Vec<Block>,
        pre: &mut String,
        sect: &mut Option<String>,
    ) {
        for c in &body.children {
            match c.name.as_str() {
                "#text" | "sdtPr" | "sdtEndPr" | "customXmlPr" => {}
                "p" => {
                    let p = self.read_para(c, bwraps, std::mem::take(pre));
                    out.push(Block::Para(p));
                }
                "tbl" => {
                    let t = self.read_table(c, bwraps, std::mem::take(pre));
                    out.push(Block::Table(t));
                }
                "sdt" | "customXml" => {
                    let content = if c.name == "sdt" {
                        c.child("sdtContent")
                    } else {
                        Some(c)
                    };
                    let before = out.len();
                    let w = self.wrap(
                        c,
                        if c.name == "sdt" {
                            "Content control"
                        } else {
                            "Custom XML"
                        },
                    );
                    let mut inner = bwraps.to_vec();
                    inner.push(w);
                    let mut inner_pre = std::mem::take(pre);
                    if let Some(content) = content {
                        self.read_body(content, &inner, out, &mut inner_pre, sect);
                    }
                    if out.len() == before {
                        // Nothing editable inside: keep the element as is.
                        *pre = inner_pre;
                        pre.push_str(&c.to_xml());
                    } else {
                        pre.push_str(&inner_pre);
                    }
                }
                "sectPr" => *sect = Some(c.to_xml()),
                _ => pre.push_str(&c.to_xml()),
            }
        }
    }
}

trait FindText {
    fn find_all_text_named(&self, name: &str) -> String;
}

impl FindText for XNode {
    fn find_all_text_named(&self, name: &str) -> String {
        let mut s = String::new();
        fn walk(n: &XNode, name: &str, s: &mut String) {
            if n.name == name {
                s.push_str(&n.all_text());
            } else {
                n.children.iter().for_each(|c| walk(c, name, s));
            }
        }
        walk(self, name, &mut s);
        clean(&s)
    }
}

/// Header/footer lines (one per paragraph, with its alignment), with
/// PAGE / NUMPAGES / DATE fields turned into `{page}` / `{pages}` / `{date}`.
fn hf_lines(x: &XNode) -> Vec<(String, Align)> {
    struct St {
        out: String,
        skip: bool,
        pending: Option<&'static str>,
    }
    fn field_of(instr: &str) -> Option<&'static str> {
        let i = instr.to_uppercase();
        let word = i.split_whitespace().next().unwrap_or("");
        match word {
            "NUMPAGES" | "SECTIONPAGES" => Some("{pages}"),
            "PAGE" => Some("{page}"),
            "DATE" | "CREATEDATE" | "SAVEDATE" | "PRINTDATE" => Some("{date}"),
            _ => None,
        }
    }
    fn text(n: &XNode, st: &mut St) {
        match n.name.as_str() {
            "fldSimple" => {
                if let Some(f) = field_of(n.attr("instr").unwrap_or("")) {
                    st.out.push_str(f);
                    return;
                }
            }
            "instrText" => {
                if let Some(f) = field_of(&n.all_text()) {
                    st.pending = Some(f);
                }
                return;
            }
            "fldChar" => {
                match n.attr("fldCharType") {
                    Some("separate") => {
                        if let Some(p) = st.pending.take() {
                            st.out.push_str(p);
                            st.skip = true;
                        }
                    }
                    Some("end") => {
                        if let Some(p) = st.pending.take() {
                            st.out.push_str(p);
                        }
                        st.skip = false;
                    }
                    _ => {}
                }
                return;
            }
            "t" => {
                if !st.skip {
                    st.out.push_str(&n.all_text());
                }
                return;
            }
            "tab" | "ptab" => {
                if !st.skip {
                    st.out.push(' ');
                }
            }
            "pPr" | "rPr" => return,
            _ => {}
        }
        for c in &n.children {
            text(c, st);
        }
    }
    fn paras(n: &XNode, out: &mut Vec<(String, Align)>) {
        if n.name == "p" {
            let jc = n
                .child("pPr")
                .and_then(|p| p.child("jc"))
                .and_then(|j| j.attr("val"));
            let align = match jc {
                Some("center") => Align::Center,
                Some("right" | "end") => Align::Right,
                _ => Align::Left,
            };
            let mut st = St {
                out: String::new(),
                skip: false,
                pending: None,
            };
            text(n, &mut st);
            out.push((clean(st.out.trim()), align));
            return;
        }
        for c in &n.children {
            paras(c, out);
        }
    }
    let mut out = Vec::new();
    paras(x, &mut out);
    while out.last().is_some_and(|(t, _)| t.is_empty()) {
        out.pop();
    }
    out
}

fn page_from_sect(s: &XNode, page: &mut PageSetup) {
    if let Some(sz) = s.child("pgSz")
        && let (Some(w), Some(h)) = (num_attr(sz, "w"), num_attr(sz, "h"))
    {
        page.width = (w / TWIPS_PER_IN).clamp(3.0, 30.0);
        page.height = (h / TWIPS_PER_IN).clamp(3.0, 30.0);
    }
    if let Some(m) = s.child("pgMar") {
        let g = |k: &str, d: f32| {
            (num_attr(m, k).unwrap_or(d * TWIPS_PER_IN).abs() / TWIPS_PER_IN).clamp(0.0, 4.0)
        };
        page.margin_top = g("top", 1.0);
        page.margin_bottom = g("bottom", 1.0);
        page.margin_left = g("left", 1.0);
        page.margin_right = g("right", 1.0);
    }
}

/// Read all parts of a zip package, in order.
fn read_parts(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("not a .docx file: {e}"))?;
    let mut out = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(|e| e.to_string())?.to_string();
        let mut buf = Vec::new();
        f.take(MAX_PART + 1)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        if buf.len() as u64 > MAX_PART {
            return Err(format!("{name} is too large"));
        }
        total += buf.len() as u64;
        if total > MAX_TOTAL {
            return Err("document is too large".into());
        }
        out.push((name, buf));
    }
    Ok(out)
}

pub fn load(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    from_bytes(&bytes)
}

pub fn from_bytes(bytes: &[u8]) -> Result<Document, String> {
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(
            "this file is encrypted or in the old .doc format; WordBench can't open it".into(),
        );
    }
    let parts = read_parts(bytes)?;
    let get = |name: &str| {
        parts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.as_slice())
    };
    let tree = |name: &str| {
        get(name)
            .and_then(|b| parse(&String::from_utf8_lossy(b)).ok())
            .unwrap_or_default()
    };

    let main = read_rels(get("_rels/.rels"))
        .into_iter()
        .find(|r| r.ty.ends_with("/officeDocument"))
        .map(|r| resolve("", &r.target))
        .unwrap_or_else(|| "word/document.xml".into());
    let doc_xml = get(&main).ok_or("not a .docx file (no main document part)")?;
    let rels = read_rels(get(&rels_path(&main)));
    let dir = dir_of(&main).to_string();
    let target_of = |ty: &str| {
        rels.iter()
            .find(|r| r.ty.ends_with(ty))
            .map(|r| resolve(&dir, &r.target))
    };

    let (raw_styles, doc_ppr, doc_rpr) =
        read_styles(&target_of("/styles").map(|p| tree(&p)).unwrap_or_default());
    let numbering = read_numbering(
        &target_of("/numbering")
            .map(|p| tree(&p))
            .unwrap_or_default(),
    );
    let theme = target_of("/theme").and_then(|p| read_theme_fonts(&tree(&p)));
    let (notes, note_seps) = read_notes(
        &target_of("/footnotes")
            .map(|p| tree(&p))
            .unwrap_or_default(),
        "footnote",
    );
    let (endnotes, endnote_seps) = read_notes(
        &target_of("/endnotes").map(|p| tree(&p)).unwrap_or_default(),
        "endnote",
    );

    let x = parse(&String::from_utf8_lossy(doc_xml))?;
    let root = x
        .children
        .iter()
        .find(|c| c.name == "document")
        .ok_or("document has no body")?;
    let body = root.child("body").ok_or("document has no body")?;
    let doc_pre: String = root
        .children
        .iter()
        .filter(|c| c.name != "body" && c.name != "#text")
        .map(XNode::to_xml)
        .collect();

    let mut doc = Document::new();
    if let Some(cx) = target_of("/comments").map(|p| tree(&p))
        && let Some(root) = cx.child("comments")
    {
        for c in root.children_named("comment") {
            let Some(id) = c.attr("id") else { continue };
            let text = c
                .children_named("p")
                .map(|p| note_text(p).trim().to_string())
                .collect::<Vec<_>>()
                .join(" ");
            doc.comments.insert(
                id.to_string(),
                Comment {
                    author: c.attr("author").unwrap_or("").to_string(),
                    initials: c.attr("initials").unwrap_or("").to_string(),
                    date: c.attr("date").unwrap_or("").to_string(),
                    orig_text: text.clone(),
                    text,
                    xml: c.to_xml(),
                },
            );
        }
    }
    let core_path = read_rels(get("_rels/.rels"))
        .into_iter()
        .find(|r| r.ty.ends_with("/core-properties"))
        .map(|r| resolve("", &r.target));
    if let Some(cx) = core_path.map(|p| tree(&p))
        && let Some(root) = cx.child("coreProperties")
    {
        let t = |n: &str| {
            root.child(n)
                .map(|c| clean(&c.all_text()))
                .unwrap_or_default()
        };
        doc.core = CoreProps {
            title: t("title"),
            subject: t("subject"),
            author: t("creator"),
            keywords: t("keywords"),
            description: t("description"),
        };
    }
    if !raw_styles.is_empty() {
        let mut store = doc.store.borrow_mut();
        let mut sheet =
            StyleSheet::build(raw_styles, &doc_ppr, &doc_rpr, &numbering, &mut store, true);
        sheet.add_missing_builtins(&mut store);
        drop(store);
        doc.styles = Rc::new(sheet);
    }
    doc.numbering = Rc::new(numbering);
    if let Some(t) = theme {
        doc.theme_fonts = t;
    }
    let mut blocks = Vec::new();
    let mut sect = None;
    let mut tail = String::new();
    {
        let mut rd = Rd {
            doc: &mut doc,
            notes,
            endnotes,
            rels: rels.clone(),
        };
        rd.read_body(body, &[], &mut blocks, &mut tail, &mut sect);
    }
    doc.blocks = blocks;

    let mut pkg = DocxPkg {
        main: main.clone(),
        root_open: root.start_tag(),
        doc_pre,
        body_tail: tail,
        sect: sect.clone(),
        note_seps,
        endnote_seps,
        ..Default::default()
    };
    if let Some(s) = sect.as_deref().and_then(parse_one) {
        page_from_sect(&s, &mut doc.page);
        let hf = |tag: &str| -> Option<(Vec<(String, Align)>, String)> {
            let refs: Vec<&XNode> = s.children_named(tag).collect();
            let r = refs
                .iter()
                .find(|r| r.attr("type") == Some("default"))
                .or(refs.first())?;
            let id = r.attr("id")?;
            let target = rels.iter().find(|x| x.id == id)?;
            let name = resolve(&dir, &target.target);
            Some((hf_lines(&tree(&name)), name))
        };
        // A header that is only a page number becomes the page number setting.
        let only_page =
            |l: &Vec<(String, Align)>| (l.len() == 1 && l[0].0 == "{page}").then(|| l[0].1);
        if let Some((lines, part)) = hf("headerReference") {
            pkg.header_part = Some(part);
            match only_page(&lines) {
                Some(Align::Right) => doc.page.page_numbers = PageNumPos::TopRight,
                _ => doc.header = crate::print::hf_join(&lines),
            }
        }
        if let Some((lines, part)) = hf("footerReference") {
            pkg.footer_part = Some(part);
            match only_page(&lines) {
                Some(Align::Right) => doc.page.page_numbers = PageNumPos::BottomRight,
                Some(Align::Center) => doc.page.page_numbers = PageNumPos::BottomCenter,
                _ => doc.footer = crate::print::hf_join(&lines),
            }
        }
    }
    pkg.page = doc.page.clone();
    pkg.core = doc.core.clone();
    doc.track_changes = target_of("/settings")
        .map(|p| tree(&p))
        .and_then(|x| {
            x.child("settings")
                .and_then(|s| s.child("trackRevisions"))
                .map(on)
        })
        .unwrap_or(false);
    pkg.track_changes = doc.track_changes;
    pkg.header = doc.header.clone();
    pkg.footer = doc.footer.clone();
    pkg.parts = parts;
    doc.docx = Some(Rc::new(pkg));
    doc.normalize();
    Ok(doc)
}

// ===================================================================== export

struct Wr<'a> {
    doc: &'a Document,
    st: std::cell::Ref<'a, Store>,
    /// footnote id -> (docx id, endnote?)
    fn_ids: HashMap<u32, u32>,
    used_styles: BTreeSet<String>,
    /// Numbering: first free abstractNumId / numId, and what we added.
    abs_base: u32,
    num_next: u32,
    bullet_num: Option<u32>,
    new_nums: Vec<(u32, ListKind)>,
    in_number_list: Option<u32>,
}

/// Run properties for text with formatting `fmt` and properties `cp`, in a
/// context whose styles give `style_fmt`: the original items, with the
/// quick-format flags merged in.
pub(crate) fn rpr_xml(
    st: &Store,
    fmt: Fmt,
    cp: u32,
    style_fmt: Fmt,
    force_ref_style: bool,
) -> String {
    let mut items = st.cprops(cp).rpr.clone();
    let raw_on = |items: &Vec<(String, String)>, name: &str| {
        items
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, x)| parse_one(x))
            .map(|n| on(&n))
    };
    for (flag, name, on_xml, off_xml) in [
        (Fmt::BOLD, "b", "<w:b/>", r#"<w:b w:val="0"/>"#),
        (Fmt::ITALIC, "i", "<w:i/>", r#"<w:i w:val="0"/>"#),
        (
            Fmt::UNDERLINE,
            "u",
            r#"<w:u w:val="single"/>"#,
            r#"<w:u w:val="none"/>"#,
        ),
    ] {
        let want_on = fmt.has(flag);
        let want_off = !want_on && fmt.off(flag) && style_fmt.has(flag);
        let raw = raw_on(&items, name);
        let xml = match (want_on, want_off, raw) {
            (true, _, Some(true)) | (_, true, Some(false)) => continue,
            (true, _, _) => Some(on_xml.to_string()),
            (_, true, _) => Some(off_xml.to_string()),
            _ => None,
        };
        crate::props::set_item(&mut items, name, xml);
    }
    // Strikeout: keep a double strike if that's what the text had.
    let want_on = fmt.has(Fmt::STRIKE);
    let want_off = !want_on && fmt.off(Fmt::STRIKE) && style_fmt.has(Fmt::STRIKE);
    let (s_raw, d_raw) = (raw_on(&items, "strike"), raw_on(&items, "dstrike"));
    if !(want_on && (s_raw == Some(true) || d_raw == Some(true)))
        && !(want_off && s_raw == Some(false))
    {
        crate::props::set_item(&mut items, "dstrike", None);
        let xml = if want_on {
            Some("<w:strike/>".into())
        } else if want_off {
            Some(r#"<w:strike w:val="0"/>"#.into())
        } else {
            None
        };
        crate::props::set_item(&mut items, "strike", xml);
    }
    let want_va = if fmt.has(Fmt::SUPER) {
        Some("superscript")
    } else if fmt.has(Fmt::SUB) {
        Some("subscript")
    } else {
        None
    };
    let raw_va = items
        .iter()
        .find(|(n, _)| n == "vertAlign")
        .and_then(|(_, x)| parse_one(x))
        .and_then(|n| n.attr("val").map(str::to_string));
    let raw_is_script = matches!(raw_va.as_deref(), Some("superscript" | "subscript"));
    if raw_va.as_deref() != want_va && (want_va.is_some() || raw_is_script) {
        crate::props::set_item(
            &mut items,
            "vertAlign",
            want_va.map(|v| format!(r#"<w:vertAlign w:val="{v}"/>"#)),
        );
    }
    if force_ref_style && !items.iter().any(|(n, _)| n == "rStyle") {
        items.push((
            "rStyle".into(),
            r#"<w:rStyle w:val="FootnoteReference"/>"#.into(),
        ));
    }
    let inner = ordered(RPR_ORDER, items);
    if inner.is_empty() {
        inner
    } else {
        format!("<w:rPr>{inner}</w:rPr>")
    }
}

/// Adjust a wrapper stack: close until `cur` is a prefix of `want`, then open
/// the rest.
fn set_wraps(out: &mut String, st: &Store, cur: &mut Vec<u32>, want: &[u32]) {
    let common = cur.iter().zip(want).take_while(|(a, b)| a == b).count();
    while cur.len() > common {
        let w = cur.pop().unwrap();
        out.push_str(
            &st.wraps
                .get(w as usize)
                .map(|d| d.close.clone())
                .unwrap_or_default(),
        );
    }
    for &w in &want[common..] {
        out.push_str(
            &st.wraps
                .get(w as usize)
                .map(|d| d.open.clone())
                .unwrap_or_default(),
        );
        cur.push(w);
    }
}

impl Wr<'_> {
    fn alloc_num(&mut self, kind: ListKind) -> u32 {
        match kind {
            ListKind::Bullet => {
                if let Some(n) = self.bullet_num {
                    return n;
                }
                let n = self.num_next;
                self.num_next += 1;
                self.bullet_num = Some(n);
                self.new_nums.push((n, kind));
                n
            }
            ListKind::Number => {
                if let Some(n) = self.in_number_list {
                    return n;
                }
                let n = self.num_next;
                self.num_next += 1;
                self.new_nums.push((n, kind));
                self.in_number_list = Some(n);
                n
            }
        }
    }

    fn use_style(&mut self, id: &str) {
        if let Some(s) = self.doc.styles.get(id)
            && !s.in_file
        {
            self.used_styles.insert(s.id.clone());
        }
    }

    fn ppr_xml(&mut self, p: &Paragraph, in_table: bool) -> String {
        let pp = &p.props;
        let x = p.extra.as_deref();
        let sheet = &self.doc.styles;
        let sp = sheet
            .get(&pp.style)
            .map(|s| s.props.clone())
            .unwrap_or_default();
        // Compare with what was read; new paragraphs compare with their style,
        // so only real direct formatting is written.
        let old: ParaProps = x.and_then(|x| x.at_read.clone()).unwrap_or_else(|| {
            let mut o = sp.clone();
            o.style = sheet.default_para.clone();
            if sheet.is_default(&pp.style) {
                o.style = pp.style.clone();
            }
            o
        });
        let mut items: Vec<(String, String)> = x.map(|x| x.ppr.clone()).unwrap_or_default();
        let find = |items: &Vec<(String, String)>, n: &str| items.iter().position(|(k, _)| k == n);
        let node_of = |items: &Vec<(String, String)>, n: &str| {
            find(items, n).and_then(|i| parse_one(&items[i].1))
        };
        let set = |items: &mut Vec<(String, String)>, n: &str, xml: Option<String>| {
            crate::props::set_item(items, n, xml)
        };

        if old.style != pp.style {
            let xml = (!sheet.is_default(&pp.style))
                .then(|| format!(r#"<w:pStyle w:val="{}"/>"#, esc(&pp.style)));
            let id = pp.style.clone();
            self.use_style(&id);
            set(&mut items, "pStyle", xml);
        }
        if old.align != pp.align {
            let v = match pp.align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
                Align::Full => "both",
            };
            set(
                &mut items,
                "jc",
                (pp.align != sp.align).then(|| format!(r#"<w:jc w:val="{v}"/>"#)),
            );
        }
        if old.page_break_before != pp.page_break_before {
            let xml = (pp.page_break_before != sp.page_break_before && !in_table).then(|| {
                if pp.page_break_before {
                    "<w:pageBreakBefore/>".into()
                } else {
                    r#"<w:pageBreakBefore w:val="0"/>"#.into()
                }
            });
            set(&mut items, "pageBreakBefore", xml);
        }
        // Lists made in WordBench: numbered lists that restart get their own
        // numbering instance.
        match pp.list {
            Some(li) if li.kind == ListKind::Number && li.num == 0 => {}
            _ if !in_table => self.in_number_list = None,
            _ => {}
        }
        if old.list != pp.list {
            let xml = match pp.list.filter(|_| !in_table) {
                _ if pp.list == sp.list => None,
                Some(li) if li.num != 0 => Some(format!(
                    r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{}"/></w:numPr>"#,
                    li.level, li.num
                )),
                Some(li) => {
                    let id = self.alloc_num(li.kind);
                    Some(format!(
                        r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{id}"/></w:numPr>"#,
                        li.level
                    ))
                }
                None => Some(r#"<w:numPr><w:numId w:val="0"/></w:numPr>"#.into()),
            };
            set(&mut items, "numPr", xml);
        } else if let Some(li) = pp.list.filter(|l| l.kind == ListKind::Number && l.num == 0) {
            let _ = li;
            if let Some(id) = node_of(&items, "numPr").and_then(|n| {
                n.child("numId")
                    .and_then(|c| c.attr("val"))
                    .and_then(|v| v.parse().ok())
            }) {
                self.in_number_list = Some(id);
            }
        }
        if (old.spacing, old.space_before, old.space_after)
            != (pp.spacing, pp.space_before, pp.space_after)
        {
            let mut n = node_of(&items, "spacing").unwrap_or_else(|| XNode::elem("spacing"));
            let tw = |lines: u8| (lines as u32 * 240).to_string();
            if pp.spacing != sp.spacing {
                n.set_attr("line", if pp.spacing >= 2 { "480" } else { "240" });
                n.set_attr("lineRule", "auto");
            } else if old.spacing != pp.spacing {
                n.remove_attr("line");
                n.remove_attr("lineRule");
            }
            if old.space_before != pp.space_before {
                n.remove_attr("beforeAutospacing");
                if pp.space_before != sp.space_before {
                    n.set_attr("before", tw(pp.space_before))
                } else {
                    n.remove_attr("before")
                }
            }
            if old.space_after != pp.space_after {
                n.remove_attr("afterAutospacing");
                if pp.space_after != sp.space_after {
                    n.set_attr("after", tw(pp.space_after))
                } else {
                    n.remove_attr("after")
                }
            }
            set(
                &mut items,
                "spacing",
                (!n.attrs.is_empty()).then(|| n.to_xml()),
            );
        }
        if (old.indent, old.first_indent) != (pp.indent, pp.first_indent) {
            let mut n = node_of(&items, "ind").unwrap_or_else(|| XNode::elem("ind"));
            for k in ["start", "left", "firstLine", "hanging"] {
                n.remove_attr(k);
            }
            if (pp.indent, pp.first_indent) != (sp.indent, sp.first_indent) {
                n.set_attr(
                    "left",
                    ((pp.indent as f32 * TWIPS_PER_COL) as i32).to_string(),
                );
                let first = (pp.first_indent as f32 * TWIPS_PER_COL) as i32;
                if first >= 0 {
                    n.set_attr("firstLine", first.to_string());
                } else {
                    n.set_attr("hanging", (-first).to_string());
                }
            }
            set(&mut items, "ind", (!n.attrs.is_empty()).then(|| n.to_xml()));
        }
        let inner = ordered(PPR_ORDER, items);
        if inner.is_empty() {
            inner
        } else {
            format!("<w:pPr>{inner}</w:pPr>")
        }
    }

    fn para_xml(&mut self, p: &Paragraph, in_table: bool, out: &mut String) {
        // A cell that starts with a nested table has no paragraph before it.
        let starts_with_block = in_table
            && p.extra
                .as_ref()
                .is_none_or(|x| x.ppr.is_empty() && x.p_attrs.is_empty())
            && p.cells
                .first()
                .is_some_and(|c| self.st.obj(c.ch).is_some_and(|o| o.kind == ObjKind::Block));
        if !starts_with_block {
            let attrs = p.extra.as_ref().map(|x| x.p_attrs.as_str()).unwrap_or("");
            out.push_str(&format!("<w:p{attrs}>"));
            let ppr = self.ppr_xml(p, in_table);
            out.push_str(&ppr);
        }
        let style_fmt = self
            .doc
            .styles
            .get(&p.props.style)
            .map(|s| s.fmt)
            .unwrap_or_default();
        self.runs_xml(p, out, !starts_with_block, style_fmt);
        out.push_str("</w:p>");
    }

    fn runs_xml(&mut self, p: &Paragraph, out: &mut String, p_open: bool, style_fmt: Fmt) {
        let st = &self.st;
        let sheet = &self.doc.styles;
        let mut wraps: Vec<u32> = Vec::new();
        let mut run: Option<String> = None;
        let mut cache: HashMap<(Fmt, u32, bool), String> = HashMap::new();
        let mut rpr_for = |fmt: Fmt, cp: u32, fnref: bool| -> String {
            cache
                .entry((fmt, cp, fnref))
                .or_insert_with(|| {
                    let cs = st
                        .look(cp)
                        .char_style
                        .as_deref()
                        .and_then(|id| sheet.get(id));
                    rpr_xml(
                        st,
                        fmt,
                        cp,
                        style_fmt.union(cs.map(|s| s.fmt).unwrap_or_default()),
                        fnref,
                    )
                })
                .clone()
        };
        let mut text = String::new();
        let mut p_open = p_open;

        fn flush(out: &mut String, text: &mut String) {
            if !text.is_empty() {
                out.push_str(&format!(r#"<w:t xml:space="preserve">{}</w:t>"#, esc(text)));
                text.clear();
            }
        }
        let close_run = |out: &mut String, text: &mut String, run: &mut Option<String>| {
            flush(out, text);
            if run.take().is_some() {
                out.push_str("</w:r>");
            }
        };
        let open_run = |out: &mut String,
                        text: &mut String,
                        run: &mut Option<String>,
                        rpr: String,
                        fnref: bool| {
            if run.as_ref() != Some(&rpr) || fnref {
                flush(out, text);
                if run.take().is_some() {
                    out.push_str("</w:r>");
                }
                out.push_str("<w:r>");
                out.push_str(&rpr);
                *run = Some(rpr);
            }
        };

        for c in p.cells.iter() {
            let want = &st.cprops(c.cp).wraps;
            if *want != wraps {
                close_run(out, &mut text, &mut run);
                set_wraps(out, st, &mut wraps, want);
            }
            let kind = st.obj(c.ch).map(|o| o.kind);
            if !p_open && kind != Some(ObjKind::Block) {
                out.push_str("<w:p>");
                p_open = true;
                if let Some(o) = st.obj(c.ch).filter(|o| o.kind == ObjKind::ParaBreak) {
                    out.push_str(&o.xml);
                    continue;
                }
            }
            if let Some(o) = st.obj(c.ch) {
                match o.kind {
                    ObjKind::ParaBreak => {
                        close_run(out, &mut text, &mut run);
                        set_wraps(out, st, &mut wraps, &[]);
                        out.push_str("</w:p><w:p>");
                        out.push_str(&o.xml);
                    }
                    ObjKind::Block => {
                        close_run(out, &mut text, &mut run);
                        set_wraps(out, st, &mut wraps, &[]);
                        if p_open {
                            out.push_str("</w:p>");
                        }
                        out.push_str(&o.xml);
                        p_open = false;
                    }
                    _ if o.in_run => {
                        open_run(out, &mut text, &mut run, rpr_for(c.fmt, c.cp, false), false);
                        flush(out, &mut text);
                        out.push_str(&o.xml);
                    }
                    _ => {
                        close_run(out, &mut text, &mut run);
                        out.push_str(&o.xml);
                    }
                }
                continue;
            }
            if let Some(id) = fn_id(c.ch) {
                let Some(&n) = self.fn_ids.get(&id) else {
                    continue;
                };
                if !st.cprops(c.cp).rpr.iter().any(|(k, _)| k == "rStyle") {
                    self.used_styles.insert("FootnoteReference".into());
                }
                let endnote = self.doc.note_meta.get(&id).is_some_and(|m| m.endnote);
                // Superscript comes from the reference style when there is one.
                let styled = st.cprops(c.cp).rpr.iter().any(|(k, _)| k == "rStyle");
                let fmt = if styled {
                    c.fmt
                } else {
                    c.fmt.with(Fmt::SUPER, true)
                };
                open_run(out, &mut text, &mut run, rpr_for(fmt, c.cp, true), true);
                let tag = if endnote {
                    "endnoteReference"
                } else {
                    "footnoteReference"
                };
                out.push_str(&format!(r#"<w:{tag} w:id="{n}"/>"#));
                close_run(out, &mut text, &mut run);
                continue;
            }
            open_run(out, &mut text, &mut run, rpr_for(c.fmt, c.cp, false), false);
            match c.ch {
                '\t' => {
                    flush(out, &mut text);
                    out.push_str("<w:tab/>");
                }
                '\u{2011}' => {
                    flush(out, &mut text);
                    out.push_str("<w:noBreakHyphen/>");
                }
                ch => text.push(ch),
            }
        }
        close_run(out, &mut text, &mut run);
        set_wraps(out, st, &mut wraps, &[]);
        if !p_open {
            out.push_str("<w:p>");
        }
    }

    fn table_xml(&mut self, t: &Table, out: &mut String) {
        let x = t.extra.as_deref();
        let border = r#"w:val="single" w:sz="4" w:space="0" w:color="auto""#;
        out.push_str("<w:tbl>");
        match x.and_then(|x| x.tbl_pr.as_deref()) {
            Some(raw) => out.push_str(raw),
            None => out.push_str(&format!(
                r#"<w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top {border}/><w:left {border}/><w:bottom {border}/><w:right {border}/><w:insideH {border}/><w:insideV {border}/></w:tblBorders><w:tblLook w:val="0000"/></w:tblPr>"#
            )),
        }
        let widths: Vec<i32> = t
            .cols
            .iter()
            .map(|&c| ((c + 3) as f32 * TWIPS_PER_COL) as i32)
            .collect();
        match x
            .and_then(|x| x.grid.as_ref())
            .filter(|(cols, _)| *cols == t.cols)
        {
            Some((_, raw)) => out.push_str(raw),
            None => {
                out.push_str("<w:tblGrid>");
                for w in &widths {
                    out.push_str(&format!(r#"<w:gridCol w:w="{w}"/>"#));
                }
                out.push_str("</w:tblGrid>");
            }
        }
        for row in &t.rows {
            let (attrs, inner) = row[0]
                .extra
                .as_ref()
                .and_then(|x| x.tr.clone())
                .unwrap_or_default();
            out.push_str(&format!("<w:tr{attrs}>{inner}"));
            for (ci, p) in row.iter().enumerate() {
                let px = p.extra.as_deref();
                if px.is_some_and(|x| x.covered) {
                    continue;
                }
                out.push_str("<w:tc>");
                match px.map(|x| x.tc_pr.as_deref()) {
                    Some(Some(raw)) => out.push_str(raw),
                    // Read from a file without cell properties: keep it that way.
                    Some(None) if px.is_some_and(|x| x.at_read.is_some()) => {}
                    _ => {
                        let w = widths.get(ci).copied().unwrap_or(1440);
                        out.push_str(&format!(
                            r#"<w:tcPr><w:tcW w:w="{w}" w:type="dxa"/></w:tcPr>"#
                        ));
                    }
                }
                self.para_xml(p, true, out);
                out.push_str("</w:tc>");
            }
            out.push_str("</w:tr>");
        }
        out.push_str("</w:tbl>");
    }

    fn body_xml(&mut self) -> String {
        let mut out = String::new();
        let mut bw: Vec<u32> = Vec::new();
        for b in &self.doc.blocks {
            let (want, pre) = match b {
                Block::Para(p) => p
                    .extra
                    .as_deref()
                    .map(|x| (x.bwraps.clone(), x.pre.clone())),
                Block::Table(t) => t
                    .extra
                    .as_deref()
                    .map(|x| (x.bwraps.clone(), x.pre.clone())),
            }
            .unwrap_or_default();
            set_wraps(&mut out, &self.st, &mut bw, &want);
            out.push_str(&pre);
            match b {
                Block::Para(p) => self.para_xml(p, false, &mut out),
                Block::Table(t) => {
                    self.in_number_list = None;
                    self.table_xml(t, &mut out);
                }
            }
        }
        set_wraps(&mut out, &self.st, &mut bw, &[]);
        out
    }
}

/// Header/footer paragraph(s) with page fields.
fn hf_para_xml(text: &str, align: Align) -> String {
    let jc = match align {
        Align::Center => r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#,
        Align::Right => r#"<w:pPr><w:jc w:val="right"/></w:pPr>"#,
        _ => "",
    };
    let field = |instr: &str| {
        format!(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> {instr} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>"#
        )
    };
    let run = |s: &str| {
        if s.is_empty() {
            String::new()
        } else {
            format!(r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#, esc(s))
        }
    };
    let mut runs = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let next = [
            ("{page}", "PAGE"),
            ("{pages}", "NUMPAGES"),
            ("{date}", "DATE \\@ \"MMMM d, yyyy\""),
        ]
        .iter()
        .filter_map(|(k, f)| rest.find(k).map(|i| (i, *k, *f)))
        .min_by_key(|(i, _, _)| *i);
        match next {
            Some((i, k, f)) => {
                runs.push_str(&run(&rest[..i]));
                runs.push_str(&field(f));
                rest = &rest[i + k.len()..];
            }
            None => {
                runs.push_str(&run(rest));
                rest = "";
            }
        }
    }
    format!("<w:p>{jc}{runs}</w:p>")
}

fn hf_part_xml(tag: &str, parts: &[(String, Align)]) -> String {
    let paras: String = if parts.is_empty() {
        "<w:p/>".into()
    } else {
        parts.iter().map(|(t, a)| hf_para_xml(t, *a)).collect()
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:{tag} {NS}>{paras}</w:{tag}>"
    )
}

/// styles.xml for a new document: WordBench's styles.
fn styles_xml(doc: &Document) -> String {
    let defs: String = doc
        .styles
        .styles
        .iter()
        .filter_map(|s| s.xml.clone())
        .collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles {NS}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" w:eastAsia="Courier New"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>{defs}</w:styles>"#
    )
}

fn abstract_num_xml(id: u32, kind: ListKind) -> String {
    let mut lvls = String::new();
    for l in 0..9u32 {
        let ind = 360 * (l + 1) + 360;
        let (fmt, text) = match kind {
            ListKind::Bullet => ("bullet", ["•", "◦", "▪"][(l % 3) as usize].to_string()),
            ListKind::Number => (
                ["decimal", "lowerLetter", "lowerRoman"][(l % 3) as usize],
                format!("%{}.", l + 1),
            ),
        };
        lvls.push_str(&format!(
            r#"<w:lvl w:ilvl="{l}"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{ind}" w:hanging="360"/></w:pPr></w:lvl>"#
        ));
    }
    format!(
        r#"<w:abstractNum w:abstractNumId="{id}"><w:multiLevelType w:val="hybridMultilevel"/>{lvls}</w:abstractNum>"#
    )
}

/// Patch the final `w:sectPr`: page size/margins and new header/footer refs.
fn sect_xml(doc: &Document, pkg: Option<&DocxPkg>, hdr: Option<&str>, ftr: Option<&str>) -> String {
    let pg = &doc.page;
    let tw = |inches: f32| ((inches * TWIPS_PER_IN).round() as i32).to_string();
    let mut n = pkg
        .and_then(|p| p.sect.as_deref())
        .and_then(parse_one)
        .unwrap_or_else(|| {
            let mut n = XNode::elem("sectPr");
            n.children.push(XNode::elem("pgSz"));
            let mut m = XNode::elem("pgMar");
            for (k, v) in [("header", "720"), ("footer", "720"), ("gutter", "0")] {
                m.set_attr(k, v);
            }
            n.children.push(m);
            n
        });
    let changed = pkg.is_none_or(|p| p.page != *pg);
    if changed {
        if n.child("pgSz").is_none() {
            n.children.push(XNode::elem("pgSz"));
        }
        let sz = n.child_mut("pgSz").unwrap();
        sz.set_attr("w", tw(pg.width));
        sz.set_attr("h", tw(pg.height));
        if pg.width > pg.height {
            sz.set_attr("orient", "landscape");
        } else {
            sz.remove_attr("orient");
        }
        if n.child("pgMar").is_none() {
            n.children.push(XNode::elem("pgMar"));
        }
        let m = n.child_mut("pgMar").unwrap();
        m.set_attr("top", tw(pg.margin_top));
        m.set_attr("right", tw(pg.margin_right));
        m.set_attr("bottom", tw(pg.margin_bottom));
        m.set_attr("left", tw(pg.margin_left));
        for (k, v) in [("header", "720"), ("footer", "720"), ("gutter", "0")] {
            if m.attr(k).is_none() {
                m.set_attr(k, v);
            }
        }
    }
    let mut refs = Vec::new();
    for (tag, rid) in [("headerReference", hdr), ("footerReference", ftr)] {
        if let Some(rid) = rid {
            n.children
                .retain(|c| !(c.name == tag && c.attr("type").is_none_or(|t| t == "default")));
            let mut r = XNode::elem(tag);
            r.set_attr("type", "default");
            r.attrs.push(("r:id".into(), rid.into()));
            refs.push(r);
        }
    }
    // Header/footer references come first in a sectPr.
    let insert_at = n
        .children
        .iter()
        .take_while(|c| {
            c.name == "headerReference" || c.name == "footerReference" || c.name == "#text"
        })
        .count();
    for (k, r) in refs.into_iter().enumerate() {
        n.children.insert((insert_at + k).min(n.children.len()), r);
    }
    n.to_xml()
}

/// Parsed [Content_Types].xml that only re-serializes when changed.
struct ContentTypes {
    root: XNode,
    changed: bool,
}

impl ContentTypes {
    fn new(bytes: Option<&[u8]>) -> ContentTypes {
        let root = bytes
            .and_then(|b| parse(&String::from_utf8_lossy(b)).ok())
            .and_then(|x| x.children.into_iter().find(|c| c.name == "Types"));
        match root {
            Some(root) => ContentTypes {
                root,
                changed: false,
            },
            None => {
                let mut root = XNode {
                    name: "Types".into(),
                    qname: "Types".into(),
                    ..Default::default()
                };
                root.attrs.push((
                    "xmlns".into(),
                    "http://schemas.openxmlformats.org/package/2006/content-types".into(),
                ));
                let mut ct = ContentTypes {
                    root,
                    changed: true,
                };
                ct.add_default(
                    "rels",
                    "application/vnd.openxmlformats-package.relationships+xml",
                );
                ct.add_default("xml", "application/xml");
                ct
            }
        }
    }
    fn add_default(&mut self, ext: &str, ty: &str) {
        if self.root.children.iter().any(|c| {
            c.name == "Default"
                && c.attr("Extension")
                    .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        }) {
            return;
        }
        let mut n = XNode {
            name: "Default".into(),
            qname: "Default".into(),
            ..Default::default()
        };
        n.attrs = vec![
            ("Extension".into(), ext.into()),
            ("ContentType".into(), ty.into()),
        ];
        self.root.children.insert(0, n);
        self.changed = true;
    }
    fn ensure(&mut self, part: &str, ty: &str) {
        let name = format!("/{part}");
        if self
            .root
            .children
            .iter()
            .any(|c| c.name == "Override" && c.attr("PartName") == Some(name.as_str()))
        {
            return;
        }
        let mut n = XNode {
            name: "Override".into(),
            qname: "Override".into(),
            ..Default::default()
        };
        n.attrs = vec![("PartName".into(), name), ("ContentType".into(), ty.into())];
        self.root.children.push(n);
        self.changed = true;
    }
    fn xml(&self) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}",
            self.root.to_xml()
        )
    }
}

/// Insert XML strings into a root element before the first child named one
/// of `before` (or at the end).
fn insert_into(root: &XNode, items: &[String], before: &[&str]) -> String {
    let pos = root
        .children
        .iter()
        .position(|c| before.contains(&c.name.as_str()))
        .unwrap_or(root.children.len());
    let mut s = root.start_tag();
    for (i, c) in root.children.iter().enumerate() {
        if i == pos {
            items.iter().for_each(|x| s.push_str(x));
        }
        s.push_str(&c.to_xml());
    }
    if pos == root.children.len() {
        items.iter().for_each(|x| s.push_str(x));
    }
    s.push_str(&root.end_tag());
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{s}")
}

fn max_attr(root: &XNode, elem: &str, attr: &str) -> i64 {
    root.children_named(elem)
        .filter_map(|n| n.attr(attr)?.parse::<i64>().ok())
        .max()
        .unwrap_or(-1)
}

/// The notes part; the flag says whether any note was written fresh (and so
/// needs the note styles).
fn notes_xml(
    doc: &Document,
    tag: &str,
    root_open: Option<String>,
    seps: &[String],
    notes: &[(u32, u32)],
) -> (String, bool) {
    let mut fresh = false;
    let mut s = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".to_string();
    s.push_str(&root_open.unwrap_or_else(|| format!("<w:{tag}s {NS}>")));
    if seps.is_empty() {
        s.push_str(&format!(
            r#"<w:{tag} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{tag}><w:{tag} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{tag}>"#
        ));
    } else {
        seps.iter().for_each(|x| s.push_str(x));
    }
    let style = if tag == "footnote" {
        "FootnoteText"
    } else {
        "EndnoteText"
    };
    let refstyle = if tag == "footnote" {
        "FootnoteReference"
    } else {
        "EndnoteReference"
    };
    for &(id, n) in notes {
        let text = doc.footnotes.get(&id).map(String::as_str).unwrap_or("");
        let kept = doc
            .note_meta
            .get(&id)
            .filter(|m| m.orig_text == text && !m.xml.is_empty())
            .and_then(|m| parse_one(&m.xml));
        match kept {
            Some(mut node) => {
                node.set_attr("id", n.to_string());
                s.push_str(&node.to_xml());
            }
            None => {
                fresh = true;
                s.push_str(&format!(
                r#"<w:{tag} w:id="{n}"><w:p><w:pPr><w:pStyle w:val="{style}"/></w:pPr><w:r><w:rPr><w:rStyle w:val="{refstyle}"/></w:rPr><w:{tag}Ref/></w:r><w:r><w:t xml:space="preserve"> {}</w:t></w:r></w:p></w:{tag}>"#,
                esc(text)
            ))
            }
        }
    }
    s.push_str(&format!("</w:{tag}s>"));
    (s, fresh)
}

fn first_free_id(seps: &[String]) -> u32 {
    seps.iter()
        .filter_map(|x| parse_one(x)?.attr("id")?.parse::<i64>().ok())
        .max()
        .map(|m| (m + 1).max(1) as u32)
        .unwrap_or(1)
}

pub fn to_bytes(doc: &Document) -> Result<Vec<u8>, String> {
    let pkg = doc.docx.as_deref();
    let main = pkg
        .map(|p| p.main.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let dir = dir_of(&main).to_string();
    let mut parts: Vec<(String, Vec<u8>)> = pkg.map(|p| p.parts.clone()).unwrap_or_default();
    let get = |parts: &Vec<(String, Vec<u8>)>, name: &str| {
        parts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.clone())
    };
    let put = |parts: &mut Vec<(String, Vec<u8>)>, name: &str, bytes: Vec<u8>| match parts
        .iter_mut()
        .find(|(n, _)| n == name)
    {
        Some(p) => p.1 = bytes,
        None => parts.push((name.to_string(), bytes)),
    };
    let mut ct = ContentTypes::new(get(&parts, "[Content_Types].xml").as_deref());
    let rels_name = rels_path(&main);
    let mut rels = read_rels(get(&parts, &rels_name).as_deref());
    let rels_before = rels.len();
    let rel_target = |rels: &Vec<Rel>, ty: &str| {
        rels.iter()
            .find(|r| r.ty == format!("{REL_BASE}/{ty}"))
            .map(|r| resolve(&dir, &r.target))
    };
    let add_rel = |rels: &mut Vec<Rel>, ty: &str, target: &str| -> String {
        let mut k = rels.len() + 1;
        let id = loop {
            let id = format!("rIdWb{k}");
            if !rels.iter().any(|r| r.id == id) {
                break id;
            }
            k += 1;
        };
        rels.push(Rel {
            id: id.clone(),
            ty: format!("{REL_BASE}/{ty}"),
            target: target.into(),
            mode: None,
        });
        id
    };
    let short = |full: &str| {
        full.strip_prefix(&format!("{dir}/"))
            .unwrap_or(full)
            .to_string()
    };

    // Footnote/endnote numbering in order of appearance.
    let order = doc.footnote_numbers();
    let fn_seps = pkg.map(|p| p.note_seps.clone()).unwrap_or_default();
    let en_seps = pkg.map(|p| p.endnote_seps.clone()).unwrap_or_default();
    let (mut next_fn, mut next_en) = (first_free_id(&fn_seps), first_free_id(&en_seps));
    let mut fn_ids = HashMap::new();
    let (mut foot, mut end) = (Vec::new(), Vec::new());
    for id in &order {
        if doc.note_meta.get(id).is_some_and(|m| m.endnote) {
            fn_ids.insert(*id, next_en);
            end.push((*id, next_en));
            next_en += 1;
        } else {
            fn_ids.insert(*id, next_fn);
            foot.push((*id, next_fn));
            next_fn += 1;
        }
    }

    // Numbering: new lists get ids after the file's own.
    let numbering_path = rel_target(&rels, "numbering");
    let numbering_root = numbering_path
        .as_ref()
        .and_then(|p| get(&parts, p))
        .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
        .and_then(|x| x.children.into_iter().find(|c| c.name == "numbering"));
    let (abs_base, num_base) = match &numbering_root {
        Some(r) => (
            (max_attr(r, "abstractNum", "abstractNumId") + 1) as u32,
            (max_attr(r, "num", "numId") + 1).max(1) as u32,
        ),
        None => (0, 1),
    };

    let mut wr = Wr {
        doc,
        st: doc.store.borrow(),
        fn_ids,
        used_styles: BTreeSet::new(),
        abs_base,
        num_next: num_base,
        bullet_num: None,
        new_nums: Vec::new(),
        in_number_list: None,
    };
    let body = wr.body_xml();
    wr.used_styles.extend(doc.added_styles.iter().cloned());

    // Headers and footers: rewritten only when their text changed.
    let (hdr, ftr) = crate::print::header_footer_parts(doc);
    let top_right = |p: &PageSetup| p.page_numbers == PageNumPos::TopRight;
    let bottom = |p: &PageSetup| {
        matches!(
            p.page_numbers,
            PageNumPos::BottomCenter | PageNumPos::BottomRight
        )
    };
    let mut hf_rids: [Option<String>; 2] = [None, None];
    for (k, (tag, content)) in [("hdr", &hdr), ("ftr", &ftr)].into_iter().enumerate() {
        let changed = match pkg {
            Some(p) if k == 0 => {
                p.header != doc.header
                    || top_right(&p.page) != top_right(&doc.page)
                    || (p.header_part.is_none() && !content.is_empty())
            }
            Some(p) => {
                p.footer != doc.footer
                    || p.page.page_numbers != doc.page.page_numbers
                        && (bottom(&p.page) || bottom(&doc.page))
                    || (p.footer_part.is_none() && !content.is_empty())
            }
            None => !content.is_empty(),
        };
        if !changed {
            continue;
        }
        let existing = pkg.and_then(|p| {
            if k == 0 {
                p.header_part.clone()
            } else {
                p.footer_part.clone()
            }
        });
        let xml = hf_part_xml(tag, content);
        match existing {
            Some(part) => put(&mut parts, &part, xml.into_bytes()),
            None if !content.is_empty() => {
                let (name, ty) = if k == 0 {
                    ("header_wb1.xml", "header")
                } else {
                    ("footer_wb1.xml", "footer")
                };
                let full = format!("{dir}/{name}");
                put(&mut parts, &full, xml.into_bytes());
                ct.ensure(&full, &format!("{CT_MAIN}.{ty}+xml"));
                hf_rids[k] = Some(add_rel(&mut rels, ty, name));
            }
            None => {}
        }
    }

    // Notes.
    for (tag, notes, seps, ty) in [
        ("footnote", &foot, &fn_seps, "footnotes"),
        ("endnote", &end, &en_seps, "endnotes"),
    ] {
        let existing = rel_target(&rels, ty);
        if notes.is_empty() && existing.is_none() {
            continue;
        }
        let root_open = existing
            .as_ref()
            .and_then(|p| get(&parts, p))
            .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
            .and_then(|x| x.children.into_iter().find(|c| c.name == format!("{tag}s")))
            .map(|r| r.start_tag());
        let (xml, fresh) = notes_xml(doc, tag, root_open, seps, notes);
        if fresh && tag == "footnote" {
            wr.used_styles.insert("FootnoteText".into());
            wr.used_styles.insert("FootnoteReference".into());
        }
        let path = existing.unwrap_or_else(|| {
            let full = format!("{dir}/{ty}.xml");
            add_rel(&mut rels, ty, &short(&full));
            full
        });
        ct.ensure(&path, &format!("{CT_MAIN}.{ty}+xml"));
        put(&mut parts, &path, xml.into_bytes());
    }

    // Numbering definitions for new lists.
    if !wr.new_nums.is_empty() || numbering_root.is_none() && pkg.is_none() {
        let mut abs = Vec::new();
        let mut nums = Vec::new();
        for (k, (num, kind)) in wr.new_nums.iter().enumerate() {
            let aid = wr.abs_base + k as u32;
            abs.push(abstract_num_xml(aid, *kind));
            let restart = if *kind == ListKind::Number {
                r#"<w:lvlOverride w:ilvl="0"><w:startOverride w:val="1"/></w:lvlOverride>"#
            } else {
                ""
            };
            nums.push(format!(
                r#"<w:num w:numId="{num}"><w:abstractNumId w:val="{aid}"/>{restart}</w:num>"#
            ));
        }
        let xml = match &numbering_root {
            Some(root) => {
                let with_abs = insert_into(root, &abs, &["num", "numIdMacAtCleanup"]);
                let root2 = parse(&with_abs)
                    .ok()
                    .and_then(|x| x.children.into_iter().find(|c| c.name == "numbering"));
                match root2 {
                    Some(r) => insert_into(&r, &nums, &["numIdMacAtCleanup"]),
                    None => with_abs,
                }
            }
            None => format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering {NS}>{}{}</w:numbering>",
                abs.concat(),
                nums.concat()
            ),
        };
        let path = numbering_path.clone().unwrap_or_else(|| {
            let full = format!("{dir}/numbering.xml");
            add_rel(&mut rels, "numbering", "numbering.xml");
            full
        });
        ct.ensure(&path, &format!("{CT_MAIN}.numbering+xml"));
        put(&mut parts, &path, xml.into_bytes());
    }

    // Styles: add definitions for styles we use that the file lacks.
    match rel_target(&rels, "styles") {
        Some(path) => {
            if let Some(root) = get(&parts, &path)
                .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
                .and_then(|x| x.children.into_iter().find(|c| c.name == "styles"))
            {
                let have: BTreeSet<String> = root
                    .children_named("style")
                    .filter_map(|s| s.attr("styleId").map(str::to_string))
                    .collect();
                let missing: Vec<String> = wr
                    .used_styles
                    .iter()
                    .filter(|s| !have.contains(*s))
                    .filter_map(|s| doc.styles.get(s).and_then(|d| d.xml.clone()))
                    .collect();
                if !missing.is_empty() {
                    put(
                        &mut parts,
                        &path,
                        insert_into(&root, &missing, &[]).into_bytes(),
                    );
                }
            }
        }
        None => {
            let full = format!("{dir}/styles.xml");
            put(&mut parts, &full, styles_xml(doc).into_bytes());
            ct.ensure(&full, &format!("{CT_MAIN}.styles+xml"));
            add_rel(&mut rels, "styles", "styles.xml");
        }
    }

    // Pictures and links added while editing.
    for r in &doc.new_rels {
        if !rels.iter().any(|x| x.id == r.id) {
            rels.push(Rel {
                id: r.id.clone(),
                ty: format!("{REL_BASE}/{}", r.ty),
                target: r.target.clone(),
                mode: r.external.then(|| "External".to_string()),
            });
        }
    }
    for (path, bytes) in &doc.new_parts {
        put(&mut parts, path, bytes.clone());
        if let Some(ext) = path.rsplit('.').next() {
            let ty = match ext.to_ascii_lowercase().as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "bmp" => "image/bmp",
                _ => "application/octet-stream",
            };
            ct.add_default(&ext.to_ascii_lowercase(), ty);
        }
    }

    // Comments: rewritten only when one was added, edited or removed.
    let referenced: BTreeSet<String> = {
        let st = doc.store.borrow();
        let mut ids = BTreeSet::new();
        for pr in doc.refs() {
            for c in doc.para(pr).cells.iter() {
                if let Some(o) = st
                    .obj(c.ch)
                    .filter(|o| o.xml.starts_with("<w:commentReference"))
                    && let Some(id) =
                        parse_one(&o.xml).and_then(|n| n.attr("id").map(str::to_string))
                {
                    ids.insert(id);
                }
            }
        }
        ids
    };
    let comments_path = rel_target(&rels, "comments");
    let changed = doc
        .comments
        .iter()
        .any(|(id, c)| c.xml.is_empty() || c.text != c.orig_text || !referenced.contains(id))
        || comments_path.is_none() && !referenced.is_empty();
    if changed && (!doc.comments.is_empty() || comments_path.is_some()) {
        let root_open = comments_path
            .as_ref()
            .and_then(|p| get(&parts, p))
            .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
            .and_then(|x| x.children.into_iter().find(|c| c.name == "comments"))
            .map(|r| r.start_tag())
            .unwrap_or_else(|| format!("<w:comments {NS}>"));
        let mut xml =
            format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{root_open}");
        for (id, c) in doc
            .comments
            .iter()
            .filter(|(id, _)| referenced.contains(*id))
        {
            if !c.xml.is_empty() && c.text == c.orig_text {
                xml.push_str(&c.xml);
            } else {
                xml.push_str(&format!(
                    r#"<w:comment w:id="{}" w:author="{}" w:date="{}" w:initials="{}"><w:p><w:r><w:annotationRef/></w:r><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:comment>"#,
                    esc(id),
                    esc(&c.author),
                    esc(&c.date),
                    esc(&c.initials),
                    esc(&c.text)
                ));
            }
        }
        xml.push_str("</w:comments>");
        let path = comments_path.unwrap_or_else(|| {
            let full = format!("{dir}/comments.xml");
            add_rel(&mut rels, "comments", "comments.xml");
            full
        });
        ct.ensure(&path, &format!("{CT_MAIN}.comments+xml"));
        put(&mut parts, &path, xml.into_bytes());
    }

    // Document properties.
    let core_changed = match pkg {
        Some(p) => p.core != doc.core,
        None => doc.core != CoreProps::default(),
    };
    if core_changed {
        let core_path = read_rels(get(&parts, "_rels/.rels").as_deref())
            .into_iter()
            .find(|r| r.ty.ends_with("/core-properties"))
            .map(|r| resolve("", &r.target));
        let existing = core_path
            .as_ref()
            .and_then(|p| get(&parts, p))
            .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
            .and_then(|x| x.children.into_iter().find(|c| c.name == "coreProperties"));
        let mut root = existing.unwrap_or_else(|| {
            let mut r = XNode {
                name: "coreProperties".into(),
                qname: "cp:coreProperties".into(),
                ..Default::default()
            };
            r.attrs = vec![
                (
                    "xmlns:cp".into(),
                    "http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                        .into(),
                ),
                ("xmlns:dc".into(), "http://purl.org/dc/elements/1.1/".into()),
            ];
            r
        });
        if !root.attrs.iter().any(|(k, _)| k == "xmlns:dc") {
            root.attrs
                .push(("xmlns:dc".into(), "http://purl.org/dc/elements/1.1/".into()));
        }
        let cp_prefix = root
            .qname
            .split(':')
            .next()
            .filter(|_| root.qname.contains(':'))
            .unwrap_or("cp")
            .to_string();
        for (local, qname, value) in [
            ("title", "dc:title".to_string(), &doc.core.title),
            ("subject", "dc:subject".to_string(), &doc.core.subject),
            ("creator", "dc:creator".to_string(), &doc.core.author),
            (
                "keywords",
                format!("{cp_prefix}:keywords"),
                &doc.core.keywords,
            ),
            (
                "description",
                "dc:description".to_string(),
                &doc.core.description,
            ),
        ] {
            root.children.retain(|c| c.name != local);
            if !value.is_empty() {
                let text = XNode {
                    name: "#text".into(),
                    text: value.clone(),
                    ..Default::default()
                };
                root.children.push(XNode {
                    name: local.into(),
                    qname,
                    children: vec![text],
                    ..Default::default()
                });
            }
        }
        let path = core_path.unwrap_or_else(|| {
            // Register the new part with the package.
            let mut root_rels = read_rels(get(&parts, "_rels/.rels").as_deref());
            if !root_rels.is_empty() {
                root_rels.push(Rel {
                    id: "rIdWbCore".into(),
                    ty: "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties".into(),
                    target: "docProps/core.xml".into(),
                    mode: None,
                });
                put(&mut parts, "_rels/.rels", rels_xml(&root_rels).into_bytes());
            }
            "docProps/core.xml".into()
        });
        put(
            &mut parts,
            &path,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}",
                root.to_xml()
            )
            .into_bytes(),
        );
        ct.ensure(
            &path,
            "application/vnd.openxmlformats-package.core-properties+xml",
        );
    }

    // Track changes setting, if switched.
    if let (Some(p), Some(path)) = (pkg, rel_target(&rels, "settings"))
        && p.track_changes != doc.track_changes
        && let Some(root) = get(&parts, &path)
            .and_then(|b| parse(&String::from_utf8_lossy(&b)).ok())
            .and_then(|x| x.children.into_iter().find(|c| c.name == "settings"))
    {
        let mut root = root;
        root.children.retain(|c| c.name != "trackRevisions");
        let xml = if doc.track_changes {
            insert_into(
                &root,
                &["<w:trackRevisions/>".to_string()],
                &[
                    "doNotTrackMoves",
                    "doNotTrackFormatting",
                    "documentProtection",
                    "autoFormatOverride",
                    "styleLockTheme",
                    "styleLockQFSet",
                    "defaultTabStop",
                    "autoHyphenation",
                    "consecutiveHyphenLimit",
                    "hyphenationZone",
                    "doNotHyphenateCaps",
                    "evenAndOddHeaders",
                    "characterSpacingControl",
                    "footnotePr",
                    "endnotePr",
                    "compat",
                    "docVars",
                    "rsids",
                    "mathPr",
                    "themeFontLang",
                    "clrSchemeMapping",
                    "shapeDefaults",
                    "decimalSymbol",
                    "listSeparator",
                ],
            )
        } else {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{}",
                root.to_xml()
            )
        };
        put(&mut parts, &path, xml.into_bytes());
    }

    // Settings (new documents only; an existing file's are kept).
    if rel_target(&rels, "settings").is_none() {
        let full = format!("{dir}/settings.xml");
        let track = if doc.track_changes {
            "<w:trackRevisions/>"
        } else {
            ""
        };
        let xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:settings {NS}>{track}<w:footnotePr><w:footnote w:id=\"-1\"/><w:footnote w:id=\"0\"/></w:footnotePr></w:settings>"
        );
        put(&mut parts, &full, xml.into_bytes());
        ct.ensure(&full, &format!("{CT_MAIN}.settings+xml"));
        add_rel(&mut rels, "settings", "settings.xml");
    }

    // Main document.
    let sect = sect_xml(doc, pkg, hf_rids[0].as_deref(), hf_rids[1].as_deref());
    let mut root_open = pkg
        .map(|p| p.root_open.clone())
        .unwrap_or_else(|| format!("<w:document {NS}>"));
    for (prefix, ns) in [("w", W_NS), ("r", R_NS)] {
        let decl = format!("xmlns:{prefix}=\"");
        if !root_open.contains(&decl) {
            root_open = root_open.replacen('>', &format!(r#" xmlns:{prefix}="{ns}">"#), 1);
        }
    }
    let root_q = root_open[1..]
        .split(|c: char| c.is_whitespace() || c == '>')
        .next()
        .unwrap_or("w:document")
        .to_string();
    let doc_pre = pkg.map(|p| p.doc_pre.as_str()).unwrap_or("");
    let tail = pkg.map(|p| p.body_tail.as_str()).unwrap_or("");
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n{root_open}{doc_pre}<w:body>{body}{tail}{sect}</w:body></{root_q}>"
    );
    put(&mut parts, &main, document.into_bytes());
    ct.ensure(&main, &format!("{CT_MAIN}.document.main+xml"));
    drop(wr);

    // Package plumbing for brand-new documents.
    if get(&parts, "_rels/.rels").is_none() {
        let root_rels = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL_BASE}/officeDocument" Target="{main}"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#
        );
        put(&mut parts, "_rels/.rels", root_rels.into_bytes());
        let core = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>WordBench</dc:creator></cp:coreProperties>"#;
        if get(&parts, "docProps/core.xml").is_none() {
            put(&mut parts, "docProps/core.xml", core.as_bytes().to_vec());
        }
        ct.ensure(
            "docProps/core.xml",
            "application/vnd.openxmlformats-package.core-properties+xml",
        );
    }
    if rels.len() != rels_before || get(&parts, &rels_name).is_none() {
        put(&mut parts, &rels_name, rels_xml(&rels).into_bytes());
    }
    if ct.changed || get(&parts, "[Content_Types].xml").is_none() {
        put(&mut parts, "[Content_Types].xml", ct.xml().into_bytes());
    }

    // Content types first, then the rest in their original order.
    parts.sort_by_key(|(n, _)| n != "[Content_Types].xml");
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, content) in &parts {
            zw.start_file(name.as_str(), opts)
                .map_err(|e| e.to_string())?;
            zw.write_all(content).map_err(|e| e.to_string())?;
        }
        zw.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

pub fn save(doc: &Document, path: &Path) -> Result<(), String> {
    super::write_atomic(path, &to_bytes(doc)?)
}

#[cfg(test)]
mod tests;
