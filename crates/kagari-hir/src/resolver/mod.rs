mod collect;
mod resolve;
mod resolved;
mod table;

pub use collect::resolve_names;
pub use resolved::{ResolvedName, ResolvedNames};
pub use table::NameTable;
