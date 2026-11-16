//! Scoped static contracts for source-backed declarations, without binding authority.

use crate::{
    analysis::{error::AnalysisError, external::ExternalModule},
    hir::item::function::{FunctionImplementation, NativeContract},
    lower::LoweredModule,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{map::DefinitionContext, mapping::DefinitionMappingError, table::DefinitionTable},
};
use kagari_types::{
    declaration::module::DeclarationError,
    ty::{Ty, identity::PortableTypes},
};

impl ExternalModule {
    pub(super) fn attach_contracts(
        &self,
        lowered: &mut LoweredModule,
        definitions: &DefinitionContext,
        cancel: &CancellationToken,
    ) -> Result<(), AnalysisError> {
        for (selector, metadata) in &self.contracts {
            cancel.check()?;
            let mut matching = lowered
                .module
                .functions
                .iter()
                .filter(|function| selector.matches(function, lowered));
            let function = matching.next().ok_or_else(|| {
                DeclarationError(format!(
                    "missing external Native contract target {selector:?}"
                ))
            })?;
            if matching.next().is_some()
                || !matches!(function.implementation, FunctionImplementation::Native(_))
            {
                return Err(DeclarationError(format!(
                    "invalid external Native contract target {selector:?}"
                ))
                .into());
            }
            let id = function.id;
            let imported = metadata.import_into(definitions, cancel)?;
            lowered.module.functions[id.index()].implementation =
                FunctionImplementation::Native(imported.into_records());
        }
        Ok(())
    }
}

/// Canonical input identity excludes process-local definition IDs while retaining
/// operation order, repeated slots, member selection and the concrete result.
pub(in crate::analysis) fn fingerprint(
    contract: &NativeContract,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<String, AnalysisError> {
    let mut parts = Vec::new();
    let encode = |types: &[Ty]| {
        PortableTypes::encode(definitions, types, cancel)
            .map(|types| format!("{types:?}"))
            .map_err(|error| {
                DeclarationError(format!("invalid external contract types: {error:?}"))
            })
    };
    for required in &contract.callable_requirements {
        cancel.check()?;
        let member = definitions
            .resolve(required.member)
            .map_err(DefinitionMappingError::from)?
            .to_path();
        let mut types = vec![
            required.receiver.clone(),
            Ty::Trait(required.interface.clone()),
        ];
        types.extend(required.arguments.iter().cloned());
        parts.push(format!("{member:?}:{}", encode(&types)?));
    }
    let result = encode(&contract.concrete_result.iter().cloned().collect::<Vec<_>>())?;
    cancel.check()?;
    Ok(format!(
        "{:?}:{:?}:{parts:?}:{result}",
        contract.receiver_access, contract.host_policy
    ))
}
