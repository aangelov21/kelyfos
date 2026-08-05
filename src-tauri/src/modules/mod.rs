pub mod agent;
pub mod backup;
pub mod fonts;
pub mod menu;
pub mod models;
pub mod process;
pub mod pty;
pub mod shell_integration;
pub mod startup;
pub mod storage;
pub mod variables;
pub mod workspace;

// Architecture placeholder, no implementation yet. Agent state itself now lives
// in `agent`, which reads Claude Code rather than hosting an agent of its own.
pub mod mcp;
