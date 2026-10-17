//! Pictures: finding a picture's bytes (in the original .docx package or
//! added while editing), its size on the 10 cpi / 6 lpi grid, and decoding
//! PNG and JPEG data for PDF export and terminal graphics.

use crate::model::*;

const EMU_PER_INCH: f64 = 914_400.0;
/// Pictures larger than this are left as placeholders.
const MAX_PIXELS: u64 = 40_000_000;

/// Value of attribute `name` (any namespace prefix) in a start tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let mut from = 0;
    while let Some(i) = tag[from..].find(&pat) {
        let at = from + i;
        let before = tag[..at].chars().next_back();
        if matches!(before, Some(' ' | ':' | '\t' | '\n')) {
            let v = &tag[at + pat.len()..];
            return Some(&v[..v.find('"')?]);
        }
        from = at + pat.len();
    }
    None
}

/// The first start tag with local name `local`.
fn tag<'a>(xml: &'a str, local: &str) -> Option<&'a str> {
    let pat = format!(":{local}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&pat) {
        let at = from + i;
        let end = at + pat.len();
        let open = xml[..at].rfind('<')?;
        let named = xml[open + 1..at].chars().all(|c| c.is_ascii_alphanumeric());
        if named && matches!(xml.as_bytes().get(end), Some(b' ' | b'/' | b'>')) {
            return Some(&xml[open..end + xml[end..].find('>')?]);
        }
        from = end;
    }
    None
}

/// Displayed size of a drawing in EMUs (`wp:extent`).
pub fn extent(xml: &str) -> Option<(u64, u64)> {
    let t = tag(xml, "extent")?;
    Some((attr(t, "cx")?.parse().ok()?, attr(t, "cy")?.parse().ok()?))
}

/// Relationship id of the picture data (`a:blip r:embed`).
pub fn embed_id(xml: &str) -> Option<&str> {
    attr(tag(xml, "blip")?, "embed").filter(|s| !s.is_empty())
}

/// An inline object that is a picture with stored data and a known size.
pub fn is_picture(o: &InlineObj) -> bool {
    o.kind == ObjKind::Visible
        && !crate::review::is_deletion(o)
        && embed_id(&o.xml).is_some()
        && extent(&o.xml).is_some()
}

/// Columns and lines a picture covers on the 10 cpi / 6 lpi grid.
pub fn grid_size(o: &InlineObj) -> Option<(u16, u16)> {
    if !is_picture(o) {
        return None;
    }
    let (cx, cy) = extent(&o.xml)?;
    let cells = |emu: u64, per_inch: f32| {
        ((emu as f64 / EMU_PER_INCH * per_inch as f64).round() as u64).clamp(1, 400) as u16
    };
    Some((cells(cx, CPI), cells(cy, LPI)))
}

/// Package path and bytes of a picture's data.
pub fn source<'a>(doc: &'a Document, o: &InlineObj) -> Option<(String, &'a [u8])> {
    let id = embed_id(&o.xml)?;
    let path = crate::io::docx::rel_part(doc, id)?;
    let bytes = doc
        .new_parts
        .iter()
        .rev()
        .find(|(n, _)| *n == path)
        .or_else(|| doc.docx.as_ref()?.parts.iter().find(|(n, _)| *n == path))
        .map(|(_, b)| b.as_slice())?;
    Some((path, bytes))
}

pub fn is_png(b: &[u8]) -> bool {
    b.starts_with(b"\x89PNG\r\n\x1a\n")
}

pub fn is_jpeg(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8, 0xFF])
}

/// Decoded pixels, 8-bit RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Raster {
    pub fn has_alpha(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255)
    }

    /// Nearest-neighbour resize.
    pub fn scaled(&self, w: u32, h: u32) -> Raster {
        let (w, h) = (w.max(1), h.max(1));
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let sy = (y as u64 * self.h as u64 / h as u64) as u32;
            for x in 0..w {
                let sx = (x as u64 * self.w as u64 / w as u64) as u32;
                let i = ((sy * self.w + sx) * 4) as usize;
                rgba.extend_from_slice(&self.rgba[i..i + 4]);
            }
        }
        Raster { w, h, rgba }
    }
}

/// Decode a PNG or JPEG picture.
pub fn decode(b: &[u8]) -> Option<Raster> {
    if is_png(b) {
        decode_png(b)
    } else if is_jpeg(b) {
        decode_jpeg(b)
    } else {
        None
    }
}

fn decode_png(b: &[u8]) -> Option<Raster> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(b));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let info = reader.info();
    if info.width as u64 * info.height as u64 > MAX_PIXELS {
        return None;
    }
    let mut buf = vec![0; reader.output_buffer_size()?];
    let out = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (out.width, out.height);
    let px = &buf[..out.buffer_size()];
    let rgba: Vec<u8> = match out.color_type {
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Indexed => return None, // expanded by normalize_to_color8
    };
    (rgba.len() == (w * h * 4) as usize).then_some(Raster { w, h, rgba })
}

/// Width, height and pixel format of a JPEG.
pub fn jpeg_info(b: &[u8]) -> Option<jpeg_decoder::ImageInfo> {
    let mut dec = jpeg_decoder::Decoder::new(b);
    dec.read_info().ok()?;
    dec.info()
}

fn decode_jpeg(b: &[u8]) -> Option<Raster> {
    let info = jpeg_info(b)?;
    if info.width as u64 * info.height as u64 > MAX_PIXELS {
        return None;
    }
    let mut dec = jpeg_decoder::Decoder::new(b);
    let px = dec.decode().ok()?;
    use jpeg_decoder::PixelFormat::*;
    let rgba: Vec<u8> = match info.pixel_format {
        L8 => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        L16 => px
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], 255])
            .collect(),
        RGB24 => px
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        // Adobe CMYK JPEGs store inverted values.
        CMYK32 => px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let k = p[3] as u32;
                let ch = |c: u8| (c as u32 * k / 255) as u8;
                [ch(p[0]), ch(p[1]), ch(p[2]), 255]
            })
            .collect(),
    };
    let (w, h) = (info.width as u32, info.height as u32);
    (rgba.len() == (w * h * 4) as usize).then_some(Raster { w, h, rgba })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A small RGBA PNG (left half red, right half half-transparent blue).
    pub fn sample_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            let px: Vec<u8> = (0..w * h)
                .flat_map(|i| {
                    if i % w < w / 2 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 128]
                    }
                })
                .collect();
            wr.write_image_data(&px).unwrap();
        }
        out
    }

    pub fn picture_xml(rid: &str, cx: u64, cy: u64) -> String {
        format!(
            r#"<w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/><wp:docPr id="1" name="p"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{rid}"/></pic:blipFill><pic:spPr><a:xfrm><a:ext cx="1" cy="1"/></a:xfrm></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"#
        )
    }

    #[test]
    fn reads_extent_and_embed() {
        let x = picture_xml("rId7", 914400, 457200);
        assert_eq!(extent(&x), Some((914400, 457200)));
        assert_eq!(embed_id(&x), Some("rId7"));
        let o = InlineObj {
            kind: ObjKind::Visible,
            xml: x,
            label: "[Image]".into(),
            in_run: true,
        };
        assert_eq!(grid_size(&o), Some((10, 3)));
        let chart = InlineObj {
            xml: r#"<w:drawing><wp:extent cx="1" cy="1"/><c:chart r:id="rId2"/></w:drawing>"#
                .into(),
            ..o
        };
        assert!(!is_picture(&chart));
    }

    #[test]
    fn decodes_png() {
        let r = decode(&sample_png(4, 2)).unwrap();
        assert_eq!((r.w, r.h), (4, 2));
        assert_eq!(&r.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&r.rgba[8..12], &[0, 0, 255, 128]);
        assert!(r.has_alpha());
        let s = r.scaled(2, 1);
        assert_eq!(s.rgba, vec![255, 0, 0, 255, 0, 0, 255, 128]);
    }

    #[test]
    fn finds_picture_bytes() {
        let d = crate::io::docx::load(std::path::Path::new("tests/fixtures/rich.docx")).unwrap();
        let st = d.store.borrow();
        let o = st.objects.iter().find(|o| is_picture(o)).expect("picture");
        let (path, bytes) = source(&d, o).unwrap();
        assert_eq!(path, "word/media/image1.png");
        assert!(is_png(bytes));
        assert!(decode(bytes).is_some());
    }
}
