use std::env;
use std::fmt;
use std::io::Write;

use ansi_term::Color;
use chrono::prelude::*;
use env_logger;
use log;
use log::Level;

// Taken from https://github.com/seanmonstar/pretty-env-logger.
struct ColorLevel(Level);

impl fmt::Display for ColorLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Level::Trace => Color::Purple.paint("TRACE"),
            Level::Debug => Color::Blue.paint("DEBUG"),
            Level::Info => Color::Green.paint("INFO"),
            Level::Warn => Color::Yellow.paint("WARN"),
            Level::Error => Color::Red.paint("ERROR"),
        }.fmt(f)
    }
}

pub fn init() {
    let mut builder = env_logger::Builder::new();

    builder
        .format(|buf, record| {
            writeln!(
                buf,
                "{} - {}:{} |{}| {}",
                Utc::now().format("%Y-%m-%dT%H:%M:%S"),
                record.module_path().unwrap(),
                record.line().unwrap(),
                ColorLevel(record.level()),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Info);

    if let Ok(rust_log) = env::var("RUST_LOG") {
        builder.parse(&rust_log);
    }

    builder.init();
}
