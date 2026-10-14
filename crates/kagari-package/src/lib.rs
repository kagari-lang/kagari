//! Compile-time package inputs, manifest normalization and source discovery.
//!
//! This crate acquires source descriptions; it never installs or executes code.
//! Runtime consumers receive checked executable products from the compiler.

pub mod error;
pub mod filesystem;
pub mod graph;
pub mod host;
pub mod identity;
pub mod input;
pub mod lock;
pub mod manifest;
pub mod modules;
pub mod path;
pub mod project;
pub mod snapshot;
pub mod workspace;
