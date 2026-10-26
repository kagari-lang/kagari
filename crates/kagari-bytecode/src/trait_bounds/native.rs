//! Admit native occurrences only after the existing declaration and call checks.
use crate::{
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::{BytecodeModule, CallableTarget},
    program::{
        BytecodeProgram, ModuleRef,
        native::{NativeApplicationOccurrence, NativeApplicationWitness},
    },
    trait_bounds::preparation::VerificationScope,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::types::{applications::native::NativeApplications, type_contract};
use kagari_types::ty::substitution::TypeTransformError;

pub(super) fn witnesses(
    index: usize,
    module: &BytecodeModule,
    program: Option<&BytecodeProgram>,
    scope: &VerificationScope<'_>,
    cancel: &CancellationToken,
) -> Result<Vec<NativeApplicationWitness>, TypeTransformError> {
    let check = NativeApplications::new(
        &scope.catalog,
        |id| {
            scope.modules.iter().find_map(|owner| {
                type_contract(
                    &owner.identity,
                    &owner.public_items,
                    id,
                    scope.catalog.definitions(),
                )
                .or_else(|| {
                    owner
                        .private_types
                        .iter()
                        .find(|record| &record.declaration == id)
                        .map(|record| &record.definition)
                })
            })
        },
        cancel,
    );
    let mut witnesses = Vec::new();
    for (slot, import) in module.native_imports.iter().enumerate() {
        for nominal in check.import(import)? {
            witnesses.push(NativeApplicationWitness::new(
                ModuleRef::new(index),
                NativeApplicationOccurrence::Import(NativeImportId::new(slot)),
                nominal,
                import.generic.clone(),
            ));
        }
    }
    for function in &module.functions {
        for (offset, instruction) in function.instructions.iter().enumerate() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            let BytecodeInstruction::Call {
                callee:
                    CallTarget::Shared {
                        module: owner,
                        target: CallableTarget::Native(slot),
                        contract,
                    },
                ..
            } = instruction
            else {
                continue;
            };
            let owner = program
                .and_then(|program| program.modules.get(owner.index()))
                .or_else(|| (program.is_none() && owner.index() == 0).then_some(module))
                .ok_or(TypeTransformError::InvalidContract)?;
            let import = owner
                .native_imports
                .get(slot.index())
                .ok_or(TypeTransformError::InvalidContract)?;
            let binder = function.metadata.semantic.generic.as_ref();
            for nominal in check.shared(import, contract, binder)? {
                witnesses.push(NativeApplicationWitness::new(
                    ModuleRef::new(index),
                    NativeApplicationOccurrence::SharedCall {
                        function: function.id,
                        offset,
                    },
                    nominal,
                    binder.cloned(),
                ));
            }
        }
    }
    Ok(witnesses)
}
