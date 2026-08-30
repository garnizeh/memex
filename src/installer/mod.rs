pub mod agent_rules;
pub mod config_writer;
pub mod targets;

pub use agent_rules::*;
pub use config_writer::*;
pub use targets::{
    AgentTarget, AntigravityTarget, ClaudeTarget, CursorTarget, DetectionResult, InstallOptions,
    TargetRegistry,
};
