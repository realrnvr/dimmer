//! In-memory log ring buffer shown on the Debug page.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use dimmer_core::time::now_string;

const MAX_ENTRIES: usize = 500;

/// Thread-safe, bounded application log.
#[derive(Debug)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<String>>,
    verbose: AtomicBool,
}

impl LogBuffer {
    #[must_use]
    pub fn new(verbose: bool) -> Self {
        Self { entries: Mutex::new(VecDeque::with_capacity(64)), verbose: AtomicBool::new(verbose) }
    }

    pub fn info(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if self.verbose.load(Ordering::Relaxed) {
            eprintln!("[dimmer] {message}");
        }
        self.push("INFO", message);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        eprintln!("[dimmer] warning: {message}");
        self.push("WARN", message);
    }

    pub fn error(&self, message: impl AsRef<str>) {
        let message = message.as_ref();
        eprintln!("[dimmer] error: {message}");
        self.push("ERROR", message);
    }

    fn push(&self, level: &str, message: &str) {
        let line = format!("{} {level:<5} {message}", now_string());
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() == MAX_ENTRIES {
                entries.pop_front();
            }
            entries.push_back(line);
        }
    }

    /// Returns the log contents, oldest first.
    #[must_use]
    pub fn text(&self) -> String {
        self.entries
            .lock()
            .map(|entries| entries.iter().cloned().collect::<Vec<_>>().join("\n"))
            .unwrap_or_default()
    }
}
