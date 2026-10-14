//! Field contracts keep variable type arguments outside the compact instruction stream.
use crate::module::{
    LoadedModule, StructLayoutRef,
    execution::layout::{FrameLayout, Location},
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, StructId},
    module::BytecodeModule,
};
use kagari_types::{reference::ReferenceAccess, ty::Ty};

#[derive(Debug, Clone, Copy)]
pub(crate) enum FieldAccess {
    Read { dst: Location },
    Write { value: Location },
}

#[derive(Debug)]
pub(crate) struct PreparedFieldOperation {
    pub(crate) base: Location,
    pub(crate) access: FieldAccess,
    pub(crate) structure: StructId,
    pub(crate) slot: u32,
    pub(crate) pc: usize,
    pub(crate) concrete: bool,
    pub(crate) reference: Option<ReferenceAccess>,
}

impl PreparedFieldOperation {
    pub(super) fn prepare(
        pc: usize,
        instruction: &BytecodeInstruction,
        registers: &FrameLayout,
        module: &BytecodeModule,
    ) -> Self {
        let (base, field, access) = match instruction {
            BytecodeInstruction::ReadAggregateField { dst, base, field } => (
                base,
                field,
                FieldAccess::Read {
                    dst: registers
                        .location(dst.index())
                        .expect("sealed field destination"),
                },
            ),
            BytecodeInstruction::WriteAggregateField { base, field, value } => (
                base,
                field,
                FieldAccess::Write {
                    value: registers
                        .location(value.index())
                        .expect("sealed field value"),
                },
            ),
            _ => unreachable!("field preparation only"),
        };
        let layout = &module.structures[field.structure.index()];
        Self {
            base: registers.location(base.index()).expect("sealed field base"),
            access,
            concrete: layout.arguments == field.arguments
                && layout.arguments.iter().all(Ty::is_concrete),
            pc,
            structure: field.structure,
            slot: field.slot,
            reference: field.reference,
        }
    }

    /// Shared verified metadata contains no runtime identities. The admitted
    /// frame supplies the exact version; applied layouts require a transition.
    pub(crate) fn concrete_layout(&self, owner: &LoadedModule) -> Option<StructLayoutRef> {
        self.concrete.then(|| StructLayoutRef {
            module: owner.clone(),
            id: self.structure,
            applied: None,
            canonical: Some(owner.program.layouts.structure(owner.slot, self.structure)),
            scope: None,
        })
    }
}
