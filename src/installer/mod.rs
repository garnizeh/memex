pub mod config_writer;
pub mod targets;

pub use config_writer::*;
pub use targets::{
    AgentTarget, AntigravityTarget, ClaudeTarget, CursorTarget, DetectionResult, InstallOptions,
    TargetRegistry,
};
