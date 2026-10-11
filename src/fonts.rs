//! Font discovery and resolution.
//!
//! Every installed font (Windows fonts, per-user fonts and Office cloud fonts
//! such as Aptos) is indexed at startup by reading only its `name`/`OS/2`
//! tables. Fonts are loaded into egui on demand, when a document or the UI
//! uses them. Document fonts are referred to by an interned id (`u16`) so that
//! `CharFmt` stays `Copy`.
//!
//! egui families are named `"{family}|{v}"` with v = 0 regular, 1 bold,
//! 2 italic, 3 bold italic.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

/// Fonts loaded eagerly so the UI and new documents never wait.
const EAGER: &[&str] = &["Calibri", "Calibri Light", "Segoe UI", "Consolas", "Aptos", "Aptos Display", "Times New Roman", "Arial", "Cambria"];

#[derive(Clone, Debug)]
pub struct Face {
    pub path: PathBuf,
    pub index: u32,
}

#[derive(Clone)]
pub struct Resolved {
    pub family: FontFamily,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

struct Catalog {
    /// Family name -> faces by variant, for every installed font.
    index: BTreeMap<String, [Option<Face>; 4]>,
    /// Families registered with egui and usable for layout.
    loaded: HashMap<String, [Option<FontFamily>; 4]>,
    /// Families registered with egui this frame (usable from the next one).
    pending: Vec<(String, [Option<FontFamily>; 4])>,
    defs: FontDefinitions,
    fallbacks: Vec<String>,
    /// egui family key -> font file, for PDF embedding.
    files: HashMap<String, Face>,
}

static NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static CATALOG: OnceLock<RwLock<Catalog>> = OnceLock::new();
static RESOLVED: OnceLock<Mutex<HashMap<(u16, u8), Resolved>>> = OnceLock::new();

fn names() -> &'static Mutex<Vec<String>> {
    NAMES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Interns a font name and returns its id.
pub fn font_id(name: &str) -> u16 {
    let mut v = names().lock().unwrap();
    if let Some(i) = v.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        return i as u16;
    }
    v.push(name.to_string());
    (v.len() - 1) as u16
}

pub fn font_name(id: u16) -> String {
    names().lock().unwrap().get(id as usize).cloned().unwrap_or_else(|| default_font().to_string())
}

fn catalog() -> &'static RwLock<Catalog> {
    // Without `install` (tests, headless tools) fall back to egui's built-in fonts.
    CATALOG.get_or_init(|| {
        let mut loaded = HashMap::new();
        loaded.insert("Calibri".to_string(), [Some(FontFamily::Proportional), None, None, None]);
        loaded.insert("Consolas".to_string(), [Some(FontFamily::Monospace), None, None, None]);
        RwLock::new(Catalog {
            index: BTreeMap::new(),
            loaded,
            pending: Vec::new(),
            defs: FontDefinitions::default(),
            fallbacks: Vec::new(),
            files: HashMap::new(),
        })
    })
}

/// Names of all installed font families, for the font menu.
pub fn available() -> Vec<String> {
    let c = catalog().read().unwrap();
    if c.index.is_empty() {
        let mut v: Vec<String> = c.loaded.keys().cloned().collect();
        v.sort();
        return v;
    }
    c.index.keys().filter(|k| !k.starts_with('@')).cloned().collect()
}

/// Whether a family is ready for rendering.
pub fn is_loaded(name: &str) -> bool {
    let c = catalog().read().unwrap();
    c.loaded.keys().any(|n| n.eq_ignore_ascii_case(name))
}

pub fn is_available(name: &str) -> bool {
    let c = catalog().read().unwrap();
    c.loaded.keys().any(|n| n.eq_ignore_ascii_case(name)) || c.index.keys().any(|n| n.eq_ignore_ascii_case(name))
}

pub fn default_font() -> &'static str {
    let c = catalog().read().unwrap();
    if c.loaded.contains_key("Calibri") {
        "Calibri"
    } else if c.loaded.contains_key("Aptos") {
        "Aptos"
    } else {
        "Calibri"
    }
}

pub fn heading_font() -> &'static str {
    if is_loaded("Calibri Light") { "Calibri Light" } else { default_font() }
}

pub fn mono_font() -> &'static str {
    for n in ["Consolas", "Courier New"] {
        if is_loaded(n) {
            return n;
        }
    }
    default_font()
}

/// Egui family (plus synthetic-style flags) used to render `font` with the given style.
pub fn resolve(font: u16, bold: bool, italic: bool) -> Resolved {
    let key = (font, bold as u8 | ((italic as u8) << 1));
    let cache = RESOLVED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(r) = cache.lock().unwrap().get(&key) {
        return r.clone();
    }
    let name = font_name(font);
    let c = catalog().read().unwrap();
    let fams = c
        .loaded
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        .or_else(|| c.loaded.get_key_value(default_font()))
        .or_else(|| c.loaded.iter().next())
        .map(|(_, f)| f.clone())
        .unwrap_or([Some(FontFamily::Proportional), None, None, None]);
    let want = key.1 as usize;
    // Preference order of (variant, fake_bold, fake_italic) when the wanted face is missing.
    let order: &[(usize, bool, bool)] = match want {
        3 => &[(3, false, false), (1, false, true), (2, true, false), (0, true, true)],
        1 => &[(1, false, false), (0, true, false)],
        2 => &[(2, false, false), (0, false, true)],
        _ => &[(0, false, false)],
    };
    let mut res = None;
    for &(v, fb, fi) in order {
        if let Some(f) = &fams[v] {
            res = Some(Resolved { family: f.clone(), fake_bold: fb, fake_italic: fi });
            break;
        }
    }
    let res = res.unwrap_or(Resolved { family: FontFamily::Proportional, fake_bold: bold, fake_italic: italic });
    drop(c);
    cache.lock().unwrap().insert(key, res.clone());
    res
}

/// Font file backing an egui family (used for PDF embedding).
pub fn font_file(family: &FontFamily) -> Option<Face> {
    let key = match family {
        FontFamily::Name(n) => n.to_string(),
        _ => format!("{}|0", default_font()),
    };
    catalog().read().unwrap().files.get(&key).cloned()
}

// ---------------------------------------------------------------- discovery

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(w) = std::env::var("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        let l = PathBuf::from(l);
        dirs.push(l.join("Microsoft").join("Windows").join("Fonts"));
        // Office cloud fonts (Aptos & co.) live in one folder per family.
        if let Ok(rd) = std::fs::read_dir(l.join("Microsoft").join("FontCache").join("4").join("CloudFonts")) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    dirs.push(e.path());
                }
            }
        }
    }
    for d in ["/usr/share/fonts", "/usr/local/share/fonts", "/Library/Fonts", "/System/Library/Fonts"] {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

fn be16(b: &[u8], o: usize) -> u16 {
    b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).unwrap_or(0)
}

fn be32(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).unwrap_or(0)
}

fn read_at(f: &mut std::fs::File, off: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(off)).ok()?;
    let mut v = vec![0u8; len];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

/// (family, bold, italic) of the face at `offset` in a font file.
fn face_info(f: &mut std::fs::File, offset: u64) -> Option<(String, bool, bool)> {
    let head = read_at(f, offset, 12)?;
    let num_tables = be16(&head, 4) as usize;
    let dir = read_at(f, offset + 12, num_tables * 16)?;
    let mut name_tbl = None;
    let mut os2 = None;
    for i in 0..num_tables {
        let rec = &dir[i * 16..i * 16 + 16];
        let tag = &rec[0..4];
        let (off, len) = (be32(rec, 8) as u64, be32(rec, 12) as usize);
        match tag {
            b"name" => name_tbl = Some((off, len)),
            b"OS/2" => os2 = Some((off, len)),
            _ => {}
        }
    }
    let (noff, nlen) = name_tbl?;
    let nb = read_at(f, noff, nlen.min(1 << 20))?;
    let count = be16(&nb, 2) as usize;
    let storage = be16(&nb, 4) as usize;
    let mut family = None;
    let mut typo_family = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, encoding, lang, name_id, len, off) =
            (be16(&nb, r), be16(&nb, r + 2), be16(&nb, r + 4), be16(&nb, r + 6), be16(&nb, r + 8) as usize, be16(&nb, r + 10) as usize);
        if platform != 3 || !(encoding == 1 || encoding == 0) {
            continue;
        }
        let Some(raw) = nb.get(storage + off..storage + off + len) else { continue };
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        let s = String::from_utf16_lossy(&units);
        // Prefer US English names.
        let better = |cur: &Option<(String, u16)>| cur.as_ref().is_none_or(|(_, l)| *l != 0x409 && lang == 0x409);
        match name_id {
            1 if better(&family) => family = Some((s, lang)),
            16 if better(&typo_family) => typo_family = Some((s, lang)),
            _ => {}
        }
    }
    let (mut bold, mut italic) = (false, false);
    if let Some((ooff, olen)) = os2 {
        if let Some(ob) = read_at(f, ooff, olen.min(100)) {
            let sel = be16(&ob, 62);
            italic = sel & 0x01 != 0;
            bold = sel & 0x20 != 0;
        }
    }
    // Name 1 is the style-linked family Word uses ("Arial Black", "Segoe UI Semibold"...).
    let fam = family.or(typo_family)?.0;
    Some((fam, bold, italic))
}

fn scan_file(path: &PathBuf, index: &mut BTreeMap<String, [Option<Face>; 4]>) {
    let Ok(mut f) = std::fs::File::open(path) else { return };
    let Some(head) = read_at(&mut f, 0, 16) else { return };
    let offsets: Vec<(u32, u64)> = if &head[0..4] == b"ttcf" {
        let n = be32(&head, 8).min(64) as usize;
        let Some(tbl) = read_at(&mut f, 12, n * 4) else { return };
        (0..n).map(|i| (i as u32, be32(&tbl, i * 4) as u64)).collect()
    } else {
        vec![(0, 0)]
    };
    for (index_in_file, off) in offsets {
        let Some((fam, bold, italic)) = face_info(&mut f, off) else { continue };
        let v = bold as usize | ((italic as usize) << 1);
        let entry = index.entry(fam).or_insert([None, None, None, None]);
        if entry[v].is_none() {
            entry[v] = Some(Face { path: path.clone(), index: index_in_file });
        }
    }
}

fn build_index() -> BTreeMap<String, [Option<Face>; 4]> {
    let mut index = BTreeMap::new();
    for dir in font_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if matches!(ext.as_str(), "ttf" | "otf" | "ttc") {
                scan_file(&p, &mut index);
            }
        }
    }
    // Only families with a regular face are usable.
    index.retain(|_, v| v[0].is_some());
    index
}

/// Registers the faces of one family in the font definitions (no-op if unknown).
fn register(c: &mut Catalog, name: &str) -> bool {
    let Some((real, faces)) = c.index.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, f)| (n.clone(), f.clone()))
    else {
        return false;
    };
    let mut fams: [Option<FontFamily>; 4] = [None, None, None, None];
    for (v, face) in faces.iter().enumerate() {
        let Some(face) = face else { continue };
        let Ok(bytes) = std::fs::read(&face.path) else { continue };
        let key = format!("{real}|{v}");
        let mut data = FontData::from_owned(bytes);
        data.index = face.index;
        c.defs.font_data.insert(key.clone(), Arc::new(data));
        let mut chain = vec![key.clone()];
        chain.extend(c.fallbacks.iter().cloned());
        let fam = FontFamily::Name(key.clone().into());
        c.defs.families.insert(fam.clone(), chain);
        c.files.insert(key, face.clone());
        fams[v] = Some(fam);
    }
    if fams[0].is_none() {
        return false;
    }
    c.pending.push((real, fams));
    true
}

/// Loads system fonts into egui and builds the catalog. Call once at startup.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let builtin: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut fallbacks: Vec<String> = Vec::new();
    let index = build_index();
    let mut c = Catalog {
        index,
        loaded: HashMap::new(),
        pending: Vec::new(),
        defs: FontDefinitions::default(),
        fallbacks: Vec::new(),
        files: HashMap::new(),
    };
    // Symbol fallbacks for glyphs missing from document fonts.
    for fam in ["Segoe UI Symbol", "Segoe UI Emoji"] {
        if let Some(face) = c.index.get(fam).and_then(|f| f[0].clone()) {
            if let Ok(bytes) = std::fs::read(&face.path) {
                let mut data = FontData::from_owned(bytes);
                data.index = face.index;
                defs.font_data.insert(fam.into(), Arc::new(data));
                fallbacks.push(fam.into());
            }
        }
    }
    fallbacks.extend(builtin);
    c.fallbacks = fallbacks;
    c.defs = defs;
    for name in EAGER {
        register(&mut c, name);
    }
    // Calibri Light has no bold faces of its own; borrow Calibri's.
    let calibri_bold = c.pending.iter().find(|(n, _)| n == "Calibri").map(|(_, f)| (f[1].clone(), f[3].clone()));
    if let (Some(light), Some((b, bi))) = (c.pending.iter_mut().find(|(n, _)| n == "Calibri Light"), calibri_bold) {
        light.1[1] = b;
        light.1[3] = bi;
    }
    // Use Segoe UI for the application chrome when present.
    if c.defs.font_data.contains_key("Segoe UI|0") {
        if let Some(p) = c.defs.families.get_mut(&FontFamily::Proportional) {
            p.insert(0, "Segoe UI|0".into());
            for k in ["Segoe UI Symbol", "Segoe UI Emoji"] {
                if c.defs.font_data.contains_key(k) {
                    p.push(k.into());
                }
            }
        }
    }
    // Eagerly loaded fonts are usable immediately: the definitions are set before the first frame.
    let pending = std::mem::take(&mut c.pending);
    for (n, f) in pending {
        c.loaded.insert(n, f);
    }
    if c.loaded.is_empty() {
        c.loaded.insert("Calibri".into(), [Some(FontFamily::Proportional), None, None, None]);
        c.loaded.insert("Consolas".into(), [Some(FontFamily::Monospace), None, None, None]);
    }
    ctx.set_fonts(c.defs.clone());
    match CATALOG.get() {
        Some(lock) => *lock.write().unwrap() = c,
        None => {
            let _ = CATALOG.set(RwLock::new(c));
        }
    }
    if let Some(r) = RESOLVED.get() {
        r.lock().unwrap().clear();
    }
    for n in available() {
        font_id(&n);
    }
}

/// Requests fonts used by a document; returns true if new fonts were registered
/// (they become usable after [`commit`] on the next frame).
pub fn request(ctx: &egui::Context, families: impl IntoIterator<Item = String>) -> bool {
    let mut c = catalog().write().unwrap();
    let mut added = false;
    for name in families {
        let known = c.loaded.keys().any(|n| n.eq_ignore_ascii_case(&name))
            || c.pending.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name));
        if !known && register(&mut c, &name) {
            added = true;
        }
    }
    if added {
        ctx.set_fonts(c.defs.clone());
    }
    added
}

/// Makes fonts registered in the previous frame usable. Returns true if any were.
pub fn commit() -> bool {
    let mut c = catalog().write().unwrap();
    if c.pending.is_empty() {
        return false;
    }
    let pending = std::mem::take(&mut c.pending);
    for (n, f) in pending {
        c.loaded.insert(n, f);
    }
    drop(c);
    if let Some(r) = RESOLVED.get() {
        r.lock().unwrap().clear();
    }
    true
}

/// All font names referenced by a document (body, headers/footers).
pub fn used_by(doc: &crate::doc::Document) -> Vec<String> {
    let mut ids = std::collections::BTreeSet::new();
    let mut visit = |d: &crate::doc::Document| {
        for p in &d.paras {
            ids.insert(p.end_fmt.font);
            for f in &p.fmts {
                ids.insert(f.font);
            }
        }
    };
    visit(doc);
    for d in doc.hf_docs() {
        visit(d);
    }
    ids.into_iter().map(font_name).collect()
}
