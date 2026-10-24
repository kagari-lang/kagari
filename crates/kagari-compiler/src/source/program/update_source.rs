//! Validate source-owned provenance against the complete named inventory.

use kagari_common::identity::mapping::DefinitionMappingError;
use kagari_contract::types::named::{NamedContract, NamedDeclaration};
use kagari_hir::{
    CheckedAnalysis,
    update_source::{self, SourceBodyKind, SourceBudget, SourceEvidence},
};

/// Require authored function/constant owners in the named declaration inventory.
/// Statics retain their separate owning contracts. Use one budget per publication.
pub fn collect(
    module: &CheckedAnalysis,
    inventory: &[NamedDeclaration],
    budget: &mut SourceBudget<'_>,
) -> Result<SourceEvidence, DefinitionMappingError> {
    let evidence = update_source::collect(module, budget)?;
    for body in &evidence.bodies {
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
    use kagari_common::cancellation::CancellationToken;
    use kagari_hir::analyze_source;
    use kagari_source::source::SourceFile;
    use kagari_stdlib::catalog;

    #[test]
    fn source_adapter_checks_provenance_inventory_without_collecting_function_contents() {
        let module = analyze_source(
            &SourceFile::new(
                "adapter.kgr",
                "fn unused<T>(x: T) -> T { x } static val N: i32 = ((1 + 2));",
            ),
            catalog::shared().unwrap(),
        )
        .unwrap()
        .into_codegen()
        .unwrap();
        let cancel = CancellationToken::default();
        assert_eq!(
            collect(&module, &[], &mut SourceBudget::new(&cancel)),
            Err(DefinitionMappingError::InvalidContract)
        );
        let inventory = module_contract(&module, &cancel).unwrap();
        let records = collect(
            &module,
            &inventory.records().named_declarations,
            &mut SourceBudget::new(&cancel),
        )
        .unwrap();
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
