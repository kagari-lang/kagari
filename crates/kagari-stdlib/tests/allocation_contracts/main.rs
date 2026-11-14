//! Allocation windows require one isolated, thread-local counting allocator.
// Shared preparation keeps verifier and linker failures observable.
#[path = "../../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode_fixture;
#[path = "../../../kagari-runtime/tests/native_allocations_counter.rs"]
mod native_allocations_counter;

mod interface_allocations;
mod native_allocations;
