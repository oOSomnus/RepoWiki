pub mod analyzer;
pub mod cli;
pub mod docs;
pub mod html;
pub mod language;
pub mod model;
pub mod prompts;
pub mod reader;
pub mod session;
pub mod update;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
