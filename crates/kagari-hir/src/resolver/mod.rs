mod collect;
mod resolve;
mod resolved;
mod table;

pub use collect::resolve_names;
pub(crate) use collect::{collect_declarations, resolve_bodies};
pub use resolved::{
    BodyOwner, DeclarationNames, LexicalScope, ResolvedName, ResolvedNames, ScopeBinding,
};
pub use table::NameTable;
