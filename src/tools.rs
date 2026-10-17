//! Spell checker and thesaurus, both offline.
//!
//! Spelling uses `hunspell -a` or `aspell -a` (the ispell pipe protocol) when
//! installed, falling back to a word list such as /usr/share/dict/words.
//! The thesaurus reads LibreOffice/OpenOffice MyThes data files
//! (package `mythes-en-us`) or WordNet's `wn` command.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub fn config_dir() -> PathBuf {
    crate::platform::config_dir()
}

enum Backend {
    Pipe {
        _child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
    Words(HashSet<String>),
}

pub struct Speller {
    backend: Backend,
    pub name: String,
    cache: HashMap<String, Option<Vec<String>>>,
    ignored: HashSet<String>,
    personal: HashSet<String>,
}

fn spawn_pipe(cmd: &str, args: &[&str]) -> Option<Backend> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdin = child.stdin.take()?;
    let mut stdout = BufReader::new(child.stdout.take()?);
    let mut banner = String::new();
    // The first line is a version banner ("@(#) International Ispell ...").
    if stdout.read_line(&mut banner).ok()? == 0 || !banner.starts_with("@(#)") {
        let _ = child.kill();
        return None;
    }
    Some(Backend::Pipe {
        _child: child,
        stdin,
        stdout,
    })
}

fn load_word_list(path: &Path) -> Option<HashSet<String>> {
    let s = std::fs::read_to_string(path).ok()?;
    let set: HashSet<String> = s
        .lines()
        .map(|l| l.split('/').next().unwrap_or("").trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    (set.len() > 1000).then_some(set)
}

impl Speller {
    pub fn new(lang: &str) -> Result<Speller, String> {
        let attempts: [(&str, Vec<String>); 2] = [
            (
                "hunspell",
                vec![
                    "-a".into(),
                    "-i".into(),
                    "utf-8".into(),
                    "-d".into(),
                    lang.into(),
                ],
            ),
            (
                "aspell",
                vec![
                    "-a".into(),
                    "--encoding=utf-8".into(),
                    format!("--lang={lang}"),
                ],
            ),
        ];
        let mut found = None;
        for (cmd, args) in &attempts {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            if let Some(b) = spawn_pipe(cmd, &args) {
                found = Some((b, cmd.to_string()));
                break;
            }
        }
        if found.is_none() {
            let lists = [
                config_dir().join("words.txt"),
                PathBuf::from("/usr/share/dict/words"),
                PathBuf::from("/usr/share/dict/american-english"),
                PathBuf::from("/usr/share/dict/british-english"),
            ];
            for p in lists {
                if let Some(set) = load_word_list(&p) {
                    found = Some((Backend::Words(set), format!("word list {}", p.display())));
                    break;
                }
            }
        }
        let Some((backend, name)) = found else {
            return Err(format!("No spell checker found. {}", SPELL_HINT));
        };
        let personal: HashSet<String> = std::fs::read_to_string(config_dir().join("personal.dic"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty())
            .collect();
        Ok(Speller {
            backend,
            name,
            cache: HashMap::new(),
            ignored: HashSet::new(),
            personal,
        })
    }

    fn query(&mut self, word: &str) -> Option<Vec<String>> {
        match &mut self.backend {
            Backend::Pipe { stdin, stdout, .. } => {
                if writeln!(stdin, "^{word}")
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    return None;
                }
                let mut result = None;
                loop {
                    let mut line = String::new();
                    match stdout.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if line.starts_with('&') || line.starts_with('?') {
                        let sugg = line.split_once(": ").map(|(_, s)| s).unwrap_or("");
                        result = Some(
                            sugg.split(", ")
                                .map(str::to_string)
                                .filter(|s| !s.is_empty())
                                .collect(),
                        );
                    } else if line.starts_with('#') {
                        result = Some(Vec::new());
                    }
                }
                result
            }
            Backend::Words(set) => {
                let lw = word.to_lowercase();
                let base = lw.trim_end_matches("'s").trim_end_matches("’s");
                if set.contains(&lw) || set.contains(base) {
                    None
                } else {
                    Some(Vec::new())
                }
            }
        }
    }

    /// `None` if the word is fine, otherwise suggested replacements.
    pub fn check(&mut self, word: &str) -> Option<Vec<String>> {
        if word.chars().count() < 2 || word.chars().any(|c| c.is_numeric()) {
            return None;
        }
        let lw = word.to_lowercase();
        if self.ignored.contains(&lw) || self.personal.contains(&lw) {
            return None;
        }
        if let Some(r) = self.cache.get(word) {
            return r.clone();
        }
        let r = self.query(word);
        self.cache.insert(word.to_string(), r.clone());
        r
    }

    pub fn ignore(&mut self, word: &str) {
        self.ignored.insert(word.to_lowercase());
    }

    pub fn add(&mut self, word: &str) -> Result<(), String> {
        let lw = word.to_lowercase();
        self.personal.insert(lw.clone());
        let dir = config_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("personal.dic"))
            .map_err(|e| e.to_string())?;
        writeln!(f, "{lw}").map_err(|e| e.to_string())
    }
}

/// One sense of a word: part of speech (or gloss) and its synonyms.
pub type Sense = (String, Vec<String>);

enum ThesBackend {
    MyThes {
        idx: HashMap<String, u64>,
        dat: PathBuf,
        latin1: bool,
    },
    WordNet,
}

#[cfg(windows)]
const SPELL_HINT: &str = "Install Hunspell (scoop install hunspell, or choco install hunspell.portable) with an en_US dictionary, or put a word list in the settings folder as words.txt.";
#[cfg(target_os = "macos")]
const SPELL_HINT: &str = "Install one: brew install aspell (or hunspell with a dictionary).";
#[cfg(not(any(windows, target_os = "macos")))]
const SPELL_HINT: &str = "Install one: sudo apt install aspell-en (or hunspell hunspell-en-us).";

#[cfg(windows)]
const THES_HINT: &str = "Copy th_en_US_v2.dat and th_en_US_v2.idx (from a LibreOffice English dictionary) into the WordBench settings folder.";
#[cfg(target_os = "macos")]
const THES_HINT: &str = "Install WordNet (brew install wordnet), or copy th_en_US_v2.dat and .idx into the WordBench settings folder.";
#[cfg(not(any(windows, target_os = "macos")))]
const THES_HINT: &str = "Install one: sudo apt install mythes-en-us (or wordnet).";

pub struct Thesaurus {
    backend: ThesBackend,
}

fn mythes_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("WORDBENCH_THESAURUS") {
        v.push(PathBuf::from(p));
    }
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/mythes",
        "/usr/share/myspell/dicts",
        "/usr/share/hunspell",
        "/usr/local/share/mythes",
        "/usr/local/share/hunspell",
        "/opt/homebrew/share/mythes",
        "/opt/homebrew/share/hunspell",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    // A thesaurus copied into the settings folder (e.g. th_en_US_v2.dat and
    // .idx from a LibreOffice dictionary extension) works on every system.
    dirs.insert(0, config_dir());
    for dir in dirs {
        for name in [
            "th_en_US_v2.dat",
            "th_en_US_new.dat",
            "th_en_GB_v2.dat",
            "th_en_US.dat",
        ] {
            v.push(dir.join(name));
        }
    }
    v.push(config_dir().join("thesaurus.dat"));
    v
}

fn decode(bytes: &[u8], latin1: bool) -> String {
    if latin1 {
        bytes.iter().map(|&b| b as char).collect()
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

impl Thesaurus {
    pub fn new() -> Result<Thesaurus, String> {
        for dat in mythes_candidates() {
            let idx_path = dat.with_extension("idx");
            if !dat.exists() || !idx_path.exists() {
                continue;
            }
            let raw = std::fs::read(&idx_path).map_err(|e| e.to_string())?;
            let enc_line = raw.split(|&b| b == b'\n').next().unwrap_or(b"");
            let latin1 = String::from_utf8_lossy(enc_line).contains("8859");
            let text = decode(&raw, latin1);
            let mut idx = HashMap::new();
            for line in text.lines().skip(2) {
                if let Some((w, off)) = line.rsplit_once('|')
                    && let Ok(o) = off.trim().parse::<u64>()
                {
                    idx.insert(w.to_lowercase(), o);
                }
            }
            return Ok(Thesaurus {
                backend: ThesBackend::MyThes { idx, dat, latin1 },
            });
        }
        if Command::new("wn")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Ok(Thesaurus {
                backend: ThesBackend::WordNet,
            });
        }
        Err(format!("No thesaurus found. {}", THES_HINT))
    }

    pub fn lookup(&self, word: &str) -> Vec<Sense> {
        let lw = word.to_lowercase();
        match &self.backend {
            ThesBackend::MyThes { idx, dat, latin1 } => {
                let Some(&off) = idx.get(&lw) else {
                    return Vec::new();
                };
                let Ok(mut f) = std::fs::File::open(dat) else {
                    return Vec::new();
                };
                if f.seek(SeekFrom::Start(off)).is_err() {
                    return Vec::new();
                }
                let mut buf = vec![0u8; 64 * 1024];
                let n = f.read(&mut buf).unwrap_or(0);
                let text = decode(&buf[..n], *latin1);
                let mut lines = text.lines();
                let count: usize = lines
                    .next()
                    .and_then(|l| l.rsplit_once('|'))
                    .and_then(|(_, c)| c.trim().parse().ok())
                    .unwrap_or(0);
                lines
                    .take(count)
                    .filter_map(|l| {
                        let mut parts = l.split('|');
                        let pos = parts
                            .next()?
                            .trim()
                            .trim_matches(|c| c == '(' || c == ')')
                            .to_string();
                        let syns: Vec<String> = parts
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty() && s.to_lowercase() != lw)
                            .collect();
                        (!syns.is_empty()).then_some((pos, syns))
                    })
                    .collect()
            }
            ThesBackend::WordNet => {
                let Ok(out) = Command::new("wn")
                    .arg(&lw)
                    .args(["-synsn", "-synsv", "-synsa", "-synsr"])
                    .output()
                else {
                    return Vec::new();
                };
                let text = String::from_utf8_lossy(&out.stdout);
                let mut senses = Vec::new();
                let mut pos = String::new();
                let mut lines = text.lines().peekable();
                while let Some(l) = lines.next() {
                    if l.starts_with("Synonyms/Hypernyms") {
                        pos = l
                            .split(" of ")
                            .nth(1)
                            .and_then(|s| s.split_whitespace().next())
                            .unwrap_or("")
                            .to_string();
                    } else if l.starts_with("Sense ")
                        && let Some(next) = lines.next()
                    {
                        let words = next.split("--").next().unwrap_or("");
                        let syns: Vec<String> = words
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty() && s.to_lowercase() != lw)
                            .collect();
                        if !syns.is_empty() {
                            senses.push((pos.clone(), syns));
                        }
                    }
                }
                senses
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mythes_lookup() {
        let dir = std::env::temp_dir().join(format!("wordbench-thes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dat_text =
            "UTF-8\nhappy|2\n(adj)|glad|cheerful\n(adj)|felicitous\nsad|1\n(adj)|unhappy\n";
        let dat = dir.join("th.dat");
        std::fs::write(&dat, dat_text).unwrap();
        let off_sad = dat_text.find("sad|").unwrap();
        std::fs::write(
            dir.join("th.idx"),
            format!("UTF-8\n2\nhappy|6\nsad|{off_sad}\n"),
        )
        .unwrap();
        // Build directly rather than via search paths.
        let raw = std::fs::read_to_string(dir.join("th.idx")).unwrap();
        let idx = raw
            .lines()
            .skip(2)
            .filter_map(|l| l.rsplit_once('|'))
            .map(|(w, o)| (w.to_string(), o.parse().unwrap()))
            .collect();
        let t = Thesaurus {
            backend: ThesBackend::MyThes {
                idx,
                dat,
                latin1: false,
            },
        };
        let s = t.lookup("Happy");
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0],
            (
                "adj".to_string(),
                vec!["glad".to_string(), "cheerful".to_string()]
            )
        );
        assert_eq!(t.lookup("sad")[0].1, vec!["unhappy".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
