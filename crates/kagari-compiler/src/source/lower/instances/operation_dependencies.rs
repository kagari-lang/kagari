//! Select implementation shapes while deferring applicability to checked call bounds.
use crate::source::lower::{
    MirLoweringError,
    instances::{InstancePlanner, constructions::BoundEvidence},
};
use kagari_common::{identity::table::DefinitionId, span::Span};
use kagari_hir::types::{TypeId, semantic::raise_type};
use kagari_types::ty::{
    GenericParam, NominalTy, Ty,
    matching::{ImplementationPattern, match_pattern},
};

impl InstancePlanner<'_> {
    /// A symbolic receiver can select a unique implementation shape before its
    /// component operations are bound. Executable verification still proves every
    /// substituted implementation bound in the caller's actual scope.
    pub(super) fn implementation_candidate(
        &self,
        receiver: &Ty,
        interface: &NominalTy,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, MirLoweringError> {
        if !matches!(
            receiver,
            Ty::Struct(_) | Ty::NativeObject(_) | Ty::Enum(_) | Ty::Array(_)
        ) {
            return Ok(None);
        }
        let invalid = || MirLoweringError::MissingBinding("shared operation candidate");
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
            let pattern_receiver = self.encode_type(&implementation.for_type);
            let pattern_interface = self.encode_nominal(&implementation.trait_type);
            let Some(bindings) = match_pattern(
                ImplementationPattern {
                    parameters: &parameters,
                    receiver: &pattern_receiver,
                    interface: &pattern_interface,
                },
                interface,
                receiver,
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

    pub(in crate::source::lower) fn implementation_evidence(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<BoundEvidence, MirLoweringError> {
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
        self.bound_evidence(&bounds, &substitution, span)
    }
}
