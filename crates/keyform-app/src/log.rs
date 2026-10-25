//! Messages shown in the Log palette and the status bar.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Default)]
pub struct Log {
    pub entries: Vec<(Level, String)>,
    /// Latest message for the status bar.
    pub status: String,
}

impl Log {
    pub fn push(&mut self, level: Level, msg: impl Into<String>) {
        let msg = msg.into();
        self.status.clone_from(&msg);
        self.entries.push((level, msg));
        if self.entries.len() > 2000 {
            self.entries.drain(..500);
        }
    }
    pub fn info(&mut self, msg: impl Into<String>) {
        self.push(Level::Info, msg);
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.push(Level::Warning, msg);
    }
    pub fn error(&mut self, msg: impl Into<String>) {
        self.push(Level::Error, msg);
    }
}
