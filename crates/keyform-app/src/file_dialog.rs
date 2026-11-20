//! File dialogs: the platform dialog (rfd), or — for scripted UI tests on a
//! display without a desktop portal — answers taken from a file.
//!
//! With `KEYFORM_FILE_DIALOG=/path/answers.txt`, every dialog takes the
//! first line of that file as the chosen path (an empty line = Cancel) and
//! removes it; a missing or empty file also means Cancel. The end-to-end
//! test writes the answer just before it clicks the menu item that opens
//! the dialog. Keyform's own UI is still driven by real input.

use std::path::{Path, PathBuf};

pub struct FileDialog {
    inner: rfd::FileDialog,
}

impl FileDialog {
    pub fn new() -> Self {
        Self {
            inner: rfd::FileDialog::new(),
        }
    }

    pub fn add_filter(self, name: impl Into<String>, extensions: &[impl ToString]) -> Self {
        Self {
            inner: self.inner.add_filter(name, extensions),
        }
    }

    pub fn set_file_name(self, name: impl Into<String>) -> Self {
        Self {
            inner: self.inner.set_file_name(name),
        }
    }

    pub fn set_title(self, title: impl Into<String>) -> Self {
        Self {
            inner: self.inner.set_title(title),
        }
    }

    pub fn set_directory<P: AsRef<Path>>(self, dir: P) -> Self {
        Self {
            inner: self.inner.set_directory(dir),
        }
    }

    pub fn pick_file(self) -> Option<PathBuf> {
        match scripted() {
            Some(a) => a,
            None => self.inner.pick_file(),
        }
    }

    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        match scripted() {
            Some(a) => a.map(|p| vec![p]),
            None => self.inner.pick_files(),
        }
    }

    pub fn save_file(self) -> Option<PathBuf> {
        match scripted() {
            Some(a) => a,
            None => self.inner.save_file(),
        }
    }

    pub fn pick_folder(self) -> Option<PathBuf> {
        match scripted() {
            Some(a) => a,
            None => self.inner.pick_folder(),
        }
    }
}

/// `Some(answer)` when dialogs are scripted (`None` inside = Cancel). A
/// test hook: never in release builds.
fn scripted() -> Option<Option<PathBuf>> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let file = std::env::var_os("KEYFORM_FILE_DIALOG")?;
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim().to_owned();
    let rest: Vec<&str> = lines.collect();
    let _ = std::fs::write(
        &file,
        if rest.is_empty() {
            String::new()
        } else {
            rest.join("\n") + "\n"
        },
    );
    Some((!first.is_empty()).then(|| PathBuf::from(first)))
}

#[cfg(test)]
mod tests {
    /// Scripted answers are a test hook: off unless KEYFORM_FILE_DIALOG is
    /// set (one test, so the variable isn't raced by a parallel test).
    #[cfg(debug_assertions)]
    #[test]
    fn answers_are_consumed_in_order() {
        if std::env::var_os("KEYFORM_FILE_DIALOG").is_none() {
            assert_eq!(super::scripted(), None, "off by default");
        }
        let f = std::env::temp_dir().join(format!("kf-dialog-{}.txt", std::process::id()));
        std::fs::write(&f, "/a/one.psd\n\n/b/two.cmo3\n").unwrap();
        std::env::set_var("KEYFORM_FILE_DIALOG", &f);
        assert_eq!(super::scripted(), Some(Some("/a/one.psd".into())));
        assert_eq!(super::scripted(), Some(None));
        assert_eq!(super::scripted(), Some(Some("/b/two.cmo3".into())));
        assert_eq!(super::scripted(), Some(None));
        std::env::remove_var("KEYFORM_FILE_DIALOG");
        let _ = std::fs::remove_file(&f);
    }

    /// Release builds never script dialogs (run with `cargo test --release`).
    #[cfg(not(debug_assertions))]
    #[test]
    fn ignored_in_release_builds() {
        let f = std::env::temp_dir().join(format!("kf-dialog-rel-{}.txt", std::process::id()));
        std::fs::write(&f, "/a/one.psd\n").unwrap();
        std::env::set_var("KEYFORM_FILE_DIALOG", &f);
        let got = super::scripted();
        std::env::remove_var("KEYFORM_FILE_DIALOG");
        let _ = std::fs::remove_file(&f);
        assert_eq!(got, None);
    }
}
