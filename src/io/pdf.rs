//! PDF export. Pages are drawn on the same 10 cpi / 6 lpi grid as the
//! screen using the built-in Courier fonts at 12pt, so the printed page
//! matches the screen line for line. Table borders are drawn as vector lines
//! and pictures are embedded as images in the space their placeholder takes.

use crate::layout::{GKind, Glyph, Layout, PicBox};
use crate::model::*;
use crate::pictures;
use crate::print::compose;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

const PT_PER_COL: f32 = 7.2; // 72pt / 10 cpi
const PT_PER_ROW: f32 = 12.0; // 72pt / 6 lpi
const FONT_SIZE: f32 = 12.0;

/// Map a character to WinAnsiEncoding (used by the standard PDF fonts).
fn win_ansi(c: char) -> u8 {
    match c {
        ' '..='~' => c as u8,
        '\u{a0}'..='\u{ff}' => c as u32 as u8,
        '€' => 0x80,
        '‚' => 0x82,
        'ƒ' => 0x83,
        '„' => 0x84,
        '…' => 0x85,
        '†' => 0x86,
        '‡' => 0x87,
        'ˆ' => 0x88,
        '‰' => 0x89,
        'Š' => 0x8a,
        '‹' => 0x8b,
        'Œ' => 0x8c,
        'Ž' => 0x8e,
        '‘' => 0x91,
        '’' => 0x92,
        '“' => 0x93,
        '”' => 0x94,
        '•' | '▪' => 0x95,
        '–' => 0x96,
        '—' => 0x97,
        '˜' => 0x98,
        '™' => 0x99,
        'š' => 0x9a,
        '›' => 0x9b,
        'œ' => 0x9c,
        'ž' => 0x9e,
        'Ÿ' => 0x9f,
        '◦' => b'o',
        '─' | '━' => b'-',
        _ => b'?',
    }
}

fn pdf_string(bytes: &[u8]) -> String {
    let mut s = String::from("(");
    for &b in bytes {
        match b {
            b'(' | b')' | b'\\' => {
                s.push('\\');
                s.push(b as char);
            }
            0x20..=0x7e => s.push(b as char),
            _ => s.push_str(&format!("\\{b:03o}")),
        }
    }
    s.push(')');
    s
}

/// Line segments (in cell-relative units, 0..1 with y down) for box-drawing
/// characters.
fn box_segments(c: char) -> Option<&'static [(f32, f32, f32, f32)]> {
    const H: (f32, f32, f32, f32) = (0.0, 0.5, 1.0, 0.5);
    const V: (f32, f32, f32, f32) = (0.5, 0.0, 0.5, 1.0);
    const L: (f32, f32, f32, f32) = (0.0, 0.5, 0.5, 0.5);
    const R: (f32, f32, f32, f32) = (0.5, 0.5, 1.0, 0.5);
    const U: (f32, f32, f32, f32) = (0.5, 0.0, 0.5, 0.5);
    const D: (f32, f32, f32, f32) = (0.5, 0.5, 0.5, 1.0);
    Some(match c {
        '─' => &[H],
        '│' => &[V],
        '┌' => &[R, D],
        '┐' => &[L, D],
        '└' => &[R, U],
        '┘' => &[L, U],
        '├' => &[V, R],
        '┤' => &[V, L],
        '┬' => &[H, D],
        '┴' => &[H, U],
        '┼' => &[H, V],
        _ => return None,
    })
}

fn font_for(f: Fmt) -> &'static str {
    match (f.has(Fmt::BOLD), f.has(Fmt::ITALIC)) {
        (false, false) => "F1",
        (true, false) => "F2",
        (false, true) => "F3",
        (true, true) => "F4",
    }
}

/// A picture ready to embed: the image dictionary entries and stream data,
/// plus an alpha channel (soft mask) for PNGs with transparency.
struct PdfImage {
    dict: String,
    data: Vec<u8>,
    smask: Option<Vec<u8>>,
    w: u32,
    h: u32,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// JPEGs go in as they are (DCTDecode); other pictures are decoded and
/// stored as compressed RGB.
fn pdf_image(bytes: &[u8]) -> Option<PdfImage> {
    use jpeg_decoder::{CodingProcess, PixelFormat};
    if pictures::is_jpeg(bytes) {
        let info = pictures::jpeg_info(bytes)?;
        let space = match info.pixel_format {
            PixelFormat::L8 => Some("/DeviceGray"),
            PixelFormat::RGB24 => Some("/DeviceRGB"),
            // Adobe's CMYK JPEGs are stored inverted.
            PixelFormat::CMYK32 if bytes.windows(5).take(4096).any(|w| w == b"Adobe") => {
                Some("/DeviceCMYK /Decode [1 0 1 0 1 0 1 0]")
            }
            PixelFormat::CMYK32 => Some("/DeviceCMYK"),
            PixelFormat::L16 => None,
        };
        if let Some(space) = space.filter(|_| info.coding_process != CodingProcess::Lossless) {
            return Some(PdfImage {
                dict: format!("/ColorSpace {space} /BitsPerComponent 8 /Filter /DCTDecode"),
                data: bytes.to_vec(),
                smask: None,
                w: info.width as u32,
                h: info.height as u32,
            });
        }
    }
    let r = pictures::decode(bytes)?;
    let rgb: Vec<u8> = r
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    let alpha: Vec<u8> = r.rgba.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    Some(PdfImage {
        dict: "/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode".into(),
        data: deflate(&rgb),
        smask: r.has_alpha().then(|| deflate(&alpha)),
        w: r.w,
        h: r.h,
    })
}

/// A picture on a page: its top row, place in the layout, image number
/// and width/height ratio.
type PagePic = (usize, PicBox, usize, f32);

fn page_content(rows: &[Vec<Glyph>], page: &PageSetup, pics: &[PagePic]) -> String {
    let h = page.height * 72.0;
    let left = page.margin_left * 72.0;
    let top = page.margin_top * 72.0;
    let mut text = String::new();
    let mut lines = String::new();
    let mut fills = String::new();
    let mut images = String::new();
    for &(row, pb, img, aspect) in pics {
        // Scale to fit the picture's box, keeping its proportions, and sit
        // it on the box's bottom-left corner like an inline picture.
        let (bw, bh) = (pb.cols as f32 * PT_PER_COL, pb.rows as f32 * PT_PER_ROW);
        let (iw, ih) = if bw / bh > aspect {
            (bh * aspect, bh)
        } else {
            (bw, bw / aspect)
        };
        let x = left + pb.x as f32 * PT_PER_COL;
        let y = h - top - (row + pb.rows as usize) as f32 * PT_PER_ROW;
        images.push_str(&format!(
            "q {iw:.2} 0 0 {ih:.2} {x:.2} {y:.2} cm /Im{img} Do Q\n"
        ));
    }
    // The placeholder text of a picture drawn as an image is left out.
    let covered = |r: usize, g: &Glyph| {
        g.kind == GKind::Object
            && pics.iter().any(|(top, pb, ..)| {
                r == top + pb.rows as usize - 1 && g.x >= pb.x && g.x < pb.x + pb.cols
            })
    };
    for (r, row) in rows.iter().enumerate() {
        let row_top = h - top - r as f32 * PT_PER_ROW;
        let baseline = row_top - 9.0;
        let mut glyphs: Vec<&Glyph> = row.iter().filter(|g| !covered(r, g)).collect();
        glyphs.sort_by_key(|g| g.x);
        // Batch runs of glyphs with the same font at consecutive columns.
        let mut i = 0;
        while i < glyphs.len() {
            let g = glyphs[i];
            let x = left + g.x as f32 * PT_PER_COL;
            if let Some(segs) = (g.kind == GKind::Border || matches!(g.ch, '─'..='╿'))
                .then(|| box_segments(g.ch))
                .flatten()
            {
                for &(x0, y0, x1, y1) in segs {
                    lines.push_str(&format!(
                        "{:.2} {:.2} m {:.2} {:.2} l S\n",
                        x + x0 * PT_PER_COL,
                        row_top - y0 * PT_PER_ROW,
                        x + x1 * PT_PER_COL,
                        row_top - y1 * PT_PER_ROW
                    ));
                }
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < glyphs.len()
                && glyphs[j].fmt == g.fmt
                && glyphs[j].color == g.color
                && glyphs[j].bg == g.bg
                && glyphs[j].x == glyphs[j - 1].x + 1
                && glyphs[j].kind != GKind::Border
                && box_segments(glyphs[j].ch).is_none()
            {
                j += 1;
            }
            let bytes: Vec<u8> = glyphs[i..j].iter().map(|g| win_ansi(g.ch)).collect();
            let (size, rise) = if g.fmt.has(Fmt::SUPER) {
                (8.0, 4.0)
            } else if g.fmt.has(Fmt::SUB) {
                (8.0, -2.0)
            } else {
                (FONT_SIZE, 0.0)
            };
            let width = (j - i) as f32 * PT_PER_COL;
            let rgb = |c: u32| {
                format!(
                    "{:.3} {:.3} {:.3}",
                    (c >> 16 & 255) as f32 / 255.0,
                    (c >> 8 & 255) as f32 / 255.0,
                    (c & 255) as f32 / 255.0
                )
            };
            if let Some(b) = g.bg {
                fills.push_str(&format!(
                    "{} rg {x:.2} {:.2} {width:.2} {PT_PER_ROW:.2} re f\n",
                    rgb(b),
                    row_top - PT_PER_ROW
                ));
            }
            let color = g.color.map(rgb).unwrap_or_else(|| "0 0 0".into());
            text.push_str(&format!(
                "BT {color} rg /{} {size} Tf {:.2} {:.2} Td {} Tj ET\n",
                font_for(g.fmt),
                x,
                baseline + rise,
                pdf_string(&bytes)
            ));
            if g.fmt.has(Fmt::UNDERLINE) {
                lines.push_str(&format!(
                    "{x:.2} {:.2} m {:.2} {:.2} l S\n",
                    baseline - 1.5,
                    x + width,
                    baseline - 1.5
                ));
            }
            if g.fmt.has(Fmt::STRIKE) {
                lines.push_str(&format!(
                    "{x:.2} {:.2} m {:.2} {:.2} l S\n",
                    baseline + 3.5,
                    x + width,
                    baseline + 3.5
                ));
            }
            i = j;
        }
    }
    format!("{fills}{images}0 0 0 RG 0.6 w\n{lines}{text}")
}

pub fn to_bytes(doc: &Document) -> Vec<u8> {
    let layout = Layout::build(doc);
    let pages = compose(doc, &layout);
    let w = doc.page.width * 72.0;
    let h = doc.page.height * 72.0;

    // Pictures, each embedded once however often it is used.
    let mut images: Vec<PdfImage> = Vec::new();
    let mut by_part: HashMap<String, Option<usize>> = HashMap::new();
    let page_pics: Vec<Vec<PagePic>> = {
        let st = doc.store.borrow();
        pages
            .iter()
            .map(|p| {
                p.pics
                    .iter()
                    .filter_map(|&(row, pb)| {
                        let o = st.obj(pb.obj)?;
                        let (cx, cy) = pictures::extent(&o.xml)?;
                        let (part, bytes) = pictures::source(doc, o)?;
                        let img = *by_part.entry(part).or_insert_with(|| {
                            let im = pdf_image(bytes)?;
                            images.push(im);
                            Some(images.len() - 1)
                        });
                        Some((row, pb, img?, cx.max(1) as f32 / cy.max(1) as f32))
                    })
                    .collect()
            })
            .collect()
    };

    // Object numbers: 1 catalog, 2 pages, 3-6 fonts, 7 info, then
    // page/content pairs, then images (each followed by its soft mask).
    let mut objs: Vec<Vec<u8>> = Vec::new();
    let first_page = 8;
    let first_image = first_page + pages.len() * 2;
    let mut image_nums = Vec::new();
    let mut n = first_image;
    for im in &images {
        image_nums.push(n);
        n += 1 + usize::from(im.smask.is_some());
    }
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", first_page + i * 2))
        .collect();
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(
        format!(
            "<< /Type /Pages /Kids [{}] /Count {} >>",
            kids.join(" "),
            pages.len()
        )
        .into_bytes(),
    );
    for name in [
        "Courier",
        "Courier-Bold",
        "Courier-Oblique",
        "Courier-BoldOblique",
    ] {
        objs.push(
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>"
            )
            .into_bytes(),
        );
    }
    let info = |s: &str| pdf_string(&s.chars().map(win_ansi).collect::<Vec<u8>>());
    objs.push(
        format!(
            "<< /Producer (WordBench) /Creator (WordBench) /Title {} /Author {} /Subject {} >>",
            info(&doc.core.title),
            info(&doc.core.author),
            info(&doc.core.subject)
        )
        .into_bytes(),
    );
    for (i, p) in pages.iter().enumerate() {
        let pics = &page_pics[i];
        let content = page_content(&p.rows, &doc.page, pics);
        let mut used: Vec<usize> = pics.iter().map(|p| p.2).collect();
        used.sort_unstable();
        used.dedup();
        let xobjects = if used.is_empty() {
            String::new()
        } else {
            let refs: Vec<String> = used
                .iter()
                .map(|&k| format!("/Im{k} {} 0 R", image_nums[k]))
                .collect();
            format!(" /XObject << {} >>", refs.join(" "))
        };
        objs.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R /F4 6 0 R >>{xobjects} >> /Contents {} 0 R >>",
                first_page + i * 2 + 1
            )
            .into_bytes(),
        );
        objs.push(
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            )
            .into_bytes(),
        );
    }
    let stream = |dict: String, data: &[u8]| {
        let mut o = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
        o.extend_from_slice(data);
        o.extend_from_slice(b"\nendstream");
        o
    };
    for (k, im) in images.iter().enumerate() {
        let size = format!(
            "/Type /XObject /Subtype /Image /Width {} /Height {}",
            im.w, im.h
        );
        let smask = if im.smask.is_some() {
            format!(" /SMask {} 0 R", image_nums[k] + 1)
        } else {
            String::new()
        };
        objs.push(stream(format!("{size} {}{smask}", im.dict), &im.data));
        if let Some(a) = &im.smask {
            objs.push(stream(
                format!("{size} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),
                a,
            ));
        }
    }

    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 7 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

pub fn save(doc: &Document, path: &Path) -> Result<(), String> {
    super::write_atomic(path, &to_bytes(doc))
}

/// Send the document to the default printer via `lp` (or `lpr`).
pub fn print(doc: &Document) -> Result<String, String> {
    let path = std::env::temp_dir().join(format!("wordbench-print-{}.pdf", std::process::id()));
    std::fs::write(&path, to_bytes(doc)).map_err(|e| e.to_string())?;
    crate::platform::print_pdf(&path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::Editor;

    fn text(pdf: &[u8]) -> String {
        String::from_utf8_lossy(pdf).into_owned()
    }

    #[test]
    fn embeds_docx_picture() {
        let doc =
            crate::io::docx::from_bytes(include_bytes!("../../tests/fixtures/rich.docx")).unwrap();
        let pdf = text(&to_bytes(&doc));
        assert!(
            pdf.contains("/Subtype /Image /Width 40 /Height 20 /ColorSpace /DeviceRGB"),
            "{pdf}"
        );
        assert!(pdf.contains("/XObject << /Im0 "));
        // 0.79" x 0.39" picture: 8 columns by 2 lines, drawn at its size.
        assert!(pdf.contains("q 48.00 0 0 24.00 "), "{pdf}");
        assert!(pdf.contains(" cm /Im0 Do Q"));
        // The placeholder text is not printed over it.
        assert!(!pdf.contains("([Image"), "{pdf}");
    }

    #[test]
    fn embeds_inserted_jpeg_and_png() {
        let dir = std::env::temp_dir().join(format!("wordbench-pdf-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("alpha.png");
        std::fs::write(&png, crate::pictures::tests::sample_png(96, 48)).unwrap();
        let mut ed = Editor::new();
        ed.insert_picture(Path::new("tests/fixtures/picture.jpg"))
            .unwrap();
        ed.insert_picture(&png).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let bytes = to_bytes(&ed.doc);
        let pdf = text(&bytes);
        // The JPEG goes in unchanged.
        let jpeg = std::fs::read("tests/fixtures/picture.jpg").unwrap();
        assert!(
            pdf.contains(
                "/Width 30 /Height 20 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"
            ),
            "{pdf}"
        );
        assert!(bytes.windows(jpeg.len()).any(|w| w == jpeg.as_slice()));
        // The PNG's transparency becomes a soft mask.
        assert!(pdf.contains("/Width 96 /Height 48 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask"), "{pdf}");
        assert!(pdf.contains("/Im0 Do") && pdf.contains("/Im1 Do"));
    }
}
