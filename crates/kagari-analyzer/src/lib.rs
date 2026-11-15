//! Protocol-neutral editor queries borrowing immutable semantic analysis results.
//!
//! Preparation, checked facts and compiler admission belong to `kagari-hir`.
//! These views select source positions and build editor projections only on demand;
//! the analyzer binary independently owns LSP lifecycle and document synchronization.

pub mod calls;
pub mod declarations;
pub mod documentation;
pub mod file;
pub mod methods;
pub mod navigation;
pub mod snapshot;
pub mod staged;

use crate::{
    declarations::{DeclarationFileQueries, DeclarationSnapshotQueries},
    file::FileQueries,
    snapshot::SnapshotQueries,
    staged::{FunctionQueries, SignatureQueries},
};
use kagari_hir::analysis::{
    AnalysisSnapshot, FileAnalysis,
    body::FunctionAnalysis,
    declarations::{DeclarationSnapshot, FileDeclarations},
    signatures::FileSignatures,
};

/// Opens a borrowing editor view without checking source or preparing an index.
///
/// Keep a view to reuse its demand-built navigation across related requests.
pub trait AnalyzerQueries {
    /// Query view tied to the exact semantic result borrowed by this call.
    type Queries<'a>
    where
        Self: 'a;

    /// Borrows this immutable semantic owner for editor queries.
    fn queries(&self) -> Self::Queries<'_>;
}

impl AnalyzerQueries for FileAnalysis {
    type Queries<'a> = FileQueries<'a>;

    fn queries(&self) -> FileQueries<'_> {
        FileQueries::new(self)
    }
}

impl AnalyzerQueries for AnalysisSnapshot {
    type Queries<'a> = SnapshotQueries<'a>;

    fn queries(&self) -> SnapshotQueries<'_> {
        SnapshotQueries::new(self)
    }
}

impl AnalyzerQueries for FileDeclarations {
    type Queries<'a> = DeclarationFileQueries<'a>;

    fn queries(&self) -> DeclarationFileQueries<'_> {
        DeclarationFileQueries::new(self)
    }
}

impl AnalyzerQueries for DeclarationSnapshot {
    type Queries<'a> = DeclarationSnapshotQueries<'a>;

    fn queries(&self) -> DeclarationSnapshotQueries<'_> {
        DeclarationSnapshotQueries::new(self)
    }
}

impl AnalyzerQueries for FileSignatures {
    type Queries<'a> = SignatureQueries<'a>;

    fn queries(&self) -> SignatureQueries<'_> {
        SignatureQueries::new(self)
    }
}

impl AnalyzerQueries for FunctionAnalysis {
    type Queries<'a> = FunctionQueries<'a>;

    fn queries(&self) -> FunctionQueries<'_> {
        FunctionQueries::new(self)
    }
}

mod namespaces;
mod scope;

#[cfg(test)]
mod query_tests;
