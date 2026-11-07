//! Select admitted associated constants after applying an instance's type arguments.

use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, scalar::ScalarValue, table::ResolvedAssociatedConst},
    types::{TypeId, TypeSubstitution},
};

impl InstancePlanner<'_> {
    pub fn constant(&self, declaration: &DefinitionId) -> Option<ScalarValue> {
        let module = self.modules.get(&self.module_handle(*declaration))?;
        let ResolvedName::Const(id) = module
            .environment()
            .declarations()
            .definition_target(declaration)?
        else {
            return None;
        };
        module
            .environment()
            .local_constant(id)
            .map(|constant| constant.value.clone())
    }

    /// Apply an admitted associated constant selection in the current instance.
    pub fn associated_constant(
        &self,
        fact: &ResolvedAssociatedConst,
        substitution: &TypeSubstitution,
        bounds: &GenericBounds,
        span: Span,
    ) -> Result<ScalarValue, MirLoweringError> {
        let types = self.arguments(
            &[fact.receiver.clone(), TypeId::Trait(fact.interface.clone())],
            substitution,
            span,
        )?;
        let TypeId::Trait(interface) = &types[1] else {
            return Err(MirLoweringError::MissingBinding("constant trait"));
        };
        let (implementation, _) = self
            .catalog
            .concrete_interface_implementation(
                &self.definitions(),
                interface,
                &types[0],
                bounds,
                ImplementationSearchLimits {
                    max_checks: self.options.max_type_nodes,
                    max_depth: self.options.max_type_depth,
                },
                &self.options.cancel,
            )?
            .ok_or(MirLoweringError::MissingBinding("constant implementation"))?;
        let declaration = self
            .catalog
            .implementation_constant(&implementation, &fact.member)
            .ok_or(MirLoweringError::MissingBinding("constant definition"))?;
        self.constant(declaration)
            .ok_or(MirLoweringError::MissingBinding(
                "checked associated constant",
            ))
    }
}
