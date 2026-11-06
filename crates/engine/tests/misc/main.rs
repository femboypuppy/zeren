//! Small suites in one test binary: each tests/*.rs file is its own binary,
//! linked against the whole crate, so a handful of tests each cost more to
//! link than to run.

mod claude_subagents;
mod codex_subagents;
mod local_profiles;
mod osc7501;
mod pi_resume;
