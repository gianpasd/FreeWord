//! Spell checking with Hunspell dictionaries (via `spellbook`).
//!
//! American English is bundled; other languages are picked up from
//! LibreOffice's dictionary folders and from `%APPDATA%\FreeWord\dictionaries`.
//! Dictionaries load on a background thread so startup stays fast.

use crate::doc::{OBJ, Para};
use spellbook::Dictionary;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const BUNDLED_AFF: &str = include_str!("../assets/dictionaries/en_US.aff");
const BUNDLED_DIC: &str = include_str!("../assets/dictionaries/en_US.dic");

#[derive(Clone, Debug)]
pub struct Lang {
    /// e.g. `en_US`
    pub code: String,
    /// None for the bundled dictionary.
    pub files: Option<(PathBuf, PathBuf)>,
}

impl Lang {
    /// Human-readable name.
    pub fn label(&self) -> String {
        let (lang, region) = self.code.split_once(['_', '-']).unwrap_or((&self.code, ""));
        let name = match lang {
            "en" => "English",
            "es" => "Spanish",
            "fr" => "French",
            "it" => "Italian",
            "de" => "German",
            "pt" => "Portuguese",
            "nl" => "Dutch",
            "pl" => "Polish",
            "sv" => "Swedish",
            "da" => "Danish",
            "nb" | "no" => "Norwegian",
            "fi" => "Finnish",
            "ru" => "Russian",
            "cs" => "Czech",
            other => other,
        };
        if region.is_empty() { name.to_string() } else { format!("{name} ({region})") }
    }
}

type Shared = Arc<Mutex<Option<Arc<Dictionary>>>>;

pub struct Speller {
    pub langs: Vec<Lang>,
    pub active: String,
    pub enabled: bool,
    dict: Shared,
    loading: bool,
    words: HashMap<String, bool>,
    paras: HashMap<u64, Vec<(usize, usize)>>,
    user: HashSet<String>,
    ignored: HashSet<String>,
}

fn user_dir() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|a| PathBuf::from(a).join("FreeWord"))
}

fn user_dict_path() -> Option<PathBuf> {
    user_dir().map(|d| d.join("custom.dic"))
}

/// Reads a dictionary file, converting legacy 8-bit encodings to UTF-8.
fn read_text(path: &PathBuf) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    match String::from_utf8(bytes) {
        Ok(s) => Some(s),
        // ISO-8859-x: map bytes directly to code points (exact for Latin-1).
        Err(e) => Some(e.into_bytes().iter().map(|b| *b as char).collect()),
    }
}

fn discover() -> Vec<Lang> {
    let mut out = vec![Lang { code: "en_US".into(), files: None }];
    let mut dirs: Vec<PathBuf> = Vec::new();
    for base in ["C:\\Program Files\\LibreOffice\\share\\extensions", "C:\\Program Files (x86)\\LibreOffice\\share\\extensions"] {
        if let Ok(rd) = std::fs::read_dir(base) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("dict-") {
                    dirs.push(e.path());
                }
            }
        }
    }
    if let Some(u) = user_dir() {
        dirs.push(u.join("dictionaries"));
    }
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "dic") {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                if stem.starts_with("hyph_") || stem.starts_with("th_") {
                    continue;
                }
                let aff = p.with_extension("aff");
                if aff.is_file() && !out.iter().any(|l| l.code == stem) {
                    out.push(Lang { code: stem, files: Some((aff, p)) });
                }
            }
        }
    }
    out.sort_by(|a, b| a.label().cmp(&b.label()));
    out
}

/// Whether a token should be checked at all.
fn checkable(w: &str) -> bool {
    let n = w.chars().count();
    if n < 2 {
        return false;
    }
    if w.chars().any(|c| c.is_ascii_digit() || c == '_' || c == '@' || c == '/') {
        return false;
    }
    // Like Word's default "ignore words in UPPERCASE".
    if w.chars().all(|c| !c.is_lowercase()) {
        return false;
    }
    true
}

/// Splits a paragraph into word ranges (character offsets).
pub fn words(p: &Para) -> Vec<(usize, usize)> {
    let c = &p.chars;
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i].is_alphanumeric() && !p.fmts[i].hidden {
            let s = i;
            while i < c.len()
                && !p.fmts[i].hidden
                && (c[i].is_alphanumeric()
                    || ((c[i] == '\'' || c[i] == '’' || c[i] == '-') && i + 1 < c.len() && c[i + 1].is_alphanumeric()))
            {
                i += 1;
            }
            // Skip URLs, e-mail addresses and file names.
            let glued = (s > 0 && matches!(c[s - 1], '.' | '/' | '@' | ':' | '\\' | '_'))
                || (i < c.len() && matches!(c[i], '@' | '/' | '\\' | '_') )
                || (i + 1 < c.len() && c[i] == '.' && c[i + 1].is_alphanumeric());
            if !glued {
                out.push((s, i));
            }
        } else {
            i += 1;
        }
    }
    out
}

impl Speller {
    pub fn new() -> Self {
        let user = user_dict_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
            .unwrap_or_default();
        let mut sp = Speller {
            langs: discover(),
            active: "en_US".into(),
            enabled: true,
            dict: Arc::new(Mutex::new(None)),
            loading: false,
            words: HashMap::new(),
            paras: HashMap::new(),
            user,
            ignored: HashSet::new(),
        };
        sp.load();
        sp
    }

    fn load(&mut self) {
        let Some(lang) = self.langs.iter().find(|l| l.code == self.active).cloned() else { return };
        let slot = Arc::new(Mutex::new(None));
        self.dict = slot.clone();
        self.loading = true;
        self.words.clear();
        self.paras.clear();
        std::thread::spawn(move || {
            let dict = match &lang.files {
                None => Dictionary::new(BUNDLED_AFF, BUNDLED_DIC).ok(),
                Some((aff, dic)) => match (read_text(aff), read_text(dic)) {
                    (Some(a), Some(d)) => Dictionary::new(&a, &d).ok(),
                    _ => None,
                },
            };
            *slot.lock().unwrap() = dict.map(Arc::new);
        });
    }

    /// Switches language (e.g. `en_GB`); accepts `en-GB` too.
    pub fn set_language(&mut self, code: &str) {
        let code = code.replace('-', "_");
        let found = self
            .langs
            .iter()
            .find(|l| l.code.eq_ignore_ascii_case(&code))
            .or_else(|| self.langs.iter().find(|l| l.code.split('_').next() == code.split('_').next()))
            .map(|l| l.code.clone());
        if let Some(c) = found {
            if c != self.active {
                self.active = c;
                self.load();
            }
        }
    }

    pub fn active_label(&self) -> String {
        self.langs.iter().find(|l| l.code == self.active).map(|l| l.label()).unwrap_or_default()
    }

    fn dict(&mut self) -> Option<Arc<Dictionary>> {
        let d = self.dict.lock().unwrap().clone();
        if d.is_some() && self.loading {
            self.loading = false;
            self.words.clear();
            self.paras.clear();
        }
        d
    }

    pub fn is_ready(&mut self) -> bool {
        self.dict().is_some()
    }

    pub fn check_word(&mut self, w: &str) -> bool {
        if !checkable(w) || self.user.contains(w) || self.ignored.contains(w) {
            return true;
        }
        let w = w.replace('’', "'");
        if let Some(r) = self.words.get(&w) {
            return *r;
        }
        let Some(d) = self.dict() else { return true };
        let mut ok = d.check(&w);
        if !ok && w.contains('-') {
            ok = w.split('-').all(|part| part.is_empty() || !checkable(part) || d.check(part));
        }
        self.words.insert(w, ok);
        ok
    }

    /// Misspelled word ranges of a paragraph (cached by its text).
    pub fn misspelled(&mut self, p: &Para) -> Vec<(usize, usize)> {
        if !self.enabled {
            return Vec::new();
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        p.chars.hash(&mut h);
        for f in &p.fmts {
            f.hidden.hash(&mut h);
        }
        let key = h.finish();
        if let Some(v) = self.paras.get(&key) {
            return v.clone();
        }
        if self.dict().is_none() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (s, e) in words(p) {
            if p.chars[s..e].contains(&OBJ) {
                continue;
            }
            let w: String = p.chars[s..e].iter().collect();
            if !self.check_word(&w) {
                out.push((s, e));
            }
        }
        self.paras.insert(key, out.clone());
        out
    }

    pub fn suggest(&mut self, w: &str) -> Vec<String> {
        let Some(d) = self.dict() else { return Vec::new() };
        let mut out = Vec::new();
        d.suggest(&w.replace('’', "'"), &mut out);
        out.truncate(8);
        out
    }

    fn invalidate(&mut self) {
        self.words.clear();
        self.paras.clear();
    }

    pub fn ignore_all(&mut self, w: &str) {
        self.ignored.insert(w.to_string());
        self.invalidate();
    }

    pub fn add_to_dictionary(&mut self, w: &str) {
        self.user.insert(w.to_string());
        if let (Some(dir), Some(path)) = (user_dir(), user_dict_path()) {
            let _ = std::fs::create_dir_all(dir);
            let mut words: Vec<&String> = self.user.iter().collect();
            words.sort();
            let text: String = words.iter().map(|w| format!("{w}\n")).collect();
            let _ = std::fs::write(path, text);
        }
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_dictionary_checks_and_suggests() {
        let d = Dictionary::new(BUNDLED_AFF, BUNDLED_DIC).unwrap();
        assert!(d.check("hello"));
        assert!(d.check("dictionaries"));
        assert!(!d.check("helo"));
        let mut out = Vec::new();
        d.suggest("recieve", &mut out);
        assert!(out.iter().any(|s| s == "receive"), "{out:?}");
    }

    #[test]
    fn word_splitting_skips_urls_and_keeps_apostrophes() {
        let mut p = Para::new(crate::doc::Style::Normal);
        let text = "Don't visit www.example.com or mail me@x.org today";
        p.chars = text.chars().collect();
        p.fmts = vec![crate::doc::CharFmt::body(); p.chars.len()];
        let ws: Vec<String> = words(&p).iter().map(|(s, e)| p.chars[*s..*e].iter().collect()).collect();
        assert!(ws.contains(&"Don't".to_string()), "{ws:?}");
        assert!(!ws.iter().any(|w| w == "example"), "{ws:?}");
        assert!(ws.contains(&"today".to_string()));
    }
}
