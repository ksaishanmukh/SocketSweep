//! The benchmark of `docs/architecture.md`, section 6: the engine's focus
//! mode against its plain mode (the baseline), on one connection, with the
//! navigation scripts (a), (b) and (c), interleaved.

mod config;
mod report;
mod run;
mod run_once;
mod script;

pub use config::Config;
pub use report::{ClickRecord, Report, RunRecord, ScriptSetup};
pub use run::run;
pub use run_once::run_once;
pub use script::{mode_name, Click, Reference, Script, ScriptKind};
