//! Persistent storage contracts, independent of executable generation and live state.
pub mod wire;

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{declaration::verify::types_in_scope, ty::Ty, visibility::Visibility};
use std::slice;

/// A closed storage contract. Initializer code and current values are not public ABI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticContract {
    pub declaration: DefinitionId,
    pub supplier: ModuleIdentity,
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
    pub visibility: Visibility,
}

impl StaticContract {
    /// Check identity, lexical scope and durable type shape without executing code.
    pub fn valid(&self, definitions: &DefinitionTable, cancel: &CancellationToken) -> bool {
        definitions.resolve(self.declaration).is_ok_and(|view| {
            view.module() == &self.supplier
                && view.segments().count() == 1
                && view.last().is_some_and(|part| {
                    part.kind == DefinitionKind::Static
                        && part.name == self.name
                        && part.occurrence == 0
                })
        }) && !self.name.is_empty()
            && self.ty.is_concrete()
            && types_in_scope(slice::from_ref(&self.ty), &[], cancel, definitions)
            && !self.ty.contains_stored_reference()
            && self.ty.reference_position_valid()
    }

    /// Initializers have dedicated module-level function identities, never method
    /// identities selectable through inherent/trait tables or trait defaults.
    /// Export and executable operand checks additionally keep these functions hidden.
    pub fn accepts_initializer_identity(
        &self,
        id: DefinitionId,
        definitions: &DefinitionTable,
    ) -> bool {
        definitions.resolve(id).is_ok_and(|view| {
            view.module() == &self.supplier
                && view.segments().count() == 1
                && view
                    .last()
                    .is_some_and(|part| part.kind == DefinitionKind::Function)
        })
    }

    pub fn accessible_from(&self, module: &ModuleIdentity) -> bool {
        self.visibility.allows(&self.supplier, module)
    }
}

/// An indexed binding to a declaration and its supplier's hidden initializer.
/// Importing modules repeat this verified descriptor, not the storage allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticDescriptor<I> {
    pub contract: StaticContract,
    pub initializer: I,
}

impl RemapDefinitions for StaticContract {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            declaration: mapper.reference(&self.declaration)?,
            supplier: self.supplier.clone(),
            name: self.name.clone(),
            ty: self.ty.remap_definitions(mapper)?,
            mutable: self.mutable,
            visibility: self.visibility,
        })
    }
}

impl DefinitionReferences for StaticContract {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        visit(&self.declaration)?;
        self.ty.visit_definition_ids(visit, cancel)
    }
}

impl<I: Clone> RemapDefinitions for StaticDescriptor<I> {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        Ok(Self {
            contract: self.contract.remap_definitions(mapper)?,
            initializer: self.initializer.clone(),
        })
    }
}

impl<I> DefinitionReferences for StaticDescriptor<I> {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.contract.visit_definition_ids(visit, cancel)
    }
}
