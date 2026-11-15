//! Declaration-site selection using temporary HR06/HR07 site metadata.

use kagari_hir::{
    analysis::declarations::{DeclarationSnapshot, FileDeclarations},
    declarations::{Declaration, Declarations},
};

/// Cursor selection over canonical declarations owned by one semantic result.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationQueries<'a> {
    analysis: &'a Declarations,
}

impl<'a> DeclarationQueries<'a> {
    /// Borrows declarations; does not construct another declaration directory.
    pub fn new(analysis: &'a Declarations) -> Self {
        Self { analysis }
    }

    /// Member names only; an unresolved use cannot select an enclosing declaration.
    pub fn member_at(&self, offset: usize) -> Option<&'a Declaration> {
        self.analysis
            .sites()
            .find_map(|(range, declaration, member)| {
                (member && range.start <= offset && offset < range.end).then_some(declaration)
            })
    }

    /// Selects in analysis coordinates, not a copied/native physical origin.
    pub fn site_at(&self, offset: usize) -> Option<&'a Declaration> {
        self.analysis
            .sites()
            .filter(|(range, _, _)| range.start <= offset && offset < range.end)
            .min_by_key(|(range, _, _)| range.end - range.start)
            .map(|(_, declaration, _)| declaration)
    }
}

/// Editor queries that stop before signature and body checking.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationFileQueries<'a> {
    pub(crate) analysis: &'a FileDeclarations,
}

impl<'a> DeclarationFileQueries<'a> {
    /// Borrows one declaration-stage semantic owner.
    pub fn new(analysis: &'a FileDeclarations) -> Self {
        Self { analysis }
    }

    /// Finds a declared member name without checking signatures or bodies.
    pub fn member_at(&self, offset: usize) -> Option<&'a Declaration> {
        DeclarationQueries::new(self.analysis.declarations()).member_at(offset)
    }
}

/// Documentation queries over retained declaration results, without body checking.
#[derive(Debug, Clone, Copy)]
pub struct DeclarationSnapshotQueries<'a> {
    pub(crate) analysis: &'a DeclarationSnapshot,
}

impl<'a> DeclarationSnapshotQueries<'a> {
    /// Borrows the exact declaration snapshot that owns source identities.
    pub fn new(analysis: &'a DeclarationSnapshot) -> Self {
        Self { analysis }
    }
}
