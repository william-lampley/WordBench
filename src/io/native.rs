//! Native format (.wbd): human-readable JSON that keeps everything the
//! editor knows about.

use super::write_atomic;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const FORMAT: &str = "wordbench";

#[derive(Serialize, Deserialize)]
struct NDoc {
    format: String,
    version: u32,
    page: NPage,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    header: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    footer: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    footnotes: BTreeMap<u32, String>,
    /// Ids of footnotes that are endnotes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    endnotes: Vec<u32>,
    blocks: Vec<NBlock>,
}

#[derive(Serialize, Deserialize)]
struct NPage {
    width: f32,
    height: f32,
    margin_left: f32,
    margin_right: f32,
    margin_top: f32,
    margin_bottom: f32,
    #[serde(default)]
    page_numbers: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum NBlock {
    Para(NPara),
    Table {
        cols: Vec<u16>,
        rows: Vec<Vec<NPara>>,
    },
}

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}
fn is_zero_i16(v: &i16) -> bool {
    *v == 0
}
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_one(v: &u8) -> bool {
    *v == 1
}
fn one() -> u8 {
    1
}
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Serialize, Deserialize, Default)]
struct NPara {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    align: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    list: Option<NList>,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    indent: u16,
    #[serde(default, skip_serializing_if = "is_zero_i16")]
    first_indent: i16,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    spacing: u8,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    space_before: u8,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    space_after: u8,
    #[serde(default, skip_serializing_if = "is_false")]
    page_break: bool,
    /// Runs as [text, format] or [text, format, run properties]; format
    /// letters b i u s ^ _ (capitals: explicitly off), "fn:<id>" for a
    /// footnote reference, "br"/"pg"/"par" for breaks. Run properties are
    /// Word `w:rPr` children (fonts, sizes, colours).
    #[serde(default)]
    runs: Vec<NRun>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum NRun {
    Plain(String, String),
    Props(String, String, Vec<(String, String)>),
}

#[derive(Serialize, Deserialize)]
struct NList {
    kind: String,
    level: u8,
}

fn para_out(p: &Paragraph, st: &Store) -> NPara {
    let pp = &p.props;
    NPara {
        align: if pp.align == Align::Left {
            String::new()
        } else {
            Align::NAMES[pp.align.index()].to_lowercase()
        },
        style: pp.style.clone(),
        list: pp.list.map(|l| NList {
            kind: if l.kind == ListKind::Bullet {
                "bullet".into()
            } else {
                "number".into()
            },
            level: l.level,
        }),
        indent: pp.indent,
        first_indent: pp.first_indent,
        spacing: pp.spacing,
        space_before: pp.space_before,
        space_after: pp.space_after,
        page_break: pp.page_break_before,
        runs: runs_out(p, st),
    }
}

fn runs_out(p: &Paragraph, st: &Store) -> Vec<NRun> {
    let mut out: Vec<(String, String, u32)> = Vec::new();
    for c in p.cells.iter() {
        let special = |code: &str| (String::new(), code.to_string(), 0);
        let item = if let Some(id) = fn_id(c.ch) {
            special(&format!("fn:{id}"))
        } else {
            match st.obj(c.ch).map(|o| o.kind) {
                Some(ObjKind::LineBreak) => special("br"),
                Some(ObjKind::PageBreak) => special("pg"),
                Some(ObjKind::ParaBreak) => special("par"),
                // Pictures, fields etc. only survive in .docx.
                Some(_) => continue,
                None => {
                    let code = c.fmt.to_code();
                    match out.last_mut() {
                        Some((t, f, cp)) if *f == code && *cp == c.cp && !t.is_empty() => {
                            t.push(c.ch);
                            continue;
                        }
                        _ => (c.ch.to_string(), code, c.cp),
                    }
                }
            }
        };
        out.push(item);
    }
    out.into_iter()
        .map(|(t, f, cp)| {
            // Only properties WordBench can edit or show travel in .wbd.
            let rpr: Vec<(String, String)> = st
                .cprops(cp)
                .rpr
                .iter()
                .filter(|(n, _)| {
                    matches!(
                        n.as_str(),
                        "rFonts" | "sz" | "szCs" | "color" | "highlight" | "caps" | "smallCaps"
                    )
                })
                .cloned()
                .collect();
            if rpr.is_empty() {
                NRun::Plain(t, f)
            } else {
                NRun::Props(t, f, rpr)
            }
        })
        .collect()
}

fn para_in(n: NPara, doc: &Document) -> Paragraph {
    let align = Align::NAMES
        .iter()
        .position(|a| a.eq_ignore_ascii_case(&n.align))
        .unwrap_or(0);
    let style = match doc
        .styles
        .get(&n.style)
        .or_else(|| doc.styles.find(&n.style))
    {
        Some(s) if !doc.styles.is_default(&s.id) => s.id.clone(),
        _ => String::new(),
    };
    let props = ParaProps {
        align: Align::from_index(align),
        style,
        list: n.list.map(|l| {
            ListInfo::new(
                if l.kind == "number" {
                    ListKind::Number
                } else {
                    ListKind::Bullet
                },
                l.level.min(8),
            )
        }),
        indent: n.indent,
        first_indent: n.first_indent,
        spacing: n.spacing.clamp(1, 2),
        space_before: n.space_before.min(3),
        space_after: n.space_after.min(3),
        page_break_before: n.page_break,
        ..Default::default()
    };
    let mut cells = Vec::new();
    for run in n.runs {
        let (text, f, cp) = match run {
            NRun::Plain(t, f) => (t, f, 0),
            NRun::Props(t, f, rpr) => (
                t,
                f,
                doc.store.borrow_mut().intern(CharProps {
                    rpr,
                    wraps: Vec::new(),
                }),
            ),
        };
        if let Some(id) = f.strip_prefix("fn:").and_then(|s| s.parse::<u32>().ok()) {
            cells.push(Cell::new(fn_char(id), Fmt::default()));
        } else if let Some((kind, xml, label)) = match f.as_str() {
            "br" => Some((ObjKind::LineBreak, "<w:br/>", "Ln Brk")),
            "pg" => Some((ObjKind::PageBreak, r#"<w:br w:type="page"/>"#, "HPg")),
            "par" => Some((ObjKind::ParaBreak, "", "")),
            _ => None,
        } {
            let in_run = kind != ObjKind::ParaBreak;
            let ch = doc.new_obj(InlineObj {
                kind,
                xml: xml.into(),
                label: label.into(),
                in_run,
            });
            cells.push(Cell::new(ch, Fmt::default()));
        } else {
            let fmt = Fmt::from_code(&f);
            cells.extend(
                text.chars()
                    .filter(|c| !c.is_control() || *c == '\t')
                    .map(|c| Cell::with_cp(c, fmt, cp)),
            );
        }
    }
    Paragraph::with_cells(cells, props)
}

pub fn to_string(doc: &Document) -> String {
    let used = doc.footnote_numbers();
    let st = doc.store.borrow();
    let nd = NDoc {
        format: FORMAT.into(),
        version: 1,
        page: NPage {
            width: doc.page.width,
            height: doc.page.height,
            margin_left: doc.page.margin_left,
            margin_right: doc.page.margin_right,
            margin_top: doc.page.margin_top,
            margin_bottom: doc.page.margin_bottom,
            page_numbers: doc.page.page_numbers.index(),
        },
        header: doc.header.clone(),
        footer: doc.footer.clone(),
        footnotes: doc
            .footnotes
            .iter()
            .filter(|(k, _)| used.contains(k))
            .map(|(k, v)| (*k, v.clone()))
            .collect(),
        endnotes: doc
            .note_meta
            .iter()
            .filter(|(k, m)| m.endnote && used.contains(k))
            .map(|(k, _)| *k)
            .collect(),
        blocks: doc
            .blocks
            .iter()
            .map(|b| match b {
                Block::Para(p) => NBlock::Para(para_out(p, &st)),
                Block::Table(t) => NBlock::Table {
                    cols: t.cols.clone(),
                    rows: t
                        .rows
                        .iter()
                        .map(|r| r.iter().map(|p| para_out(p, &st)).collect())
                        .collect(),
                },
            })
            .collect(),
    };
    serde_json::to_string_pretty(&nd).unwrap_or_default()
}

pub fn from_str(s: &str) -> Result<Document, String> {
    let nd: NDoc = serde_json::from_str(s).map_err(|e| format!("not a WordBench document: {e}"))?;
    if nd.format != FORMAT {
        return Err("not a WordBench document".into());
    }
    let mut d = Document::new();
    d.page = PageSetup {
        width: nd.page.width,
        height: nd.page.height,
        margin_left: nd.page.margin_left,
        margin_right: nd.page.margin_right,
        margin_top: nd.page.margin_top,
        margin_bottom: nd.page.margin_bottom,
        page_numbers: PageNumPos::from_index(nd.page.page_numbers),
    };
    d.header = nd.header;
    d.footer = nd.footer;
    d.footnotes = nd.footnotes;
    for id in nd.endnotes {
        d.note_meta.insert(
            id,
            NoteMeta {
                endnote: true,
                orig_text: String::new(),
                xml: String::new(),
            },
        );
    }
    let blocks = nd
        .blocks
        .into_iter()
        .map(|b| match b {
            NBlock::Para(p) => Block::Para(para_in(p, &d)),
            NBlock::Table { cols, rows } => Block::Table(Table {
                cols,
                rows: rows
                    .into_iter()
                    .map(|r| r.into_iter().map(|p| para_in(p, &d)).collect())
                    .collect(),
                extra: None,
            }),
        })
        .collect();
    d.blocks = blocks;
    d.normalize();
    Ok(d)
}

pub fn save(doc: &Document, path: &Path) -> Result<(), String> {
    write_atomic(path, to_string(doc).as_bytes())
}

pub fn load(path: &Path) -> Result<Document, String> {
    from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
}
