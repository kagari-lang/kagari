//! Validate source-owned provenance against the complete named inventory.

use kagari_common::{
    cancellation::CancellationToken,
    identity::mapping::{DefinitionMappingError, check_cancel},
};
use kagari_contract::types::named::{NamedContract, NamedDeclaration};
use kagari_hir::{
    compile_input::program::CheckedModule,
    update_source::{SourceBodyKind, SourceEvidence},
};

/// Require authored function/constant owners in the named declaration inventory.
/// Statics retain their separate owning contracts. Admission owns the shared budget.
pub fn collect<'a>(
    module: &'a CheckedModule<'_>,
    inventory: &[NamedDeclaration],
    cancel: &CancellationToken,
) -> Result<&'a SourceEvidence, DefinitionMappingError> {
    let evidence = module.provenance();
    check_cancel(cancel)?;
    for body in &evidence.bodies {
        check_cancel(cancel)?;
        if body.kind == SourceBodyKind::StaticInitializer {
            continue;
        }
        if !inventory.iter().any(|record| {
            record.declaration == body.declaration
                && matches!(
                    (&record.contract, body.kind),
                    (NamedContract::Constant(_), SourceBodyKind::Constant)
                        | (
                            NamedContract::Function(_),
                            SourceBodyKind::Function
                                | SourceBodyKind::AsyncFunction
                                | SourceBodyKind::Native
                                | SourceBodyKind::Required
                        )
                )
        }) {
            return Err(DefinitionMappingError::InvalidContract);
        }
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::lower::module_contract;
    use crate::tests::common::{compilation, program_ok};
    use kagari_common::cancellation::CancellationToken;

    #[test]
    fn source_adapter_checks_provenance_inventory_without_collecting_function_contents() {
        let program = program_ok("fn unused<T>(x: T) -> T { x } static val N: i32 = ((1 + 2));");
        let input = compilation(&program);
        let module = input.module(input.root()).unwrap();
        let cancel = CancellationToken::default();
        assert_eq!(
            collect(module, &[], &cancel),
            Err(DefinitionMappingError::InvalidContract)
        );
        let inventory = module_contract(module, &cancel).unwrap();
        let records = collect(module, &inventory.records().named_declarations, &cancel).unwrap();
        assert_eq!(records.initializers.len(), 1);
        let function = records
            .bodies
            .iter()
            .find(|body| body.kind == SourceBodyKind::Function)
            .unwrap();
        assert!(function.tokens.is_empty());
        assert!(function.facts.is_empty());
        assert!(function.initializer.is_none());
    }
}
