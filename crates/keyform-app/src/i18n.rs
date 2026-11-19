//! UI languages: English source strings are the keys; each other language
//! has a table of Keyform's own translations (`i18n/<code>.tsv`, one
//! "English<TAB>translation" per line). Missing entries show English.
//! spec: 50-ui/inspector-fields.md ("localise them through your own string
//! tables"), 50-ui/dialogs.md §11 (Language), 50-ui/theme.md §3 (CJK fonts)

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Lang {
    #[default]
    En,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ja];

    /// The language's own name (shown untranslated in the menu).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ja => "日本語",
        }
    }

    fn table_text(self) -> &'static str {
        match self {
            Lang::En => "",
            Lang::Ja => include_str!("../i18n/ja.tsv"),
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

pub fn current() -> Lang {
    match CURRENT.load(Ordering::Relaxed) {
        1 => Lang::Ja,
        _ => Lang::En,
    }
}

/// `\n`, `\t`, `\"` and `\\` escapes (keys mirror Rust string literals).
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// Parses a table; lines starting with `#` and blank lines are skipped.
fn parse(text: &'static str) -> HashMap<String, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .filter(|(_, t)| !t.is_empty())
        .map(|(k, v)| (unescape(k), unescape(v)))
        .collect()
}

fn table(lang: Lang) -> &'static HashMap<String, String> {
    static JA: OnceLock<HashMap<String, String>> = OnceLock::new();
    static EMPTY: OnceLock<HashMap<String, String>> = OnceLock::new();
    match lang {
        Lang::Ja => JA.get_or_init(|| parse(Lang::Ja.table_text())),
        Lang::En => EMPTY.get_or_init(HashMap::new),
    }
}

/// The UI string `s` in the current language.
pub fn tr(s: &str) -> &str {
    match current() {
        Lang::En => s,
        l => table(l).get(s).map_or(s, String::as_str),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_parse_without_duplicates() {
        for l in Lang::ALL {
            let text = l.table_text();
            let mut seen = std::collections::HashSet::new();
            for line in text
                .lines()
                .filter(|x| !x.trim().is_empty() && !x.starts_with('#'))
            {
                let (k, v) = line
                    .split_once('\t')
                    .unwrap_or_else(|| panic!("{l:?}: no tab in {line:?}"));
                assert!(seen.insert(k), "{l:?}: duplicate {k:?}");
                assert!(!v.trim().is_empty(), "{l:?}: empty translation for {k:?}");
            }
        }
        // English passes through; Japanese translates known strings and
        // falls back for unknown ones.
        set(Lang::Ja);
        assert_eq!(tr("File"), "ファイル");
        assert_eq!(tr("No such string anywhere"), "No such string anywhere");
        // Escaped keys match the Rust literals (newlines, quotes).
        assert!(tr("Extended interpolation requires at least 3 keyforms.\nWhen setting extended interpolation and changing to 2 keyforms or less, please select Linear Interpolation.").contains('\n'));
        assert!(!tr("Deformer generated successfully. Please specify each deformer and click the \"Generate Motion\" button to continue.").starts_with("Deformer"));
        set(Lang::En);
        assert_eq!(tr("File"), "File");
    }
}
