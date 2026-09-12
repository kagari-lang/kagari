mod collect;
mod resolve;
mod resolved;
mod table;

pub use collect::resolve_names;
pub(crate) use collect::resolve_names_controlled;
pub use resolved::{BodyOwner, LexicalScope, ResolvedName, ResolvedNames, ScopeBinding};
pub use table::NameTable;
