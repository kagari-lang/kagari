//! Compare checked declarations as well as body tokens before pairing independent products.
use crate::source::lower::module_contract;
use kagari_bytecode::{
    program::verified::VerifiedBytecodeProgram, source_package::SourcePackageError,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::RemapDefinitions,
    },
};
use kagari_hir::program::CheckedProgram;

pub(super) fn validate(
    checked: &CheckedProgram,
    bytecode: &VerifiedBytecodeProgram,
    cancel: &CancellationToken,
) -> Result<(), SourcePackageError> {
    let definitions = bytecode.definitions();
    for module in checked.modules() {
        check_cancel(cancel)?;
        let contract = module_contract(module, cancel);
        check_cancel(cancel)?;
        let contract = contract.map_err(|_| SourcePackageError::DeclarationMismatch)?;
        let records = contract
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    let path = contract.definitions().resolve(*id)?.to_path();
                    definitions
                        .lookup(&path)
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                definitions,
                cancel,
            ))?;
        let compiled = bytecode
            .program()
            .modules
            .iter()
            .find(|compiled| &compiled.identity == module.lowered.source.module_identity())
            .ok_or(SourcePackageError::DeclarationMismatch)?;
        // Lowering can add synthesized placement contracts. Every authored contract,
        // including private/unused declarations, must nevertheless match exactly.
        if !records
            .named_declarations
            .iter()
            .all(|record| compiled.named_declarations.contains(record))
            || !records
                .native_declarations
                .iter()
                .all(|record| compiled.native_declarations.contains(record))
            || !records
                .public_items
                .iter()
                .all(|record| compiled.public_items.contains(record))
            || !records
                .private_types
                .iter()
                .all(|record| compiled.private_types.contains(record))
            || !records
                .trait_contracts
                .iter()
                .all(|record| compiled.trait_contracts.contains(record))
        {
            return Err(SourcePackageError::DeclarationMismatch);
        }
    }
    Ok(())
}
