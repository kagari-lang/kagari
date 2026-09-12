mod collect;
mod resolve;
mod resolved;
mod table;

pub use collect::resolve_names;
pub(crate) use collect::resolve_names_controlled;
pub use resolved::{ResolvedName, ResolvedNames};
pub use table::NameTable;
