//! Paragraph and character styles, and list numbering definitions.
//!
//! As in Word (and WordCraft), a style is based on another; the effective
//! formatting of a paragraph is the document defaults, then each style in
//! the `basedOn` chain, then the paragraph's own (direct) formatting.
//! Styles keep their original XML so files are written back unchanged.

use crate::model::*;
use crate::props;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct StyleDef {
    pub id: String,
    pub name: String,
    /// Paragraph style (otherwise a character style).
    pub para: bool,
    pub hidden: bool,
    /// Resolved paragraph properties (inheritance applied).
    pub props: ParaProps,
    /// Resolved character formatting and look (`Store` entry).
    pub fmt: Fmt,
    pub cp: u32,
    pub next: Option<String>,
    /// Outline level (0 = Heading 1).
    pub outline: Option<u8>,
    /// Definition to add to styles.xml if the file lacks it.
    pub xml: Option<String>,
    /// Defined in the document's own styles part.
    pub in_file: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct StyleSheet {
    pub styles: Vec<StyleDef>,
    pub default_para: String,
    /// Look of the document defaults (`w:docDefaults`).
    pub base_cp: u32,
}

/// A style as read, before inheritance is resolved.
pub struct RawStyle {
    pub id: String,
    pub name: String,
    pub para: bool,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub hidden: bool,
    pub default: bool,
    pub ppr: Vec<(String, String)>,
    pub rpr: Vec<(String, String)>,
    pub xml: String,
}

/// Merge run property items: later ones replace earlier ones by name.
fn merge(into: &mut Vec<(String, String)>, items: &[(String, String)]) {
    for (n, x) in items {
        props::set_item(into, n, Some(x.clone()));
    }
}

impl StyleSheet {
    pub fn get(&self, id: &str) -> Option<&StyleDef> {
        let id = if id.is_empty() {
            self.default_para.as_str()
        } else {
            id
        };
        self.styles.iter().find(|s| s.id == id)
    }

    pub fn name(&self, id: &str) -> String {
        self.get(id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    pub fn is_default(&self, id: &str) -> bool {
        id.is_empty() || id == self.default_para
    }

    /// Paragraph styles to offer in menus.
    pub fn menu(&self) -> Vec<&StyleDef> {
        let mut v: Vec<&StyleDef> = self.styles.iter().filter(|s| s.para && !s.hidden).collect();
        v.sort_by_key(|s| {
            (
                !self.is_default(&s.id),
                s.outline.unwrap_or(9),
                s.name.to_lowercase(),
            )
        });
        v
    }

    /// Style id by name (case-insensitive) or id.
    pub fn find(&self, name: &str) -> Option<&StyleDef> {
        self.styles
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name) || s.id.eq_ignore_ascii_case(name))
    }

    /// Resolve inheritance. `in_file` marks the styles as part of the file.
    pub fn build(
        raw: Vec<RawStyle>,
        doc_ppr: &[(String, String)],
        doc_rpr: &[(String, String)],
        numbering: &Numbering,
        store: &mut Store,
        in_file: bool,
    ) -> StyleSheet {
        let by_id: HashMap<&str, &RawStyle> = raw.iter().map(|r| (r.id.as_str(), r)).collect();
        let base_cp = store.intern(CharProps {
            rpr: doc_rpr.to_vec(),
            wraps: Vec::new(),
        });
        let mut styles = Vec::new();
        for r in &raw {
            let mut chain = vec![r];
            let mut cur = r;
            while let Some(b) = cur.based_on.as_deref().and_then(|b| by_id.get(b)) {
                if chain.len() > 20 || chain.iter().any(|c| c.id == b.id) {
                    break;
                }
                chain.push(b);
                cur = b;
            }
            chain.reverse();
            let mut p = ParaProps::default();
            let mut rpr = doc_rpr.to_vec();
            if r.para {
                props::apply_ppr(&mut p, doc_ppr, numbering);
            }
            let mut outline = None;
            for s in &chain {
                if r.para {
                    props::apply_ppr(&mut p, &s.ppr, numbering);
                    if let Some(o) = props::outline_level(&s.ppr) {
                        outline = Some(o);
                    }
                }
                merge(&mut rpr, &s.rpr);
            }
            p.style = r.id.clone();
            let name_l = r.name.to_lowercase();
            if outline.is_none() {
                outline = name_l
                    .strip_prefix("heading ")
                    .and_then(|n| n.trim().parse::<u8>().ok())
                    .map(|n| n.saturating_sub(1));
            }
            let fmt = props::fmt_from_rpr(&rpr);
            let cp = store.intern(CharProps {
                rpr,
                wraps: Vec::new(),
            });
            styles.push(StyleDef {
                id: r.id.clone(),
                name: r.name.clone(),
                para: r.para,
                hidden: r.hidden,
                props: p,
                fmt,
                cp,
                next: r.next.clone(),
                outline,
                xml: Some(r.xml.clone()),
                in_file,
            });
        }
        let default_para = raw
            .iter()
            .find(|r| r.para && r.default)
            .or_else(|| raw.iter().find(|r| r.para && r.id == "Normal"))
            .or_else(|| raw.iter().find(|r| r.para))
            .map(|r| r.id.clone())
            .unwrap_or_default();
        StyleSheet {
            styles,
            default_para,
            base_cp,
        }
    }

    /// WordBench's own styles, for new documents (and to add to Word files
    /// that lack them).
    pub fn builtin(store: &mut Store) -> StyleSheet {
        let raw = builtin_raw();
        let doc_rpr = vec![
            (
                "rFonts".to_string(),
                r#"<w:rFonts w:ascii="Courier New" w:hAnsi="Courier New" w:cs="Courier New" w:eastAsia="Courier New"/>"#.to_string(),
            ),
            ("sz".to_string(), r#"<w:sz w:val="24"/>"#.to_string()),
        ];
        let mut sheet = StyleSheet::build(raw, &[], &doc_rpr, &Numbering::default(), store, false);
        sheet.default_para = "Normal".into();
        sheet
    }

    /// Add WordBench's built-in styles that a file's sheet lacks (by id), so
    /// they can be chosen; they're written to the file only if used.
    pub fn add_missing_builtins(&mut self, store: &mut Store) {
        let builtin = StyleSheet::builtin(store);
        for s in builtin.styles {
            if self.get(&s.id).is_none() && self.find(&s.name).is_none() {
                self.styles.push(s);
            }
        }
    }
}

fn builtin_raw() -> Vec<RawStyle> {
    // (id, name, paragraph?, hidden, pPr children, rPr children)
    let defs: [(&str, &str, bool, bool, &str, &str); 13] = [
        ("Normal", "Normal", true, false, "", ""),
        (
            "Title",
            "Title",
            true,
            false,
            r#"<w:spacing w:after="240"/><w:jc w:val="center"/>"#,
            r#"<w:b/><w:u w:val="single"/>"#,
        ),
        (
            "Subtitle",
            "Subtitle",
            true,
            false,
            r#"<w:spacing w:after="240"/><w:jc w:val="center"/>"#,
            "<w:i/>",
        ),
        (
            "Heading1",
            "heading 1",
            true,
            false,
            r#"<w:keepNext/><w:spacing w:before="240"/><w:outlineLvl w:val="0"/>"#,
            r#"<w:b/><w:u w:val="single"/>"#,
        ),
        (
            "Heading2",
            "heading 2",
            true,
            false,
            r#"<w:keepNext/><w:spacing w:before="240"/><w:outlineLvl w:val="1"/>"#,
            "<w:b/>",
        ),
        (
            "Heading3",
            "heading 3",
            true,
            false,
            r#"<w:keepNext/><w:spacing w:before="240"/><w:outlineLvl w:val="2"/>"#,
            "<w:b/><w:i/>",
        ),
        (
            "Quote",
            "Quote",
            true,
            false,
            r#"<w:ind w:left="720" w:right="720"/>"#,
            "<w:i/>",
        ),
        (
            "Caption",
            "caption",
            true,
            false,
            r#"<w:spacing w:after="200"/>"#,
            r#"<w:i/><w:color w:val="44546A"/><w:sz w:val="18"/><w:szCs w:val="18"/>"#,
        ),
        (
            "FootnoteText",
            "footnote text",
            true,
            true,
            "",
            r#"<w:sz w:val="20"/>"#,
        ),
        (
            "FootnoteReference",
            "footnote reference",
            false,
            true,
            "",
            r#"<w:vertAlign w:val="superscript"/>"#,
        ),
        (
            "Hyperlink",
            "Hyperlink",
            false,
            false,
            "",
            r#"<w:color w:val="0563C1"/><w:u w:val="single"/>"#,
        ),
        ("Strong", "Strong", false, false, "", "<w:b/>"),
        ("Emphasis", "Emphasis", false, false, "", "<w:i/>"),
    ];
    defs.iter()
        .map(|&(id, name, para, hidden, ppr, rpr)| {
            let ty = if para { "paragraph" } else { "character" };
            let default = id == "Normal";
            let mut xml = format!(
                r#"<w:style w:type="{ty}" w:styleId="{id}"{}><w:name w:val="{name}"/>"#,
                if default { r#" w:default="1""# } else { "" }
            );
            if !default {
                xml.push_str(if para {
                    r#"<w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#
                } else {
                    ""
                });
            }
            if hidden {
                xml.push_str("<w:semiHidden/>");
            } else {
                xml.push_str("<w:qFormat/>");
            }
            if !ppr.is_empty() {
                xml.push_str(&format!("<w:pPr>{ppr}</w:pPr>"));
            }
            if !rpr.is_empty() {
                xml.push_str(&format!("<w:rPr>{rpr}</w:rPr>"));
            }
            xml.push_str("</w:style>");
            RawStyle {
                id: id.into(),
                name: name.into(),
                para,
                based_on: (!default && para).then(|| "Normal".to_string()),
                next: para.then(|| "Normal".to_string()),
                hidden,
                default,
                ppr: child_items(ppr),
                rpr: child_items(rpr),
                xml,
            }
        })
        .collect()
}

/// Parse a run of sibling elements into (name, xml) items.
pub fn child_items(xml: &str) -> Vec<(String, String)> {
    let wrapped = format!(
        r#"<x xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{xml}</x>"#
    );
    crate::io::xml::parse(&wrapped)
        .ok()
        .and_then(|x| x.children.into_iter().find(|c| c.name == "x"))
        .map(|x| {
            x.children
                .iter()
                .filter(|c| c.name != "#text")
                .map(|c| (c.name.clone(), c.to_xml()))
                .collect()
        })
        .unwrap_or_default()
}

/// One level of a list definition.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelDef {
    /// `w:numFmt` value: decimal, lowerLetter, bullet...
    pub fmt: String,
    /// `w:lvlText`, e.g. "%1." or a bullet character.
    pub text: String,
    pub start: u32,
    /// Indent of the text and hanging indent of the number, in columns.
    pub left: Option<u16>,
    pub hanging: Option<u16>,
}

/// Word list definitions: numbering instances (`w:num`) pointing at
/// abstract definitions with up to nine levels.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Numbering {
    pub nums: HashMap<u32, (u32, [Option<u32>; 9])>,
    pub abstracts: HashMap<u32, Vec<LevelDef>>,
}

impl Numbering {
    pub fn level(&self, num: u32, lvl: u8) -> Option<&LevelDef> {
        let (abs, _) = self.nums.get(&num)?;
        self.abstracts.get(abs)?.get(lvl as usize)
    }

    pub fn kind(&self, num: u32, lvl: u8) -> ListKind {
        match self.level(num, lvl) {
            Some(l) if l.fmt != "bullet" && l.fmt != "none" => ListKind::Number,
            Some(_) => ListKind::Bullet,
            None => ListKind::Bullet,
        }
    }

    pub fn start(&self, num: u32, lvl: u8) -> u32 {
        self.nums
            .get(&num)
            .and_then(|(_, o)| o.get(lvl as usize).copied().flatten())
            .or_else(|| self.level(num, lvl).map(|l| l.start))
            .unwrap_or(1)
    }

    /// The marker for an item at `lvl` given the current counters.
    pub fn marker(&self, num: u32, lvl: u8, counters: &[u32; 9]) -> Option<String> {
        let def = self.level(num, lvl)?;
        if def.fmt == "bullet" {
            return Some(props::bullet_char(&def.text));
        }
        let mut s = def.text.clone();
        for k in (0..9u8).rev() {
            let pat = format!("%{}", k + 1);
            if s.contains(&pat) {
                let fmt = self
                    .level(num, k)
                    .map(|l| l.fmt.as_str())
                    .unwrap_or("decimal");
                let n = if counters[k as usize] == 0 {
                    self.start(num, k)
                } else {
                    counters[k as usize]
                };
                s = s.replace(&pat, &props::format_num(fmt, n));
            }
        }
        Some(s)
    }
}
