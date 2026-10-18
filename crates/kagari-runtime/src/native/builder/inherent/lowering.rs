//! Rebind one inherent group through the shared semantic substitution machinery.
use crate::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        builder::inherent::InherentMethodsBuilder,
    },
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::{
    declaration::{module::ImplDecl, requirement::NativeCallableRequirement},
    ty::{GenericBound, GenericParam, Ty, substitution::TypeSubstitution},
};
use std::collections::BTreeMap;

/// Completed semantic records plus the original implementation associations.
/// Lowering never prepares bindings or grants publication/installation evidence.
pub(crate) struct LoweredInherent {
    pub(crate) declaration: ImplDecl,
    pub(crate) documentation: BTreeMap<DefinitionId, String>,
    pub(crate) method_parameters: BTreeMap<DefinitionId, Vec<String>>,
    pub(crate) requirements: BTreeMap<DefinitionId, Vec<NativeCallableRequirement>>,
    pub(crate) concrete_results: BTreeMap<DefinitionId, Ty>,
    pub(crate) bindings: BTreeMap<DefinitionId, NativeBinding>,
}

impl InherentMethodsBuilder {
    /// Move receiver parameters to this group's owner, retaining each method's
    /// own binder. Requirements and concrete results use the same substitution
    /// as signatures, including when an authoring receiver is reused by groups.
    pub(crate) fn lower(
        self,
        source_parameters: &[GenericParam],
        bounds: &[GenericBound],
    ) -> NativeResult<LoweredInherent> {
        let parameters = source_parameters
            .iter()
            .map(|parameter| GenericParam {
                owner: self.owner,
                position: parameter.position,
            })
            .collect::<Vec<_>>();
        let arguments = parameters
            .iter()
            .map(GenericParam::as_type)
            .collect::<Vec<_>>();
        let mut substitution = TypeSubstitution::default();
        for (source, target) in source_parameters.iter().zip(&arguments) {
            substitution.bind(&source.owner, source.position, target);
        }
        let cancel = CancellationToken::default();
        let definitions = self.context.snapshot();
        let receiver = substitution
            .apply(self.receiver.abi(), &cancel)
            .map_err(|_| conflict("invalid inherent receiver substitution"))?;
        let bounds = substitution
            .apply_bounds(bounds, &definitions, &cancel)
            .map_err(|_| conflict("invalid inherent group bounds substitution"))?;
        let mut signatures = Vec::new();
        for mut signature in self.methods.into_values() {
            let mut combined = parameters.clone();
            combined.append(&mut signature.generic_params);
            signature.generic_params = combined;
            signature.bounds = substitution
                .apply_bounds(&signature.bounds, &definitions, &cancel)
                .map_err(|_| conflict("invalid inherent bounds substitution"))?;
            for parameter in &mut signature.params {
                parameter.ty = substitution
                    .apply(&parameter.ty, &cancel)
                    .map_err(|_| conflict("invalid inherent argument substitution"))?;
            }
            signature.return_type = substitution
                .apply(&signature.return_type, &cancel)
                .map_err(|_| conflict("invalid inherent result substitution"))?;
            signatures.push(signature);
        }
        signatures.sort_by(|left, right| left.name.cmp(&right.name));
        let concrete_results = self
            .concrete_results
            .into_iter()
            .map(|(id, ty)| {
                substitution
                    .apply(&ty, &cancel)
                    .map(|ty| (id, ty))
                    .map_err(|_| conflict("invalid inherent concrete result substitution"))
            })
            .collect::<NativeResult<_>>()?;
        let requirements = self
            .requirements
            .into_iter()
            .map(|(id, requirements)| {
                requirements
                    .iter()
                    .map(|requirement| requirement.apply(&substitution, &cancel))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|requirements| (id, requirements))
                    .map_err(|_| conflict("invalid inherent requirement substitution"))
            })
            .collect::<NativeResult<_>>()?;
        Ok(LoweredInherent {
            declaration: ImplDecl {
                generic_params: parameters,
                bounds,
                trait_type: None,
                for_type: receiver,
                methods: signatures,
            },
            documentation: self.documentation,
            method_parameters: self.method_parameters,
            requirements,
            concrete_results,
            bindings: self.bindings,
        })
    }
}

fn conflict(message: &str) -> RuntimeError {
    RuntimeError::metadata_conflict(message)
}
