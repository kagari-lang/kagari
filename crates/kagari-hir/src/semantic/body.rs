//! Retained body facts and shared signature contracts, with no checking workspace.

use crate::{
    hir::ids::{ConstId, FunctionId, StaticId},
    semantic::signatures::BodySignatures,
    typeck::{scalar::ScalarValue, table::TypeTable},
    types::TypeId,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

mod mapping;

/// Signature and body facts produced by type checking, alongside external diagnostics.
///
/// Counters distinguish freshly checked from reused function bodies. Error types
/// can remain in a recovered result; [`crate::AnalysisResult`] owns diagnostics and
/// the checked conversion gate. The table is not embedded into each HIR node.
///
/// For `fn add(x: i32) -> i32 { x + 1 }`, [`Self::signatures`] describes `x` and
/// the return type. [`Self::type_table`] records the parameter reference and sum's
/// types and selected addition operation. All local IDs refer to the lowering
/// retained by the enclosing analysis; copying facts does not rebase those IDs.
/// Selected-body queries may leave unrelated bodies unchecked. Their enclosing
/// analysis exposes completion evidence separately from this read-only table.
#[derive(Debug, Clone)]
pub struct ModuleFacts {
    /// Bodies completed without errors, recorded before presentation truncation.
    pub(crate) valid_bodies: HashSet<FunctionId>,
    /// Initializers completed without errors, before diagnostic truncation.
    pub(crate) valid_static_initializers: HashSet<StaticId>,
    /// Number of function bodies freshly checked for this result.
    pub(crate) checked_bodies: usize,
    /// Number of function bodies restored from compatible prior facts.
    pub(crate) reused_bodies: usize,
    /// Shared callable/static contracts normalized in this body environment.
    pub(crate) signatures: Arc<BodySignatures>,
    /// Local constant IDs to inferred/declared semantic types.
    pub(crate) consts: HashMap<ConstId, TypeId>,
    /// Successfully evaluated scalar constants; absent entries have no published scalar value.
    pub(crate) const_values: HashMap<ConstId, ScalarValue>,
    /// Node-keyed types and selected call/member/protocol facts.
    pub(crate) type_table: TypeTable,
}

impl ModuleFacts {
    /// Borrows normalized callable/static contracts from the fixed body environment.
    pub fn signatures(&self) -> &BodySignatures {
        &self.signatures
    }

    /// Borrows node-keyed facts from this result's matching lowered module.
    ///
    /// Missing entries can mean an inapplicable operation, an unchecked body, or
    /// recovery after an error. This table alone cannot prove executable validity.
    pub fn type_table(&self) -> &TypeTable {
        &self.type_table
    }

    /// Borrows inferred or declared types of this module's scalar constants.
    pub fn consts(&self) -> &HashMap<ConstId, TypeId> {
        &self.consts
    }

    /// Borrows successfully evaluated scalar constants without evaluating source.
    pub fn const_values(&self) -> &HashMap<ConstId, ScalarValue> {
        &self.const_values
    }

    /// Returns how many function bodies were freshly checked for this result.
    pub fn checked_bodies(&self) -> usize {
        self.checked_bodies
    }

    /// Returns how many function bodies were restored from compatible prior facts.
    pub fn reused_bodies(&self) -> usize {
        self.reused_bodies
    }
}
