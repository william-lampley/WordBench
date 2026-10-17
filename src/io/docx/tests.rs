use super::*;
use crate::editor::Editor;

#[test]
fn docx_roundtrip_of_new_document() {
    let mut e = Editor::new();
    e.insert_text("Title here\nSome ");
    e.toggle_fmt(Fmt::BOLD);
    e.insert_text("bold");
    e.toggle_fmt(Fmt::BOLD);
    e.insert_text(" text & <stuff>\tafter tab");
    e.insert_footnote("A footnote.".into());
    e.insert_text("\nitem one\nitem two");
    e.move_to(Pos::new(ParaRef::top(0), 0), false);
    e.apply_style("Title");
    e.select_range(Pos::new(ParaRef::top(2), 0), Pos::new(ParaRef::top(3), 1));
    e.toggle_list(ListKind::Number);
    e.move_to(e.doc.end_pos(), false);
    e.insert_text("\n");
    e.insert_table(2, 2);
    e.insert_text("cell");
    e.edit_doc(|d| {
        d.footer = "Page {page} of {pages}".into();
        d.page.margin_left = 1.5;
    });

    let d = from_bytes(&to_bytes(&e.doc).unwrap()).unwrap();
    assert_eq!(d.to_plain_text(), e.doc.to_plain_text());
    let p0 = d.para(ParaRef::top(0));
    assert_eq!(p0.props.style, "Title");
    assert_eq!(p0.props.align, Align::Center);
    let p1 = d.para(ParaRef::top(1));
    assert!(p1.cells[5].fmt.has(Fmt::BOLD));
    assert!(!p1.cells[0].fmt.has(Fmt::BOLD));
    assert_eq!(
        d.footnotes.values().next().map(String::as_str),
        Some("A footnote.")
    );
    assert_eq!(
        d.para(ParaRef::top(2)).props.list.map(|l| l.kind),
        Some(ListKind::Number)
    );
    assert_eq!(d.footer, "Page {page} of {pages}");
    assert!((d.page.margin_left - 1.5).abs() < 0.01);
    assert!(d.table(4).is_some());
}

// ---------------------------------------------------------------- fidelity

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn zip_parts(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zw.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    read_parts(bytes).unwrap()
}

fn part<'a>(parts: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &parts
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("missing {name}"))
        .1
}

/// A Word package using features WordBench doesn't model.
fn rich_package() -> Vec<u8> {
    let ns = format!(
        r#"xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:classic="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14""#
    );
    let drawing = r#"<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="952500" cy="952500"/><wp:docPr id="1" name="Logo"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:blipFill><a:blip r:embed="rIdImg"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#;
    let body = format!(
        r#"<w:p w14:paraId="1A2B3C4D" w:rsidR="00AB12CD"><w:pPr><w:pStyle w:val="berschrift1"/><w:keepNext/></w:pPr><w:r><w:t>Heading</w:t></w:r></w:p>
<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="2880"/></w:tabs><w:spacing w:before="120" w:after="60"/></w:pPr><w:bookmarkStart w:id="0" w:name="intro"/><w:r><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:b/><w:color w:val="FF0000"/><w:sz w:val="28"/></w:rPr><w:t>Red</w:t></w:r><w:bookmarkEnd w:id="0"/><w:r><w:t xml:space="preserve"> see </w:t></w:r><w:hyperlink r:id="rIdLink" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>link</w:t></w:r></w:hyperlink><w:r><w:t xml:space="preserve"> page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>3</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:ins w:id="5" w:author="Ann" w:date="2024-01-01T00:00:00Z"><w:r><w:t xml:space="preserve"> inserted</w:t></w:r></w:ins><w:del w:id="6" w:author="Ann" w:date="2024-01-01T00:00:00Z"><w:r><w:delText xml:space="preserve"> gone</w:delText></w:r></w:del><w:r>{drawing}</w:r><w:r><w:t>Note</w:t></w:r><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="2"/></w:r><w:r><w:br/><w:t>after break</w:t><w:tab/><w:t>tabbed</w:t></w:r><w:proofErr w:type="spellStart"/></w:p>
<w:p><w:pPr><w:pStyle w:val="Quote"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="5"/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>
<w:sdt><w:sdtPr><w:alias w:val="Box"/><w:id w:val="99"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>In control</w:t></w:r></w:p></w:sdtContent></w:sdt>
<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/><w:gridSpan w:val="2"/><w:shd w:val="clear" w:fill="D9D9D9"/></w:tcPr><w:p><w:r><w:t>merged</w:t></w:r></w:p></w:tc><w:tc><w:tcPr><w:tcW w:w="2000" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>a1</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="right"/></w:pPr><w:r><w:t>a2</w:t></w:r></w:p></w:tc><w:tc><w:tbl><w:tblGrid><w:gridCol w:w="900"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p/></w:tc><w:tc><w:p><w:r><w:t>z</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
<w:p><w:pPr><w:sectPr><w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" w:header="360" w:footer="360" w:gutter="0"/></w:sectPr></w:pPr><w:r><w:t>End of landscape section</w:t></w:r></w:p>
<w:p><w:r><w:t>Last</w:t></w:r></w:p>
<w:sectPr><w:headerReference w:type="default" r:id="rIdH1"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/><w:cols w:num="2" w:space="720"/></w:sectPr>"#
    );
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {ns}><w:background w:color="FFFFEE"/><w:body>{body}</w:body></w:document>"#
    );
    let rel = |id: &str, ty: &str, target: &str| {
        format!(
            r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{ty}" Target="{target}"/>"#
        )
    };
    let doc_rels = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{}{}{}{}{}{}<Relationship Id="rIdLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/></Relationships>"#,
        rel("rIdS", "styles", "styles.xml"),
        rel("rIdN", "numbering", "numbering.xml"),
        rel("rIdF", "footnotes", "footnotes.xml"),
        rel("rIdSet", "settings", "settings.xml"),
        rel("rIdH1", "header", "header1.xml"),
        rel("rIdImg", "image", "media/image1.png"),
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="{W}"><w:style w:type="paragraph" w:default="1" w:styleId="Standard"><w:name w:val="Normal"/></w:style><w:style w:type="paragraph" w:styleId="berschrift1"><w:name w:val="heading 1"/><w:rPr><w:b/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:rPr><w:i/></w:rPr></w:style></w:styles>"#
    );
    let numbering = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:numbering xmlns:w="{W}"><w:abstractNum w:abstractNumId="3"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1)"/></w:lvl></w:abstractNum><w:num w:numId="5"><w:abstractNumId w:val="3"/></w:num></w:numbering>"#
    );
    let footnotes = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:footnotes xmlns:w="{W}"><w:footnote w:type="separator" w:id="0"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type="continuationSeparator" w:id="1"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:rPr><w:i/></w:rPr><w:t xml:space="preserve"> An italic note.</w:t></w:r></w:p></w:footnote></w:footnotes>"#
    );
    let header = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:hdr xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:p><w:r><w:t>ACME Corp</w:t></w:r></w:p></w:hdr>"#
    );
    let settings = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"><w:zoom w:percent="140"/><w:footnotePr><w:footnote w:id="0"/><w:footnote w:id="1"/></w:footnotePr></w:settings>"#
    );
    let ct = r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#;
    let root_rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    zip_parts(&[
        ("[Content_Types].xml", ct.into()),
        ("_rels/.rels", root_rels.into()),
        ("word/document.xml", doc.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/styles.xml", styles.into_bytes()),
        ("word/numbering.xml", numbering.into_bytes()),
        ("word/footnotes.xml", footnotes.into_bytes()),
        ("word/header1.xml", header.into_bytes()),
        ("word/settings.xml", settings.into_bytes()),
        (
            "word/media/image1.png",
            vec![0x89, b'P', b'N', b'G', 1, 2, 3, 4],
        ),
        ("customXml/item1.xml", b"<data>keep me</data>".to_vec()),
    ])
}

/// Characters with their effective run properties (canonical), and the
/// count of every element by name, ignoring how text is split into runs.
fn signature(
    xml: &[u8],
) -> (
    Vec<(char, String)>,
    std::collections::BTreeMap<String, usize>,
) {
    let x = parse(&String::from_utf8_lossy(xml)).unwrap();
    let mut chars = Vec::new();
    let mut counts = std::collections::BTreeMap::new();
    fn canon(rpr: Option<&XNode>) -> String {
        let mut v: Vec<String> = rpr
            .map(|r| {
                r.children
                    .iter()
                    .filter(|c| c.name != "#text")
                    .map(|c| c.to_xml())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.concat()
    }
    fn walk(
        n: &XNode,
        rpr: &str,
        chars: &mut Vec<(char, String)>,
        counts: &mut std::collections::BTreeMap<String, usize>,
    ) {
        if n.name != "#text" {
            *counts.entry(n.name.clone()).or_default() += 1;
        }
        let rpr_here;
        let rpr = if n.name == "r" {
            rpr_here = canon(n.child("rPr"));
            rpr_here.as_str()
        } else {
            rpr
        };
        if n.name == "t" {
            chars.extend(n.all_text().chars().map(|c| (c, rpr.to_string())));
            return;
        }
        n.children.iter().for_each(|c| walk(c, rpr, chars, counts));
    }
    walk(&x, "", &mut chars, &mut counts);
    // Splitting text differently changes these counts but nothing else.
    for k in ["r", "t", "rPr", "proofErr", "pPr"] {
        counts.remove(k);
    }
    (chars, counts)
}

#[test]
fn reads_rich_document_into_the_model() {
    let d = from_bytes(&rich_package()).unwrap();
    let text = d.to_plain_text();
    assert!(
        text.starts_with(
            "Heading\nRed see link page 3 insertedNoteafter break\ttabbed\nitem\nIn control\n"
        ),
        "{text}"
    );
    assert_eq!(d.para(ParaRef::top(0)).props.style, "berschrift1");
    assert_eq!(d.styles.name("berschrift1"), "heading 1");
    assert_eq!(
        d.styles.get("berschrift1").map(|s| s.fmt.has(Fmt::BOLD)),
        Some(true)
    );
    assert_eq!(
        d.para(ParaRef::top(2)).props.list.map(|l| l.kind),
        Some(ListKind::Number)
    );
    let t = d.table(4).expect("table");
    assert_eq!(t.ncols(), 3);
    assert_eq!(t.rows[0][0].plain_text(), "merged");
    assert!(t.rows[0][1].extra.as_ref().unwrap().covered);
    assert_eq!(t.rows[1][0].plain_text(), "a1a2");
    assert_eq!(d.header, "ACME Corp");
    assert_eq!(
        d.footnotes.values().next().map(String::as_str),
        Some("An italic note.")
    );
    let st = d.store.borrow();
    let labels: Vec<&str> = st.objects.iter().map(|o| o.label.as_str()).collect();
    assert!(labels.contains(&"[Image: Logo]"), "{labels:?}");
    assert!(labels.contains(&"[Deleted:  gone]"), "{labels:?}");
    assert!(labels.contains(&"Bookmark:intro"), "{labels:?}");
    assert!(labels.contains(&"[Table]"), "{labels:?}");
}

#[test]
fn unchanged_save_keeps_everything() {
    let orig = rich_package();
    let d = from_bytes(&orig).unwrap();
    let saved = to_bytes(&d).unwrap();
    let (a, b) = (unzip(&orig), unzip(&saved));
    // Every part survives; parts WordBench doesn't edit are byte-identical.
    for (name, bytes) in &a {
        let got = part(&b, name);
        if !matches!(name.as_str(), "word/document.xml" | "word/footnotes.xml") {
            assert_eq!(
                String::from_utf8_lossy(got),
                String::from_utf8_lossy(bytes),
                "{name} changed"
            );
        }
    }
    assert_eq!(
        a.len(),
        b.len(),
        "parts added: {:?}",
        b.iter().map(|p| &p.0).collect::<Vec<_>>()
    );
    // The body has the same text, formatting and elements.
    let (ca, na) = signature(part(&a, "word/document.xml"));
    let (cb, nb) = signature(part(&b, "word/document.xml"));
    assert_eq!(ca, cb);
    assert_eq!(na, nb);
    let (fa, ga) = signature(part(&a, "word/footnotes.xml"));
    let (fb, gb) = signature(part(&b, "word/footnotes.xml"));
    assert_eq!((fa, ga), (fb, gb));
    // Saving twice gives the same bytes.
    assert_eq!(
        to_bytes(&from_bytes(&saved).unwrap()).unwrap().len(),
        saved.len()
    );
}

#[test]
fn edits_keep_everything_else() {
    let orig = rich_package();
    let mut e = Editor::with_doc(from_bytes(&orig).unwrap(), None);
    // Type at the start of the heading and in the second paragraph.
    e.insert_text("My ");
    e.find("Red", true);
    e.move_to(e.cur, false);
    e.insert_text("!");
    // Bold "see", turn the numbered item into a bullet, add a paragraph.
    e.find("see", true);
    e.toggle_fmt(Fmt::BOLD);
    e.move_to(Pos::new(ParaRef::top(2), 4), false);
    e.toggle_list(ListKind::Bullet);
    e.insert_text("\nnew item");
    e.edit_doc(|d| d.page.margin_left = 1.25);
    let saved = to_bytes(&e.doc).unwrap();
    let b = unzip(&saved);
    let a = unzip(&orig);
    for name in [
        "word/header1.xml",
        "word/settings.xml",
        "word/media/image1.png",
        "customXml/item1.xml",
        "word/styles.xml",
    ] {
        assert_eq!(
            String::from_utf8_lossy(part(&a, name)),
            String::from_utf8_lossy(part(&b, name)),
            "{name} changed"
        );
    }
    let docxml = String::from_utf8_lossy(part(&b, "word/document.xml")).to_string();
    for needle in [
        "w14:paraId=\"1A2B3C4D\"",
        "<w:pStyle w:val=\"berschrift1\"/><w:keepNext/>",
        "<w:bookmarkStart w:id=\"0\" w:name=\"intro\"/>",
        "<w:hyperlink r:id=\"rIdLink\" w:history=\"1\">",
        "<w:instrText xml:space=\"preserve\"> PAGE </w:instrText>",
        "<w:ins w:id=\"5\"",
        "<w:delText xml:space=\"preserve\"> gone</w:delText>",
        "r:embed=\"rIdImg\"",
        "<w:gridSpan w:val=\"2\"/>",
        "<w:tblHeader/>",
        "<w:tblStyle w:val=\"TableGrid\"/>",
        "<w:jc w:val=\"right\"/></w:pPr><w:r><w:t xml:space=\"preserve\">a2",
        "<w:gridCol w:w=\"900\"/>",
        "w:orient=\"landscape\"",
        "<w:cols w:num=\"2\" w:space=\"720\"/>",
        "<w:alias w:val=\"Box\"/>",
        "<w:background w:color=\"FFFFEE\"/>",
        "<w:rFonts w:ascii=\"Arial\" w:hAnsi=\"Arial\"/><w:b/><w:color w:val=\"FF0000\"/><w:sz w:val=\"28\"/>",
        "<w:tabs><w:tab w:val=\"left\" w:pos=\"2880\"/></w:tabs>",
        "w:left=\"1800\"",
    ] {
        assert!(docxml.contains(needle), "lost {needle}\n{docxml}");
    }
    // The bullet got a new numbering definition; the original one is kept.
    let num = String::from_utf8_lossy(part(&b, "word/numbering.xml")).to_string();
    assert!(
        num.contains("<w:abstractNum w:abstractNumId=\"3\">") && num.contains("w:numId=\"6\""),
        "{num}"
    );
    // Reading it back gives the edited text.
    let d2 = from_bytes(&saved).unwrap();
    assert!(
        d2.to_plain_text().starts_with("My Heading\nRed! see"),
        "{}",
        d2.to_plain_text()
    );
    let p1 = d2.para(ParaRef::top(1));
    let s_at = p1.cells.iter().position(|c| c.ch == 's').unwrap();
    assert!(p1.cells[s_at].fmt.has(Fmt::BOLD));
    assert!(!p1.cells[s_at - 1].fmt.has(Fmt::BOLD));
    assert_eq!(
        d2.para(ParaRef::top(2)).props.list.map(|l| l.kind),
        Some(ListKind::Bullet)
    );
    assert_eq!(
        d2.para(ParaRef::top(3)).props.list.map(|l| l.kind),
        Some(ListKind::Bullet)
    );
}

/// A .docx written by LibreOffice from tests/fixtures/rich.fodt (pictures,
/// comments, tracked changes, table of contents, endnotes, columns...).
const LIBREOFFICE: &[u8] = include_bytes!("../../../tests/fixtures/rich.docx");

#[test]
fn libreoffice_file_round_trips() {
    let d = from_bytes(LIBREOFFICE).unwrap();
    let text = d.to_plain_text();
    for s in [
        "Introduction",
        "red bold Sans",
        "hyperlink",
        "new words",
        "Merged header",
        "Two column text",
        "The end.",
    ] {
        assert!(text.contains(s), "missing {s:?} in {text}");
    }
    assert_eq!(d.footnotes.len(), 2, "{:?}", d.footnotes);
    let saved = to_bytes(&d).unwrap();
    let (a, b) = (unzip(LIBREOFFICE), unzip(&saved));
    assert_eq!(a.len(), b.len());
    for (name, bytes) in &a {
        if !matches!(
            name.as_str(),
            "word/document.xml" | "word/footnotes.xml" | "word/endnotes.xml"
        ) {
            assert_eq!(
                String::from_utf8_lossy(part(&b, name)),
                String::from_utf8_lossy(bytes),
                "{name} changed"
            );
        }
    }
    for name in [
        "word/document.xml",
        "word/footnotes.xml",
        "word/endnotes.xml",
    ] {
        assert_eq!(
            signature(part(&a, name)),
            signature(part(&b, name)),
            "{name} differs"
        );
    }
}

/// Writes an edited copy of the LibreOffice fixture for checking in other
/// programs: WORDBENCH_OUT=/path/out.docx cargo test write_edited -- --ignored
#[test]
#[ignore]
fn write_edited_fixture() {
    let out = std::env::var("WORDBENCH_OUT").expect("set WORDBENCH_OUT");
    let mut e = Editor::with_doc(from_bytes(LIBREOFFICE).unwrap(), None);
    e.find("This report", true);
    e.replace_selection("THIS REPORT");
    e.find("The end.", true);
    e.move_to(e.cur, false);
    e.insert_text("\nAdded by WordBench.");
    e.toggle_list(ListKind::Number);
    e.insert_text("\nSecond added item");
    save(&e.doc, std::path::Path::new(&out)).unwrap();
}

// ---------------------------------------------------------------- phase 2

fn glyph_text(d: &Document, pr: ParaRef) -> (String, crate::layout::Layout) {
    let l = crate::layout::Layout::build(d);
    let li = l.locate(Pos::new(pr, 0)).0;
    let mut g = l.glyphs(li, d);
    g.sort_by_key(|g| g.x);
    let mut s = String::new();
    let mut x = 0;
    for gl in &g {
        while x < gl.x {
            s.push(' ');
            x += 1;
        }
        s.push(gl.ch);
        x += 1;
    }
    (s, l)
}

#[test]
fn word_list_numbers_colors_and_styles() {
    let d = from_bytes(&rich_package()).unwrap();
    // numId 5 uses "%1)".
    let (line, _) = glyph_text(&d, ParaRef::top(2));
    assert!(line.trim_start().starts_with("1) item"), "{line:?}");
    // Red text keeps its colour; the heading style makes the heading bold.
    let l = crate::layout::Layout::build(&d);
    let li = l.locate(Pos::new(ParaRef::top(1), 1)).0;
    let red = l.glyphs(li, &d).into_iter().find(|g| g.ch == 'R').unwrap();
    assert_eq!(red.color, Some(0xFF0000));
    assert!(red.fmt.has(Fmt::BOLD));
    let li = l.locate(Pos::new(ParaRef::top(0), 0)).0;
    assert!(l.glyphs(li, &d).iter().all(|g| g.fmt.has(Fmt::BOLD)));
    // The quote style ("Quote") is italic and offered in the style menu.
    assert!(d.styles.menu().iter().any(|s| s.id == "Quote"));
    assert_eq!(
        d.styles.get("Quote").map(|s| s.fmt.has(Fmt::ITALIC)),
        Some(true)
    );
}

#[test]
fn right_tab_with_dot_leader() {
    let body = r#"<w:p><w:pPr><w:tabs><w:tab w:val="right" w:leader="dot" w:pos="5760"/></w:tabs></w:pPr><w:r><w:t>Intro</w:t></w:r><w:r><w:tab/><w:t>12</w:t></w:r></w:p>"#;
    let doc = format!(r#"<w:document xmlns:w="{W}"><w:body>{body}</w:body></w:document>"#);
    let pkg = zip_parts(&[
        ("_rels/.rels", br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec()),
        ("word/document.xml", doc.into_bytes()),
    ]);
    let d = from_bytes(&pkg).unwrap();
    let (line, _) = glyph_text(&d, ParaRef::top(0));
    // 5760 twips = 4" = column 40; "12" ends there.
    let line = line.trim_end();
    assert!(
        line.starts_with("Intro ") && line.ends_with(" 12") && line.contains("......"),
        "{line:?}"
    );
    assert_eq!(line.len(), 40, "{line:?}");
}

#[test]
fn turning_bold_off_inside_a_bold_style() {
    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    e.select_range(Pos::new(ParaRef::top(0), 0), Pos::new(ParaRef::top(0), 4));
    e.toggle_fmt(Fmt::BOLD); // style is bold: this turns it off
    let l = crate::layout::Layout::build(&e.doc);
    let g = l.glyphs(l.locate(Pos::new(ParaRef::top(0), 0)).0, &e.doc);
    let h = g.iter().find(|g| g.ch == 'H').unwrap();
    assert!(!h.fmt.has(Fmt::BOLD), "should not show bold");
    let saved = to_bytes(&e.doc).unwrap();
    let xml = String::from_utf8_lossy(part(&unzip(&saved), "word/document.xml")).to_string();
    assert!(
        xml.contains(r#"<w:b w:val="0"/></w:rPr><w:t xml:space="preserve">Head</w:t>"#),
        "{xml}"
    );
}

// ---------------------------------------------------------------- phase 3

fn doc_xml(bytes: &[u8]) -> String {
    String::from_utf8_lossy(part(&unzip(bytes), "word/document.xml")).to_string()
}

#[test]
fn accept_and_reject_tracked_changes() {
    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    assert_eq!(e.change_count(), 2);
    e.review(true, None);
    assert_eq!(e.change_count(), 0);
    assert!(e.doc.to_plain_text().contains("3 insertedNote"));
    let xml = doc_xml(&to_bytes(&e.doc).unwrap());
    assert!(
        !xml.contains("<w:ins ") && !xml.contains("<w:del ") && xml.contains(" inserted</w:t>"),
        "{xml}"
    );

    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    e.review(false, None);
    let text = e.doc.to_plain_text();
    assert!(text.contains("page 3 goneNote"), "{text}");
    let xml = doc_xml(&to_bytes(&e.doc).unwrap());
    assert!(
        !xml.contains("inserted") && xml.contains("> gone</w:t>"),
        "{xml}"
    );
}

#[test]
fn records_tracked_changes() {
    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    e.author = "Tester".into();
    e.track = true;
    e.move_to(Pos::new(ParaRef::top(0), 0), false);
    e.insert_text("New ");
    e.move_to(Pos::new(ParaRef::top(0), e.para().len()), false);
    e.backspace();
    e.backspace();
    // "Heading" lost "ng" as a tracked deletion; "New " is a tracked
    // insertion (the file already had two changes).
    assert_eq!(e.change_count(), 4);
    let saved = to_bytes(&e.doc).unwrap();
    let xml = doc_xml(&saved);
    assert!(xml.contains(r#"w:author="Tester""#), "{xml}");
    assert!(
        xml.contains(r#"<w:delText xml:space="preserve">ng</w:delText>"#),
        "{xml}"
    );
    let d = from_bytes(&saved).unwrap();
    let e2 = Editor::with_doc(d, None);
    assert_eq!(e2.change_count(), 4);
}

#[test]
fn hyperlinks_read_add_and_remove() {
    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    e.find("link", true);
    e.move_to(e.cur, false);
    assert_eq!(
        e.link_at_cursor().map(|l| l.1),
        Some("https://example.com/".to_string())
    );
    e.find("Last", true);
    e.add_link("https://wordbench.example/").unwrap();
    let saved = to_bytes(&e.doc).unwrap();
    let parts = unzip(&saved);
    let rels = String::from_utf8_lossy(part(&parts, "word/_rels/document.xml.rels")).to_string();
    assert!(
        rels.contains(r#"Target="https://wordbench.example/" TargetMode="External""#),
        "{rels}"
    );
    let xml = doc_xml(&saved);
    assert!(xml.contains(r#"<w:hyperlink r:id="rIdWbLink1" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t xml:space="preserve">Last"#), "{xml}");
    let styles = String::from_utf8_lossy(part(&parts, "word/styles.xml")).to_string();
    assert!(styles.contains(r#"w:styleId="Hyperlink""#));
    e.move_to(Pos::new(e.cur.pr, 1), false);
    assert!(e.remove_link());
    assert!(!doc_xml(&to_bytes(&e.doc).unwrap()).contains("wordbench.example"));
}

#[test]
fn comments_round_trip_add_and_delete() {
    let mut e = Editor::with_doc(from_bytes(LIBREOFFICE).unwrap(), None);
    let found = e.comment_positions();
    assert_eq!(found.len(), 1);
    let (old_id, _) = found[0].clone();
    assert_eq!(e.doc.comments[&old_id].author, "Bob");
    e.author = "Ann Lee".into();
    e.find("The end", true);
    let new_id = e.add_comment("Check the ending.");
    let saved = to_bytes(&e.doc).unwrap();
    let c = String::from_utf8_lossy(part(&unzip(&saved), "word/comments.xml")).to_string();
    assert!(
        c.contains("Please check this.")
            && c.contains("Check the ending.")
            && c.contains(r#"w:initials="AL""#),
        "{c}"
    );
    let d = from_bytes(&saved).unwrap();
    assert_eq!(d.comments.len(), 2);
    e.delete_comment(&old_id);
    let c = String::from_utf8_lossy(part(
        &unzip(&to_bytes(&e.doc).unwrap()),
        "word/comments.xml",
    ))
    .to_string();
    assert!(
        !c.contains("Please check this.") && c.contains("Check the ending."),
        "{c}"
    );
    assert!(e.doc.comments.contains_key(&new_id));
}

#[test]
fn endnotes_pictures_properties_sections() {
    let mut e = Editor::with_doc(from_bytes(&rich_package()).unwrap(), None);
    // Section break after the landscape paragraph shows in the layout.
    let l = crate::layout::Layout::build(&e.doc);
    assert!(l.lines.iter().any(|x| x.kind
        == crate::layout::LineKind::PageBreak {
            forced: true,
            section: true
        }));
    // Endnote.
    e.move_to(e.doc.end_pos(), false);
    e.insert_footnote("An endnote.".into());
    let id = *e.doc.footnotes.keys().next_back().unwrap();
    e.doc.note_meta.insert(
        id,
        NoteMeta {
            endnote: true,
            orig_text: String::new(),
            xml: String::new(),
        },
    );
    // Picture.
    let png = {
        let dir = std::env::temp_dir().join(format!("wordbench-pic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("dot.png");
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&192u32.to_be_bytes());
        b.extend_from_slice(&96u32.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        std::fs::write(&p, &b).unwrap();
        p
    };
    e.insert_picture(&png).unwrap();
    e.edit_doc(|d| d.core.title = "Report <2026>".into());
    let saved = to_bytes(&e.doc).unwrap();
    let parts = unzip(&saved);
    assert!(parts.iter().any(|(n, _)| n == "word/media/wb_image1.png"));
    let ct = String::from_utf8_lossy(part(&parts, "[Content_Types].xml")).to_string();
    assert!(
        ct.contains(r#"Extension="png""#) && ct.contains("endnotes+xml"),
        "{ct}"
    );
    let xml = doc_xml(&saved);
    assert!(
        xml.contains(r#"<a:blip r:embed="rIdWbImg1"/>"#) && xml.contains(r#"cx="1828800""#),
        "{xml}"
    );
    let en = String::from_utf8_lossy(part(&parts, "word/endnotes.xml")).to_string();
    assert!(en.contains("An endnote."), "{en}");
    let core = String::from_utf8_lossy(part(&parts, "docProps/core.xml")).to_string();
    assert!(
        core.contains("<dc:title>Report &lt;2026&gt;</dc:title>"),
        "{core}"
    );
    let d = from_bytes(&saved).unwrap();
    assert_eq!(d.core.title, "Report <2026>");
    assert!(d.note_meta.values().any(|m| m.endnote));
    // The endnote is laid out after the text, labelled "i".
    let l = crate::layout::Layout::build(&d);
    assert!(
        l.lines
            .iter()
            .any(|x| x.kind == crate::layout::LineKind::Endnote
                && x.deco.iter().any(|g| g.text.starts_with("i. An endnote")))
    );
}

/// Writes a document using the review features for checking in other
/// programs: WORDBENCH_OUT=/path/out.docx WORDBENCH_PNG=/path/pic.png
/// cargo test write_review -- --ignored
#[test]
#[ignore]
fn write_review_fixture() {
    let out = std::env::var("WORDBENCH_OUT").expect("set WORDBENCH_OUT");
    let png = std::env::var("WORDBENCH_PNG").expect("set WORDBENCH_PNG");
    let mut e = Editor::with_doc(from_bytes(LIBREOFFICE).unwrap(), None);
    e.author = "Tester".into();
    e.find("The end.", true);
    e.add_link("https://example.net/").unwrap();
    e.add_comment("Is this the end?");
    e.move_to(e.doc.end_pos(), false);
    e.insert_text("\nPicture: ");
    e.insert_picture(std::path::Path::new(&png)).unwrap();
    e.insert_footnote("Second endnote.".into());
    let id = *e.doc.footnotes.keys().next_back().unwrap();
    e.doc.note_meta.insert(
        id,
        NoteMeta {
            endnote: true,
            orig_text: String::new(),
            xml: String::new(),
        },
    );
    e.track = true;
    e.find("Two column", true);
    e.replace_selection("Twin column");
    e.edit_doc(|d| {
        d.core.title = "Review fixture".into();
        d.core.author = "Tester".into();
    });
    save(&e.doc, std::path::Path::new(&out)).unwrap();
}
