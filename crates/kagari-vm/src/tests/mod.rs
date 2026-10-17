mod common;
mod conformance;
mod debug_protocol;
mod execution;
mod gc;
mod helpers;
mod installed_access;
mod jit;
mod mutation_resources;
mod numeric;
mod owned_drive;
mod reentry_debug;
mod sessions;
mod source_programs;

mod native_fixtures;
mod native_wait;

mod library_collections;
mod native_boundary;
mod package_exports;
mod prepared_execution;

#[path = "../../../kagari-runtime/tests/support/native/program.rs"]
mod native_program;
