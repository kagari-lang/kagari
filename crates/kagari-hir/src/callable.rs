//! Checked callable signatures shared by source and offline host declarations.
//!
//! Source parameter bindings and host passing contracts remain with their owners.
//! Call checking needs the same named semantic types without inventing source
//! arena IDs for declarations that never had a script body.

use crate::{
    typeck::{FunctionImplementation, GenericBounds, TypedFunction},
    types::{GenericParameterType, TypeId},
};

/// The selected declaration's signature after call-site substitution. Generic
/// types belonging to the enclosing body remain for compiler monomorphization.
/// These are parameter types, not the types of the argument expressions: a
/// readonly conversion or a diverging argument must not change the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedCallSignature {
    /// Includes a method receiver; excludes the callee of a function-value call.
    pub params: Vec<TypeId>,
    pub return_type: TypeId,
}

pub trait CallableSignature {
    fn name(&self) -> &str;
    fn implementation(&self) -> FunctionImplementation;
    fn parameters(&self) -> impl ExactSizeIterator<Item = (&str, &TypeId)>;
    fn return_type(&self) -> &TypeId;

    fn generic_params(&self) -> &[GenericParameterType] {
        &[]
    }

    fn bounds(&self) -> Option<&GenericBounds> {
        None
    }
}

impl CallableSignature for TypedFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn implementation(&self) -> FunctionImplementation {
        self.implementation
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
