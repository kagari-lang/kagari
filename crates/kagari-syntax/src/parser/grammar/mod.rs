//! Grammar handlers extending the shared [`super::core::Parser`] state.
//!
//! `item` owns file/declaration structure, `stmt` owns block contents, `expr` owns
//! precedence and patterns, and `types` owns type spellings. Handlers write CST
//! nodes directly; [`crate::ast`] documents their resulting child layouts.
//! Missing expected tokens produce diagnostics rather than synthetic token leaves.

mod expr;
mod item;
mod stmt;
mod types;
