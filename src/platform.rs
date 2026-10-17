//! Things that differ between Linux (and other Unix), macOS and Windows:
//! where settings live, the home folder, opening links, printing, finding
//! fonts and today's date.

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The folder for settings and the personal dictionary:
/// `%APPDATA%\WordBench` on Windows, `~/Library/Application Support/WordBench`
/// on macOS, `$XDG_CONFIG_HOME/wordbench` (or `~/.config/wordbench`) elsewhere.
pub fn config_dir() -> PathBuf {
    if cfg!(windows)
        && let Some(a) = std::env::var_os("APPDATA")
    {
        return PathBuf::from(a).join("WordBench");
    }
    if cfg!(target_os = "macos")
        && let Some(h) = home_dir()
    {
        return h
            .join("Library")
            .join("Application Support")
            .join("WordBench");
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("wordbench")
}

/// The user's name, for comments and tracked changes.
pub fn user_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "WordBench user".into())
}

/// Open a web address (or file) with the system's default program.
pub fn open(target: &str) -> std::io::Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", target]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(target);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Send a PDF to the default printer.
pub fn print_pdf(path: &std::path::Path) -> Result<String, String> {
    if cfg!(windows) {
        // Prints through the program registered for PDFs (Edge, Acrobat...).
        let script = format!(
            "Start-Process -FilePath '{}' -Verb Print",
            path.display().to_string().replace('\'', "''")
        );
        return match Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
        {
            Ok(o) if o.status.success() => Ok("Sent to the printer".into()),
            Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
            Err(e) => Err(format!("could not start PowerShell: {e}")),
        };
    }
    let mut last_err = String::from("no print command found (lp or lpr; on Linux install CUPS)");
    for cmd in ["lp", "lpr"] {
        match Command::new(cmd).arg(path).output() {
            Ok(o) if o.status.success() => {
                let msg = String::from_utf8_lossy(&o.stdout).trim().to_string();
                return Ok(if msg.is_empty() {
                    "Sent to the printer".into()
                } else {
                    msg
                });
            }
            Ok(o) => last_err = String::from_utf8_lossy(&o.stderr).trim().to_string(),
            Err(_) => continue,
        }
    }
    Err(last_err)
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Font family names installed on this computer.
pub fn installed_fonts() -> Vec<String> {
    // fontconfig: Linux, BSD, and macOS with Homebrew.
    if let Some(out) = run("fc-list", &[":", "family"]) {
        return out
            .lines()
            .filter_map(|l| l.split(',').next())
            .map(|f| f.trim().replace('\\', ""))
            .collect();
    }
    if cfg!(windows) {
        // Registered fonts, e.g. "Arial Bold (TrueType)    REG_SZ    arialbd.ttf".
        let mut v = Vec::new();
        for key in [
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
            r"HKCU\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        ] {
            for line in run("reg", &["query", key]).unwrap_or_default().lines() {
                let Some((name, _)) = line.trim().split_once("    REG_") else {
                    continue;
                };
                let name = name.split(" (").next().unwrap_or(name).trim();
                v.push(strip_style(name));
            }
        }
        return v;
    }
    // macOS without fontconfig: font files in the usual folders.
    let mut dirs = vec![
        PathBuf::from("/System/Library/Fonts"),
        PathBuf::from("/Library/Fonts"),
    ];
    if let Some(h) = home_dir() {
        dirs.push(h.join("Library/Fonts"));
        dirs.push(h.join(".local/share/fonts"));
    }
    let mut v = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if matches!(
                p.extension()
                    .and_then(|x| x.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("ttf" | "otf" | "ttc")
            ) && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            {
                v.push(strip_style(&stem.replace(['-', '_'], " ")));
            }
        }
    }
    v
}

/// "Arial Bold Italic" -> "Arial".
fn strip_style(name: &str) -> String {
    const STYLES: [&str; 12] = [
        "Bold",
        "Italic",
        "Oblique",
        "Light",
        "Medium",
        "Semibold",
        "SemiBold",
        "Black",
        "Thin",
        "Regular",
        "Condensed",
        "Narrow",
    ];
    let words: Vec<&str> = name.split_whitespace().collect();
    let keep = words
        .iter()
        .rposition(|w| !STYLES.contains(w))
        .map(|i| i + 1)
        .unwrap_or(words.len());
    words[..keep.max(1)].join(" ")
}

/// The installed font the system shows in place of a font that isn't
/// installed (fontconfig only).
pub fn font_substitute(name: &str) -> Option<String> {
    let out = run("fc-match", &[name, ":", "family"])?;
    let sub = out.lines().next()?.split(',').next()?.trim().to_string();
    (!sub.is_empty()).then_some(sub)
}

/// Today's date as "October 17, 2026", in local time when the system can
/// tell us.
pub fn local_date() -> Option<String> {
    let out = if cfg!(windows) {
        run(
            "powershell",
            &["-NoProfile", "-Command", "Get-Date -Format 'MMMM d, yyyy'"],
        )
    } else {
        // %e (space-padded day) works with both GNU and BSD date.
        run("date", &["+%B %e, %Y"])
    };
    out.map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_style_names() {
        assert_eq!(strip_style("Arial Bold Italic"), "Arial");
        assert_eq!(strip_style("Times New Roman"), "Times New Roman");
        assert_eq!(strip_style("Bold"), "Bold");
    }

    #[test]
    fn settings_folder_is_named_for_wordbench() {
        let d = config_dir();
        let name = d.file_name().unwrap().to_string_lossy().to_lowercase();
        assert_eq!(name, "wordbench");
    }
}
