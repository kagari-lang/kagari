//! Allocation windows require one isolated, thread-local counting allocator.
// Shared preparation keeps verifier and linker failures observable.
#[path = "../support/bytecode.rs"]
mod bytecode_fixture;
#[path = "../native_allocations_counter.rs"]
mod native_allocations_counter;

mod interface_allocations;
mod native_allocations;
