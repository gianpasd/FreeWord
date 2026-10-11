//! Font discovery and resolution.
//!
//! Document fonts are referred to by a small interned id (`u16`) so that
//! `CharFmt` can stay `Copy`. System fonts are loaded from the Windows font
//! folders and registered with egui as named families, one per style variant
//! (`"Calibri|0"` regular, `|1` bold, `|2` italic, `|3` bold italic).

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

const CANDIDATES: &[(&str, [&str; 4])] = &[
    ("Calibri", ["calibri.ttf", "calibrib.ttf", "calibrii.ttf", "calibriz.ttf"]),
    ("Calibri Light", ["calibril.ttf", "", "calibrili.ttf", ""]),
    ("Arial", ["arial.ttf", "arialbd.ttf", "ariali.ttf", "arialbi.ttf"]),
    ("Times New Roman", ["times.ttf", "timesbd.ttf", "timesi.ttf", "timesbi.ttf"]),
    ("Cambria", ["cambria.ttc", "cambriab.ttf", "cambriai.ttf", "cambriaz.ttf"]),
    ("Georgia", ["georgia.ttf", "georgiab.ttf", "georgiai.ttf", "georgiaz.ttf"]),
    ("Garamond", ["GARA.TTF", "GARABD.TTF", "GARAIT.TTF", ""]),
    ("Book Antiqua", ["BKANT.TTF", "ANTQUAB.TTF", "ANTQUAI.TTF", "ANTQUABI.TTF"]),
    ("Segoe UI", ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf"]),
    ("Verdana", ["verdana.ttf", "verdanab.ttf", "verdanai.ttf", "verdanaz.ttf"]),
    ("Tahoma", ["tahoma.ttf", "tahomabd.ttf", "", ""]),
    ("Trebuchet MS", ["trebuc.ttf", "trebucbd.ttf", "trebucit.ttf", "trebucbi.ttf"]),
    ("Century Gothic", ["GOTHIC.TTF", "GOTHICB.TTF", "GOTHICI.TTF", "GOTHICBI.TTF"]),
    ("Comic Sans MS", ["comic.ttf", "comicbd.ttf", "comici.ttf", "comicz.ttf"]),
    ("Courier New", ["cour.ttf", "courbd.ttf", "couri.ttf", "courbi.ttf"]),
    ("Consolas", ["consola.ttf", "consolab.ttf", "consolai.ttf", "consolaz.ttf"]),
    ("Impact", ["impact.ttf", "", "", ""]),
];

struct Catalog {
    families: Vec<(String, [Option<FontFamily>; 4])>,
}

#[derive(Clone)]
pub struct Resolved {
    pub family: FontFamily,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

static NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static CATALOG: OnceLock<Catalog> = OnceLock::new();
static RESOLVED: OnceLock<Mutex<HashMap<(u16, u8), Resolved>>> = OnceLock::new();
static FILES: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();

/// Font file backing an egui family registered by [`install`] (used for PDF export).
pub fn font_file(family: &FontFamily) -> Option<PathBuf> {
    let key = match family {
        FontFamily::Name(n) => n.to_string(),
        _ => format!("{}|0", default_font()),
    };
    FILES.get()?.get(&key).cloned()
}

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
    names()
        .lock()
        .unwrap()
        .get(id as usize)
        .cloned()
        .unwrap_or_else(|| default_font().to_string())
}

fn catalog() -> &'static Catalog {
    CATALOG.get().expect("fonts::install must be called first")
}

/// Names of all fonts available for rendering, in menu order.
pub fn available() -> Vec<String> {
    catalog().families.iter().map(|(n, _)| n.clone()).collect()
}

pub fn is_available(name: &str) -> bool {
    catalog().families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}

pub fn default_font() -> &'static str {
    let c = catalog();
    if c.families.iter().any(|(n, _)| n == "Calibri") {
        "Calibri"
    } else {
        c.families[0].0.as_str()
    }
}

pub fn heading_font() -> &'static str {
    if is_available("Calibri Light") { "Calibri Light" } else { default_font() }
}

pub fn mono_font() -> &'static str {
    for n in ["Consolas", "Courier New", "Monospace"] {
        if is_available(n) {
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
    let c = catalog();
    let entry = c
        .families
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&name))
        .or_else(|| c.families.iter().find(|(n, _)| n == default_font()))
        .unwrap_or(&c.families[0]);
    let fams = &entry.1;
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
    let res = res.unwrap_or(Resolved {
        family: FontFamily::Proportional,
        fake_bold: bold,
        fake_italic: italic,
    });
    cache.lock().unwrap().insert(key, res.clone());
    res
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(w) = std::env::var("WINDIR") {
        dirs.push(PathBuf::from(w).join("Fonts"));
    }
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        dirs.push(PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts"));
    }
    dirs
}

fn find_font(file: &str) -> Option<PathBuf> {
    font_dirs().into_iter().map(|d| d.join(file)).find(|p| p.is_file())
}

fn read_font(file: &str) -> Option<Vec<u8>> {
    find_font(file).and_then(|p| std::fs::read(p).ok())
}

/// Loads system fonts into egui and builds the catalog. Call once at startup.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let base_fallback: Vec<String> =
        defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut extra_fallback = Vec::new();
    for (key, file) in [("Segoe UI Symbol", "seguisym.ttf"), ("Segoe UI Emoji", "seguiemj.ttf")] {
        if let Some(bytes) = read_font(file) {
            defs.font_data.insert(key.into(), Arc::new(FontData::from_owned(bytes)));
            extra_fallback.push(key.to_string());
        }
    }

    let mut paths = HashMap::new();
    let mut families = Vec::new();
    for (name, files) in CANDIDATES {
        let mut fams: [Option<FontFamily>; 4] = [None, None, None, None];
        for (v, file) in files.iter().enumerate() {
            if file.is_empty() {
                continue;
            }
            let Some(path) = find_font(file) else { continue };
            if let Ok(bytes) = std::fs::read(&path) {
                let key = format!("{name}|{v}");
                paths.insert(key.clone(), path);
                defs.font_data.insert(key.clone(), Arc::new(FontData::from_owned(bytes)));
                let fam = FontFamily::Name(key.clone().into());
                let mut chain = vec![key];
                chain.extend(extra_fallback.iter().cloned());
                chain.extend(base_fallback.iter().cloned());
                defs.families.insert(fam.clone(), chain);
                fams[v] = Some(fam);
            }
        }
        if fams[0].is_some() {
            families.push((name.to_string(), fams));
        }
    }
    // Calibri Light has no bold faces of its own; borrow Calibri's.
    if let (Some(light), Some(cal)) = (
        families.iter().position(|(n, _)| n == "Calibri Light"),
        families.iter().position(|(n, _)| n == "Calibri"),
    ) {
        families[light].1[1] = families[cal].1[1].clone();
        families[light].1[3] = families[cal].1[3].clone();
    }
    if families.is_empty() {
        families.push(("Sans".into(), [Some(FontFamily::Proportional), None, None, None]));
        families.push(("Monospace".into(), [Some(FontFamily::Monospace), None, None, None]));
    }

    // Use Segoe UI for the application chrome when present.
    if defs.font_data.contains_key("Segoe UI|0") {
        if let Some(p) = defs.families.get_mut(&FontFamily::Proportional) {
            p.insert(0, "Segoe UI|0".into());
            for k in &extra_fallback {
                p.push(k.clone());
            }
        }
    }
    ctx.set_fonts(defs);
    let _ = FILES.set(paths);
    let _ = CATALOG.set(Catalog { families });
    // Make sure the common names get stable low ids.
    for (n, _) in &catalog().families {
        font_id(n);
    }
}
