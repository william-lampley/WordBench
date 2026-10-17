//! File formats: native (.wbd JSON), Word (.docx), PDF and plain text.

pub mod docx;
pub mod html;
pub mod native;
pub mod pdf;
pub mod xml;

use crate::model::Document;
use std::path::Path;

pub fn ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub fn load(path: &Path) -> Result<Document, String> {
    match ext(path).as_str() {
        "docx" => docx::load(path),
        "wbd" => native::load(path),
        "txt" | "text" | "md" => {
            let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            Ok(Document::from_plain_text(&s))
        }
        "pdf" => Err("PDF files can be exported but not opened".into()),
        "doc" => Err("Legacy .doc files are not supported; save as .docx first".into()),
        _ => {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if bytes.starts_with(b"PK") {
                return docx::load(path);
            }
            if let Ok(d) = native::load(path) {
                return Ok(d);
            }
            Ok(Document::from_plain_text(&String::from_utf8_lossy(&bytes)))
        }
    }
}

/// Write bytes atomically (temp file + rename) so a failed save never
/// destroys the previous version.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let tmp = path.with_file_name(format!(".{name}.tmp~"));
    std::fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Save by extension. Returns a note for the status line when the format
/// drops information.
pub fn save(doc: &Document, path: &Path) -> Result<Option<&'static str>, String> {
    match ext(path).as_str() {
        "docx" => docx::save(doc, path).map(|_| None),
        "pdf" => pdf::save(doc, path).map(|_| Some("exported as PDF")),
        "txt" | "text" | "md" => write_atomic(path, doc.to_plain_text().as_bytes())
            .map(|_| Some("plain text: formatting not kept")),
        _ => native::save(doc, path).map(|_| None),
    }
}
