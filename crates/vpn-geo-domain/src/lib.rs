pub mod catalog;
pub mod config;
pub mod decision;
pub mod error;
pub mod model;
pub mod policy;

pub use error::{DomainError, DomainResult};

pub const APP_NAME: &str = "vpn-geo";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
