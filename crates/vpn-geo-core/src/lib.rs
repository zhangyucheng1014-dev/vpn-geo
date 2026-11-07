pub mod benchmark;
pub mod config;
pub mod daemon;
pub mod error;
pub mod geoip;
pub mod http;
pub mod logging;
pub mod mirror;
pub mod network;
pub mod priority;
pub mod state;

pub use config::Config;
pub use error::{Error, Result};

pub const APP_NAME: &str = "vpn-geo";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
