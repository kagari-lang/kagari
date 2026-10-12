//! Select implementation shapes while deferring applicability to checked call bounds.
use crate::source::lower::{MirLoweringError, instances::InstancePlanner};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_contract::callable::witness::OperationWitness;
use kagari_hir::types::{TypeId, semantic::raise_type};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    language::Protocol,
    ty::{
        GenericParam, Ty,
        matching::{ImplementationPattern, match_pattern},
    },
};

impl InstancePlanner<'_> {
    /// A symbolic receiver can select a unique implementation shape before its
    /// component operations are bound. Executable verification still proves every
    /// substituted implementation bound in the caller's actual scope.
    pub(super) fn operation_candidate(
        &self,
        required: &NativeCallableRequirement,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, MirLoweringError> {
        if !matches!(
            required.receiver,
            Ty::Struct(_)
                | Ty::NativeObject(_)
                | Ty::Enum(_)
                | Ty::Array(_)
                | Ty::Map { .. }
                | Ty::Set(_, _)
        ) {
            return Ok(None);
        }
        let invalid = || MirLoweringError::MissingBinding("shared operation candidate");
        let definitions = self.definitions();
        let mut selected = None;
        for (index, implementation) in self.catalog.implementations().enumerate() {
            if index >= 100_000 {
                return Err(invalid());
            }
            let parameters: Vec<_> = implementation
                .generic_params
                .iter()
                .map(|parameter| GenericParam {
                    owner: parameter.owner,
                    position: parameter.position,
                })
                .collect();
            let receiver = self.encode_type(&implementation.for_type);
            let interface = self.encode_nominal(&implementation.trait_type);
            let Some(bindings) = match_pattern(
                ImplementationPattern {
                    parameters: &parameters,
                    receiver: &receiver,
                    interface: &interface,
                    protocol: Protocol::from_definition(&interface.declaration, &definitions),
                    storage_access: None,
                },
                &required.interface,
                &required.receiver,
                &self.options.cancel,
            )
            .map_err(|_| invalid())?
            else {
                continue;
            };
            if selected.is_some() {
                return Ok(None);
            }
            let arguments = parameters
                .iter()
                .map(|parameter| {
                    bindings
                        .parameter(&parameter.owner, parameter.position)
                        .map(raise_type)
                })
                .collect::<Option<Vec<_>>>()
                .ok_or_else(invalid)?;
            selected = Some((implementation.id, arguments));
        }
        Ok(selected)
    }

    pub(super) fn implementation_operations(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<Vec<OperationWitness>, MirLoweringError> {
        let implementation = self.catalog.implementation_signature(declaration).ok_or(
            MirLoweringError::MissingBinding("shared operation implementation bounds"),
        )?;
        let substitution = implementation
            .generic_params
            .iter()
            .cloned()
            .zip(arguments.iter().cloned())
            .collect();
        let bounds = implementation.bounds.clone();
        self.bound_operations(&bounds, &substitution, span)
    }
}
