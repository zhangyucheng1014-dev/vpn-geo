use std::sync::OnceLock;

use log::{Level, LevelFilter, Log, Metadata, Record};

static MIN_LEVEL: OnceLock<LevelFilter> = OnceLock::new();

struct Logger;

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= *MIN_LEVEL.get().unwrap_or(&LevelFilter::Info)
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let level = match record.level() {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        };
        eprintln!("{level} {}", record.args());
    }

    fn flush(&self) {}
}

pub fn init(verbose: bool) {
    let _ = MIN_LEVEL.set(if verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    });
    let _ = log::set_logger(&Logger);
    log::set_max_level(LevelFilter::Trace);
}
