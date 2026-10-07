//! Checked callable signatures shared by source and offline host declarations.
//!
//! Source parameter bindings and host passing contracts remain with their owners.
//! Call checking needs the same named semantic types without inventing source
//! arena IDs for declarations that never had a script body.

use crate::{
    typeck::{FunctionImplementation, GenericBounds, TypedFunction},
    types::{GenericParameterType, TypeId},
};
use kagari_common::identity::{DefinitionPath, reference::DefinitionReference};

/// The selected declaration's signature after call-site substitution. Generic
/// types belonging to the enclosing body remain for compiler monomorphization.
/// These are parameter types, not the types of the argument expressions: a
/// readonly conversion or a diverging argument must not change the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCallSignature<I: DefinitionReference = DefinitionPath> {
    /// Includes a method receiver; excludes the callee of a function-value call.
    pub params: Vec<TypeId<I>>,
    /// Result type after the selected declaration's call-site substitution.
    pub return_type: TypeId<I>,
}

/// A shared view of source/native callable contracts used by argument checking.
pub trait CallableSignature {
    /// Returns the diagnostic callable name.
    fn name(&self) -> &str;

    /// Returns script/native/required implementation provenance.
    fn implementation(&self) -> FunctionImplementation;

    /// Visits parameter names and semantic types in call order.
    fn parameters(&self) -> impl ExactSizeIterator<Item = (&str, &TypeId)>;

    /// Borrows the declaration's semantic result type.
    fn return_type(&self) -> &TypeId;

    /// Borrows generic binders in declaration order; nongeneric signatures default to none.
    fn generic_params(&self) -> &[GenericParameterType] {
        &[]
    }

    /// Borrows checked generic constraints, when this signature supplies them.
    fn bounds(&self) -> Option<&GenericBounds> {
        None
    }
}

impl CallableSignature for TypedFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn implementation(&self) -> FunctionImplementation {
        self.implementation.clone()
    }

    fn parameters(&self) -> impl ExactSizeIterator<Item = (&str, &TypeId)> {
        self.params
            .iter()
            .map(|parameter| (parameter.name.as_str(), &parameter.ty))
    }

    fn return_type(&self) -> &TypeId {
        &self.return_type
    }

    fn generic_params(&self) -> &[GenericParameterType] {
        &self.generic_params
    }

    fn bounds(&self) -> Option<&GenericBounds> {
        Some(&self.bounds)
    }
}

mod mapping;
