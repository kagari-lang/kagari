//! Allocation windows require one isolated, thread-local counting allocator.
#[path = "../native_allocations_counter/mod.rs"]
mod native_allocations_counter;

mod interface_allocations;
mod native_allocations;
