//! Ordered aggregate sites shared by physical preparation and normalized linking.
use kagari_bytecode::instruction::{BytecodeInstruction, EnumId, StructId};
use kagari_types::ty::Ty;

/// Position in a function's layout-site table, not its deduplicated layout pool.
/// Definition normalization preserves instruction order and therefore this ordinal.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LayoutSite(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AggregateKind {
    Struct(StructId),
    Enum(EnumId, u32),
}

/// Borrowed canonical input; identity-bearing types never enter physical descriptors.
pub(crate) struct AggregateOperand<'a> {
    pub kind: AggregateKind,
    pub arguments: &'a [Ty],
}

impl<'a> AggregateOperand<'a> {
    pub(crate) fn from_instruction(instruction: &'a BytecodeInstruction) -> Option<Self> {
        let (kind, arguments) = match instruction {
            BytecodeInstruction::MakeStruct {
                structure,
                arguments,
                ..
            } => (AggregateKind::Struct(*structure), arguments),
            BytecodeInstruction::MakeEnum {
                enumeration,
                arguments,
                variant,
                ..
            }
            | BytecodeInstruction::TestEnumVariant {
                enumeration,
                arguments,
                variant,
                ..
            }
            | BytecodeInstruction::ReadEnumPayload {
                enumeration,
                arguments,
                variant,
                ..
            } => (AggregateKind::Enum(*enumeration, *variant), arguments),
            BytecodeInstruction::ReadAggregateField { field, .. }
            | BytecodeInstruction::WriteAggregateField { field, .. } => {
                (AggregateKind::Struct(field.structure), &field.arguments)
            }
            _ => return None,
        };
        Some(Self { kind, arguments })
    }
}
