//! Field contracts keep variable type arguments outside the compact instruction stream.
use crate::module::execution::{
    aggregates::LayoutSite,
    layout::{FrameLayout, Location},
};
use kagari_bytecode::instruction::BytecodeInstruction;
use kagari_types::{reference::ReferenceAccess, scalar::BuiltinType};

#[derive(Debug, Clone, Copy)]
pub(crate) enum FieldAccess {
    Read { dst: Location },
    Write { value: Location },
}

#[derive(Debug)]
pub(crate) struct PreparedFieldOperation {
    pub(crate) base: Location,
    pub(crate) access: FieldAccess,
    pub(crate) slot: u32,
    pub(crate) layout_site: LayoutSite,
    pub(crate) reference: Option<ReferenceAccess>,
    pub(crate) scalar: Option<BuiltinType>,
}

impl PreparedFieldOperation {
    pub(super) fn prepare(
        layout_site: LayoutSite,
        instruction: &BytecodeInstruction,
        slot_layout: &FrameLayout,
    ) -> Self {
        let (base, field, access) = match instruction {
            BytecodeInstruction::ReadAggregateField { dst, base, field } => (
                base,
                field,
                FieldAccess::Read {
                    dst: slot_layout
                        .location(dst.index())
                        .expect("sealed field destination"),
                },
            ),
            BytecodeInstruction::WriteAggregateField { base, field, value } => (
                base,
                field,
                FieldAccess::Write {
                    value: slot_layout
                        .location(value.index())
                        .expect("sealed field value"),
                },
            ),
            _ => unreachable!("field preparation only"),
        };
        let scalar_location = match access {
            FieldAccess::Read { dst } => dst,
            FieldAccess::Write { value } => value,
        };
        let scalar = (!scalar_location.operand.managed())
            .then_some(scalar_location.semantic)
            .flatten();
        Self {
            base: slot_layout
                .location(base.index())
                .expect("sealed field base"),
            access,
            layout_site,
            slot: field.slot,
            reference: field.reference,
            scalar,
        }
    }
}
