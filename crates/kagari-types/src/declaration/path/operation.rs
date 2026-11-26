//! Explicit call arguments are in evaluation order, not adapter slot order.
use super::{HostPathSegmentDeclaration, NativePathOperation, NativePathOperationKind};
use crate::{
    callable::CallableImplementation,
    declaration::{
        FnDecl, NativeDeclaration, TypeDeclaration,
        host::{HostPassingStyle, PathAccess},
    },
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_common::identity::{
    DefinitionKind,
    mapping::DefinitionMappingError,
    table::{DefinitionId, DefinitionTable},
};
use std::collections::BTreeSet;

impl NativeDeclaration {
    /// Resolve the explicit path against the actor's complete external schemas and
    /// check the direct callable shape. This does not authenticate an installation.
    pub fn validate_path_operation(
        &self,
        schemas: &[TypeDeclaration],
        definitions: &DefinitionTable,
    ) -> Result<(), DefinitionMappingError> {
        if let Some(operation) = &self.path_operation {
            if !operation.template_valid(self, definitions) {
                return Err(DefinitionMappingError::InvalidContract);
            }
            let contract = operation.path.contract(schemas, definitions)?;
            if !operation.matches_leaf(&self.function, &contract.result) {
                return Err(DefinitionMappingError::InvalidContract);
            }
        }
        Ok(())
    }
}

impl NativePathOperation {
    /// Check the leaf selected by schema resolution against the callable result/RHS.
    /// Use the contract resolved from this exact path, not a decoded result claim.
    fn matches_leaf(&self, function: &FnDecl, leaf: &Ty) -> bool {
        match self.kind {
            NativePathOperationKind::Read => function.return_type == *leaf,
            NativePathOperationKind::Set => {
                function.return_type == Ty::Builtin(BuiltinType::Unit)
                    && function.params.last().is_some_and(|rhs| rhs.ty == *leaf)
            }
            NativePathOperationKind::Add => {
                function.return_type == *leaf
                    && matches!(leaf, Ty::Builtin(builtin) if builtin.number_type().is_some())
                    && function.params.last().is_some_and(|rhs| rhs.ty == *leaf)
            }
        }
    }

    pub(crate) fn template_valid(
        &self,
        declaration: &NativeDeclaration,
        table: &DefinitionTable,
    ) -> bool {
        let function = &declaration.function;
        let Some(policy) = &function.host_policy else {
            return false;
        };
        let write = self.kind != NativePathOperationKind::Read;
        if function.implementation != CallableImplementation::Native(declaration.declaration)
            || !function.generic_params.is_empty()
            || !function.bounds.is_empty()
            || declaration.concrete_result.is_some()
            || !declaration.callable_requirements.is_empty()
            || function.method_policy.receiver_access.is_some()
            || policy.effects.may_suspend
            || policy.passing.len() != function.params.len()
            || policy.passing.iter().any(|p| *p != HostPassingStyle::Owned)
            || (write
                && (!policy.effects.may_mutate_host_state
                    || self.path.access != PathAccess::ReadWrite))
            || self.path.access == PathAccess::None
            || !(1..=256).contains(&self.path.segments.len())
        {
            return false;
        }
        self.owned_callable(declaration.declaration, &function.name, table)
            && self.parameters_valid(function, table)
    }

    fn owned_callable(
        &self,
        declaration: DefinitionId,
        name: &str,
        table: &DefinitionTable,
    ) -> bool {
        let Ok(Some(module)) = table.parent(self.path.root) else {
            return false;
        };
        if !table
            .resolve(module)
            .is_ok_and(|view| view.last().is_none())
            || !table.resolve(self.path.root).is_ok_and(|view| {
                view.last()
                    .is_some_and(|part| part.kind == DefinitionKind::Struct && part.occurrence == 0)
            })
        {
            return false;
        }
        let Ok(Some(parent)) = table.parent(declaration) else {
            return false;
        };
        if parent == module {
            return table.lookup_child(module, DefinitionKind::Function, name, 0)
                == Some(declaration);
        }
        table.parent(parent) == Ok(Some(module))
            && table.resolve(parent).is_ok_and(|view| {
                view.last()
                    .is_some_and(|part| part.kind == DefinitionKind::Impl && part.name.is_empty())
            })
            && table.lookup_child(parent, DefinitionKind::Method, name, 0) == Some(declaration)
    }

    fn parameters_valid(&self, function: &FnDecl, table: &DefinitionTable) -> bool {
        let mut params = function.params.iter();
        if !params
            .next()
            .is_some_and(|param| param.ty == Ty::Host(self.path.root))
        {
            return false;
        }
        let mut slots = BTreeSet::new();
        for segment in &self.path.segments {
            let valid = match segment {
                HostPathSegmentDeclaration::Index(index) => {
                    index.validate(table).is_ok()
                        && slots.insert(index.slot)
                        && params.next().is_some_and(|p| p.ty == index.index)
                }
                HostPathSegmentDeclaration::Virtual(step) => step.validate(table).is_ok(),
                HostPathSegmentDeclaration::Field(id) => table.resolve(*id).is_ok_and(|view| {
                    view.last().is_some_and(|part| {
                        part.kind == DefinitionKind::Field && part.occurrence == 0
                    })
                }),
            };
            if !valid {
                return false;
            }
        }
        if slots.iter().copied().ne(0..slots.len() as u32) {
            return false;
        }
        let leaf = if self.kind != NativePathOperationKind::Read {
            let Some(rhs) = params.next() else {
                return false;
            };
            &rhs.ty
        } else {
            &function.return_type
        };
        let result = match self.path.segments.last() {
            Some(HostPathSegmentDeclaration::Index(index)) => &index.result,
            Some(HostPathSegmentDeclaration::Virtual(step)) => &step.result,
            // Only the complete linked schema owns a field's result type.
            Some(HostPathSegmentDeclaration::Field(_)) => leaf,
            None => return false,
        };
        params.next().is_none() && self.matches_leaf(function, result)
    }
}
