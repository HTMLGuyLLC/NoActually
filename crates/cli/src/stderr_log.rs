//! Warnings from the store and sources (a chat file that couldn't be read, report cards left out)
//! go to stderr, so a person running `noactually` sees them. The desktop app has its own log, and
//! the helper commands never call this: they stay silent.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Stderr;

impl Log for Stderr {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        m.level() <= Level::Warn
    }

    fn log(&self, r: &Record<'_>) {
        if self.enabled(r.metadata()) {
            eprintln!("{}: {}", r.level().as_str().to_lowercase(), r.args());
        }
    }

    fn flush(&self) {}
}

/// Starts the logger at warn level. A logger that is already set (the app's) is left alone.
pub fn init() {
    if log::set_logger(&Stderr).is_ok() {
        log::set_max_level(LevelFilter::Warn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_warnings_and_errors_get_through() {
        let l = Stderr;
        assert!(l.enabled(&Metadata::builder().level(Level::Error).build()));
        assert!(l.enabled(&Metadata::builder().level(Level::Warn).build()));
        assert!(!l.enabled(&Metadata::builder().level(Level::Info).build()));
    }
}
