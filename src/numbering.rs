//! Word list numbering (`numbering.xml`): definitions and label computation.

use crate::xml::{El, esc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum NumFmt {
    Decimal,
    DecimalZero,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    Ordinal,
    Bullet,
    None,
}

impl NumFmt {
    fn parse(s: &str) -> NumFmt {
        match s {
            "decimalZero" => NumFmt::DecimalZero,
            "lowerLetter" => NumFmt::LowerLetter,
            "upperLetter" => NumFmt::UpperLetter,
            "lowerRoman" => NumFmt::LowerRoman,
            "upperRoman" => NumFmt::UpperRoman,
            "ordinal" => NumFmt::Ordinal,
            "bullet" => NumFmt::Bullet,
            "none" => NumFmt::None,
            _ => NumFmt::Decimal,
        }
    }

    fn name(self) -> &'static str {
        match self {
            NumFmt::Decimal => "decimal",
            NumFmt::DecimalZero => "decimalZero",
            NumFmt::LowerLetter => "lowerLetter",
            NumFmt::UpperLetter => "upperLetter",
            NumFmt::LowerRoman => "lowerRoman",
            NumFmt::UpperRoman => "upperRoman",
            NumFmt::Ordinal => "ordinal",
            NumFmt::Bullet => "bullet",
            NumFmt::None => "none",
        }
    }

    fn format(self, n: u32) -> String {
        match self {
            NumFmt::Decimal => n.to_string(),
            NumFmt::DecimalZero => format!("{n:02}"),
            NumFmt::LowerLetter => alpha(n),
            NumFmt::UpperLetter => alpha(n).to_uppercase(),
            NumFmt::LowerRoman => roman(n),
            NumFmt::UpperRoman => roman(n).to_uppercase(),
            NumFmt::Ordinal => {
                let suf = match (n % 10, n % 100) {
                    (_, 11..=13) => "th",
                    (1, _) => "st",
                    (2, _) => "nd",
                    (3, _) => "rd",
                    _ => "th",
                };
                format!("{n}{suf}")
            }
            NumFmt::Bullet | NumFmt::None => String::new(),
        }
    }
}

fn alpha(n: u32) -> String {
    if n == 0 {
        return String::new();
    }
    // Word repeats the letter: a..z, aa..zz, aaa..
    let c = (b'a' + ((n - 1) % 26) as u8) as char;
    c.to_string().repeat(((n - 1) / 26 + 1) as usize)
}

fn roman(mut n: u32) -> String {
    let table = [
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
        (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Level {
    pub fmt: NumFmt,
    pub text: String,
    pub start: u32,
    /// Left indent and hanging indent in points.
    pub left: f32,
    pub hanging: f32,
    pub font: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Abstract {
    pub levels: Vec<Level>,
    #[serde(default)]
    pub style_link: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Num {
    pub abs: u32,
    #[serde(default)]
    pub starts: BTreeMap<u8, u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Numbering {
    pub abstracts: BTreeMap<u32, Abstract>,
    pub nums: BTreeMap<u32, Num>,
    /// Definitions created in this session (to be added to an existing numbering.xml).
    #[serde(default)]
    pub new_abstracts: BTreeSet<u32>,
    #[serde(default)]
    pub new_nums: BTreeSet<u32>,
}

/// Maps symbol-font private-use bullet characters to Unicode.
fn bullet_glyph(text: &str, font: Option<&str>) -> String {
    let mut chars = text.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return text.to_string();
    };
    let code = c as u32;
    let font = font.unwrap_or("").to_ascii_lowercase();
    let low = if (0xF000..=0xF0FF).contains(&code) { code - 0xF000 } else { code };
    if font.contains("wingdings") {
        return match low {
            0xA7 => "▪",
            0xD8 => "➢",
            0xFC => "✓",
            0x76 => "❖",
            0x71 => "❑",
            0x6E => "■",
            0x6C => "●",
            0xA8 => "◆",
            0xA1 => "○",
            0x9F => "•",
            _ => "•",
        }
        .into();
    }
    if font.contains("symbol") {
        return match low {
            0xB7 => "•".into(),
            0x2D => "–".into(),
            _ => char::from_u32(low).map(|c| c.to_string()).unwrap_or_else(|| "•".into()),
        };
    }
    if c == 'o' && font.contains("courier") {
        return "◦".into();
    }
    if (0xF000..=0xF0FF).contains(&code) {
        return "•".into();
    }
    c.to_string()
}

fn twips(e: &El, key: &str) -> Option<f32> {
    e.attr(key).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 20.0)
}

impl Level {
    fn parse(l: &El) -> Level {
        let fmt = l.child_val("numFmt").map(NumFmt::parse).unwrap_or(NumFmt::Decimal);
        let text = l.child_val("lvlText").unwrap_or("").to_string();
        let start = l.child_val("start").and_then(|v| v.parse().ok()).unwrap_or(1);
        let mut left = 0.0;
        let mut hanging = 0.0;
        if let Some(ind) = l.child("pPr").and_then(|p| p.child("ind")) {
            left = twips(ind, "left").or(twips(ind, "start")).unwrap_or(0.0);
            hanging = twips(ind, "hanging").unwrap_or(0.0);
            if let Some(f) = twips(ind, "firstLine") {
                hanging = -f;
            }
        }
        let font = l
            .child("rPr")
            .and_then(|r| r.child("rFonts"))
            .and_then(|f| f.attr("ascii").or(f.attr("hAnsi")))
            .map(|s| s.to_string());
        Level { fmt, text, start, left, hanging, font }
    }

    fn to_xml(&self, ilvl: usize) -> String {
        let font = match &self.font {
            Some(f) => format!("<w:rPr><w:rFonts w:ascii=\"{0}\" w:hAnsi=\"{0}\" w:hint=\"default\"/></w:rPr>", esc(f)),
            None => String::new(),
        };
        format!(
            "<w:lvl w:ilvl=\"{ilvl}\"><w:start w:val=\"{}\"/><w:numFmt w:val=\"{}\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"{}\"/></w:pPr>{font}</w:lvl>",
            self.start,
            self.fmt.name(),
            esc(&self.text),
            (self.left * 20.0).round() as i32,
            (self.hanging * 20.0).round() as i32,
        )
    }
}

fn bullet_levels() -> Vec<Level> {
    (0..9)
        .map(|i| Level {
            fmt: NumFmt::Bullet,
            text: ["•", "◦", "▪"][i % 3].to_string(),
            start: 1,
            left: 36.0 * (i as f32 + 1.0),
            hanging: 18.0,
            font: None,
        })
        .collect()
}

fn number_levels() -> Vec<Level> {
    (0..9)
        .map(|i| Level {
            fmt: [NumFmt::Decimal, NumFmt::LowerLetter, NumFmt::LowerRoman][i % 3],
            text: format!("%{}.", i + 1),
            start: 1,
            left: 36.0 * (i as f32 + 1.0),
            hanging: 18.0,
            font: None,
        })
        .collect()
}

impl Numbering {
    /// Numbering for a brand-new document.
    pub fn new_default() -> Self {
        let mut n = Numbering::default();
        n.bullet_num();
        n
    }

    pub fn parse(root: &El) -> Self {
        let mut n = Numbering::default();
        for a in root.elems().filter(|e| e.name == "abstractNum") {
            let Some(id) = a.attr("abstractNumId").and_then(|v| v.parse().ok()) else { continue };
            let mut levels: Vec<Level> = number_levels();
            for l in a.elems().filter(|e| e.name == "lvl") {
                let il: usize = l.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                if il < 9 {
                    levels[il] = Level::parse(l);
                }
            }
            let style_link = a.child_val("numStyleLink").map(|s| s.to_string());
            n.abstracts.insert(id, Abstract { levels, style_link });
        }
        for num in root.elems().filter(|e| e.name == "num") {
            let Some(id) = num.attr("numId").and_then(|v| v.parse().ok()) else { continue };
            let abs = num.child_val("abstractNumId").and_then(|v| v.parse().ok()).unwrap_or(0);
            let mut starts = BTreeMap::new();
            for o in num.elems().filter(|e| e.name == "lvlOverride") {
                let il: u8 = o.attr("ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                if let Some(s) = o.child_val("startOverride").and_then(|v| v.parse().ok()) {
                    starts.insert(il, s);
                }
            }
            n.nums.insert(id, Num { abs, starts });
        }
        n
    }

    /// Resolves `numStyleLink` indirection given a map from numbering style id to numId.
    pub fn resolve_style_links(&mut self, style_nums: &HashMap<String, u32>) {
        let links: Vec<(u32, u32)> = self
            .nums
            .iter()
            .filter_map(|(id, num)| {
                let link = self.abstracts.get(&num.abs)?.style_link.clone()?;
                let target_num = style_nums.get(&link)?;
                let target_abs = self.nums.get(target_num)?.abs;
                (target_abs != num.abs).then_some((*id, target_abs))
            })
            .collect();
        for (id, abs) in links {
            if let Some(n) = self.nums.get_mut(&id) {
                n.abs = abs;
            }
        }
    }

    pub fn level(&self, num: u32, ilvl: u8) -> Option<&Level> {
        let n = self.nums.get(&num)?;
        self.abstracts.get(&n.abs)?.levels.get(ilvl.min(8) as usize)
    }

    pub fn is_bullet(&self, num: u32, ilvl: u8) -> bool {
        self.level(num, ilvl).is_some_and(|l| l.fmt == NumFmt::Bullet)
    }

    fn next_abs_id(&self) -> u32 {
        self.abstracts.keys().next_back().map(|k| k + 1).unwrap_or(1).max(100)
    }

    fn next_num_id(&self) -> u32 {
        self.nums.keys().next_back().map(|k| k + 1).unwrap_or(1)
    }

    /// A num instance for plain bullet lists (created if needed).
    pub fn bullet_num(&mut self) -> u32 {
        for (id, n) in &self.nums {
            if self.new_nums.contains(id) || n.starts.is_empty() {
                if let Some(a) = self.abstracts.get(&n.abs) {
                    if a.levels[0].fmt == NumFmt::Bullet && a.levels[0].text == "•" {
                        return *id;
                    }
                }
            }
        }
        let abs = self.next_abs_id();
        self.abstracts.insert(abs, Abstract { levels: bullet_levels(), style_link: None });
        self.new_abstracts.insert(abs);
        let id = self.next_num_id();
        self.nums.insert(id, Num { abs, starts: BTreeMap::new() });
        self.new_nums.insert(id);
        id
    }

    /// A new numbered list instance that starts at 1.
    pub fn new_numbered(&mut self) -> u32 {
        let abs = match self
            .new_abstracts
            .iter()
            .find(|a| self.abstracts.get(a).is_some_and(|x| x.levels[0].fmt == NumFmt::Decimal))
        {
            Some(a) => *a,
            None => {
                let a = self.next_abs_id();
                self.abstracts.insert(a, Abstract { levels: number_levels(), style_link: None });
                self.new_abstracts.insert(a);
                a
            }
        };
        let id = self.next_num_id();
        let mut starts = BTreeMap::new();
        starts.insert(0, 1);
        self.nums.insert(id, Num { abs, starts });
        self.new_nums.insert(id);
        id
    }

    /// Computes list labels for (num id, level) items in document order.
    pub fn labels<'a>(&self, items: impl Iterator<Item = Option<(u32, u8)>> + 'a) -> Vec<Option<String>> {
        let mut counters: HashMap<u32, ([u32; 9], [bool; 9])> = HashMap::new();
        let mut seen: HashSet<u32> = HashSet::new();
        let mut out = Vec::new();
        for it in items {
            let Some((num_id, ilvl)) = it else {
                out.push(None);
                continue;
            };
            let Some(num) = self.nums.get(&num_id) else {
                out.push(None);
                continue;
            };
            let Some(abs) = self.abstracts.get(&num.abs) else {
                out.push(None);
                continue;
            };
            let il = (ilvl as usize).min(8);
            let (c, started) = counters.entry(num.abs).or_insert(([0; 9], [false; 9]));
            if seen.insert(num_id) {
                for (l, s) in &num.starts {
                    let l = (*l as usize).min(8);
                    c[l] = s.saturating_sub(1);
                    started[l] = true;
                }
            }
            if !started[il] {
                c[il] = abs.levels[il].start.saturating_sub(1);
                started[il] = true;
            }
            c[il] += 1;
            for s in started.iter_mut().skip(il + 1) {
                *s = false;
            }
            let lvl = &abs.levels[il];
            let label = if lvl.fmt == NumFmt::Bullet {
                bullet_glyph(&lvl.text, lvl.font.as_deref())
            } else if lvl.fmt == NumFmt::None {
                String::new()
            } else {
                let mut s = String::new();
                let mut chars = lvl.text.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch == '%' {
                        if let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                            chars.next();
                            let k = (d as usize).saturating_sub(1).min(8);
                            let v = if k == il { c[k] } else { c[k].max(1) };
                            s.push_str(&abs.levels[k].fmt.format(v));
                            continue;
                        }
                    }
                    s.push(ch);
                }
                s
            };
            out.push(Some(label));
        }
        out
    }

    fn abstract_xml(&self, id: u32) -> String {
        let a = &self.abstracts[&id];
        let mut s = format!("<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>");
        for (i, l) in a.levels.iter().enumerate() {
            s.push_str(&l.to_xml(i));
        }
        s.push_str("</w:abstractNum>");
        s
    }

    fn num_xml(&self, id: u32) -> String {
        let n = &self.nums[&id];
        let mut s = format!("<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{}\"/>", n.abs);
        for (l, st) in &n.starts {
            s.push_str(&format!("<w:lvlOverride w:ilvl=\"{l}\"><w:startOverride w:val=\"{st}\"/></w:lvlOverride>"));
        }
        s.push_str("</w:num>");
        s
    }

    /// Complete numbering.xml from the in-memory model.
    pub fn to_xml(&self) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
        );
        for id in self.abstracts.keys() {
            s.push_str(&self.abstract_xml(*id));
        }
        for id in self.nums.keys() {
            s.push_str(&self.num_xml(*id));
        }
        s.push_str("</w:numbering>");
        s
    }

    /// Adds definitions created in this session to an existing numbering.xml.
    pub fn patch_xml(&self, original: &str) -> String {
        if self.new_abstracts.is_empty() && self.new_nums.is_empty() {
            return original.to_string();
        }
        let abs: String = self.new_abstracts.iter().map(|id| self.abstract_xml(*id)).collect();
        let nums: String = self.new_nums.iter().map(|id| self.num_xml(*id)).collect();
        let mut s = original.to_string();
        // abstractNum elements must precede num elements.
        let abs_at = s.find("<w:num ").or_else(|| s.find("<w:num>")).or_else(|| s.find("<w:numIdMacAtCleanup")).or_else(|| s.rfind("</w:numbering>"));
        if let Some(i) = abs_at {
            s.insert_str(i, &abs);
        }
        let num_at = s.find("<w:numIdMacAtCleanup").or_else(|| s.rfind("</w:numbering>"));
        if let Some(i) = num_at {
            s.insert_str(i, &nums);
        }
        s
    }
}
