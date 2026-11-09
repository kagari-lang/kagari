//! Opaque preparation identity without code retention or installation authority.
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

/// Exact immutable preparation identity. Clones retain only its identity token,
/// not bytecode, compiler inputs, a Runtime or permission to execute a function.
/// Equal content from independent preparations has distinct revision identity.
#[derive(Debug, Clone)]
pub struct ProgramRevision(Arc<Token>);

#[derive(Debug)]
struct Token;

impl ProgramRevision {
    pub(super) fn new() -> Self {
        Self(Arc::new(Token))
    }
}

impl PartialEq for ProgramRevision {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ProgramRevision {}

impl Hash for ProgramRevision {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // The owning token prevents address reuse while a key is retained.
        Arc::as_ptr(&self.0).hash(state);
    }
}
