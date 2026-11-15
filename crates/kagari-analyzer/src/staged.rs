//! Cursor adapters preserving signature-before-body and selected-body-only routes.

use crate::{
    declarations::DeclarationQueries,
    file::{member_receiver_type_in, type_at_in, type_reference_at},
};
use kagari_hir::{
    analysis::{body::FunctionAnalysis, signatures::FileSignatures},
    declarations::Declaration,
    hir::ids::BodyOwner,
    types::TypeId,
};

/// Queries borrowing checked signatures, never forcing body analysis.
#[derive(Debug, Clone, Copy)]
pub struct SignatureQueries<'a> {
    analysis: &'a FileSignatures,
}

impl<'a> SignatureQueries<'a> {
    /// Borrows a signature-stage result and its matching lowering.
    pub fn new(analysis: &'a FileSignatures) -> Self {
        Self { analysis }
    }

    /// Checked signature names and declaration sites only.
    pub fn definition_at(&self, offset: usize) -> Option<&'a Declaration> {
        DeclarationQueries::new(self.analysis.declarations())
            .site_at(offset)
            .or_else(|| {
                type_reference_at(
                    self.analysis.lowered(),
                    self.analysis.signatures().facts().type_table(),
                    self.analysis.declarations(),
                    offset,
                )
                .flatten()
            })
    }

    /// Body annotations have no facts until a selected/full body query checks them.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        type_at_in(
            self.analysis.lowered(),
            self.analysis.signatures().facts().type_table(),
            offset,
        )
    }
}

/// Queries borrowing only one checked function and its prerequisite facts.
#[derive(Debug, Clone, Copy)]
pub struct FunctionQueries<'a> {
    analysis: &'a FunctionAnalysis,
}

impl<'a> FunctionQueries<'a> {
    /// Borrows a selected-body result without checking neighboring functions.
    pub fn new(analysis: &'a FunctionAnalysis) -> Self {
        Self { analysis }
    }

    // Temporary scope history reader until HR06 derives routing from structure.
    fn contains(&self, offset: usize) -> bool {
        self.analysis.names().scopes().iter().any(|scope| {
            scope.owner == BodyOwner::Function(self.analysis.function())
                && scope.span.start <= offset
                && offset <= scope.span.end
        })
    }

    /// Returns checked types only inside the selected function's lexical extent.
    pub fn type_at(&self, offset: usize) -> Option<TypeId> {
        self.contains(offset)
            .then(|| type_at_in(self.analysis.lowered(), self.analysis.type_table(), offset))
            .flatten()
    }

    /// Returns a checked receiver only inside the selected function.
    pub fn member_receiver_type(&self, offset: usize) -> Option<TypeId> {
        self.contains(offset)
            .then(|| {
                member_receiver_type_in(self.analysis.lowered(), self.analysis.type_table(), offset)
            })
            .flatten()
    }
}
