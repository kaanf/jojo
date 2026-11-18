//! Pure domain contracts. No persistence, process launcher or effectful provider adapter.
//!
//! Clock and identity effects are supplied by callers; this crate never discovers host state.

pub mod attempt;
pub mod brief;
pub mod claude;
pub mod codex;
pub mod dependency;
mod dependency_wire;
pub mod identity;
pub mod json;
pub mod outcome;
pub mod paths;
pub mod planner;
pub mod policy;
pub mod prd;
pub mod project;
pub mod provider;
pub mod recovery;
pub mod repair;
pub mod review;
pub mod shutdown;
pub mod stream;
pub mod time;
pub mod validation;

pub mod scheduler;

pub mod ipc;
pub mod protocol;
