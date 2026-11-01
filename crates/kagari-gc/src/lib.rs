//! Explicit roots and scoped access to a nonmoving, incremental mark-sweep heap.
//!
//! [`heap::Heap`] owns storage and collection. [`root::Root`] keeps an object alive
//! across access intervals; [`object::Edge`] stores a traced internal reference.
//! [`heap::Heap::scope`] supplies copyable [`object::Gc`] identities and short data
//! loans. Checked [`access::ScalarField`] and [`access::EdgeField`] operations hide
//! publication/continuation from ordinary users; roots are explicit scope exits.
//! Unsafe providers implement [`object::GcObject`] once for scanning and mutation.
//! Ordinary consumers use safe provider constructors and cannot collect while an
//! [`access::Access`] or object loan exists. No Kagari language crate is required.

pub mod access;
pub mod descriptor;
pub mod dynamic;
pub mod error;
pub mod heap;
pub mod object;
pub mod root;
pub mod root_source;
pub mod scan;

mod allocation;
mod marking;
mod sweep;
mod table;
