//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with the canonical code. Identity-bearing and
//! variable-length operands stay in that immutable code, addressed by the PC.
pub(crate) mod allocation;
pub(crate) mod calls;
pub(crate) mod layout;

use crate::{
    frame::values::scalar,
    module::{
        LoadedModule, StructLayoutRef,
        execution::{
            calls::PreparedCall,
            layout::{FrameLayout, scalar_type},
        },
    },
    numeric::binary_operation,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, ConstantOperand, FieldRef, JumpTarget, Register, StructId,
        UnaryOp,
    },
    module::BytecodeModule,
    suspension::AwaitLiveness,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::representation::semantic_representation;
use kagari_types::{
    payload::{self, ScalarKernel},
    ty::Ty,
};
use std::{collections::BTreeMap, sync::Arc};

/// A bounded physical operand in a prepared function's value window.
#[derive(Debug, Clone, Copy)]
pub struct OperandSlot(u32);

impl OperandSlot {
    pub(crate) fn new(index: usize, managed: bool) -> Self {
        // Sealed register/local counts are u16; this bit only classifies frame
        // offsets and never truncates a runtime owner/slot/generation identity.
        Self(index as u32 | if managed { 1 << 31 } else { 0 })
    }

    pub(crate) fn index(self) -> usize {
        (self.0 & !(1 << 31)) as usize
    }

    pub(crate) fn managed(self) -> bool {
        self.0 & (1 << 31) != 0
    }

    pub(crate) fn scalar(self) -> Option<ScalarSlot> {
        (!self.managed()).then_some(ScalarSlot(self.0))
    }
}

/// A sealed scalar-bank offset. General locations prove their class once during
/// preparation; scalar execution cannot reinterpret a managed offset as payload.
#[derive(Debug, Clone, Copy)]
pub struct ScalarSlot(u32);

impl ScalarSlot {
    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PreparedField {
    structure: StructId,
    pub(crate) slot: u32,
    pub(crate) representation: ValueType,
}

impl PreparedField {
    fn prepare(
        module: &BytecodeModule<DefinitionId>,
        field: &FieldRef<DefinitionId>,
    ) -> Option<Self> {
        let layout = module.structures.get(field.structure.index())?;
        if layout.arguments != field.arguments || !layout.arguments.iter().all(Ty::is_concrete) {
            return None;
        }
        Some(Self {
            structure: field.structure,
            slot: field.slot,
            representation: semantic_representation(&layout.fields.get(field.slot as usize)?.ty),
        })
    }

    /// Only sealed concrete layouts use this record. The executing frame supplies
    /// its exact loaded owner; shared code never stores a runtime identity.
    pub(crate) fn layout(self, owner: &LoadedModule) -> StructLayoutRef {
        StructLayoutRef {
            module: owner.clone(),
            id: self.structure,
            applied: None,
            canonical: Some(owner.program.layouts.structure(owner.slot, self.structure)),
            scope: None,
        }
    }
}

/// The same sealed field description serves preparation and the scalar/object handoff.
#[derive(Debug, Clone, Copy)]
pub enum PreparedFieldOperation {
    Read {
        dst: Register,
        base: Register,
        field: PreparedField,
    },
    Write {
        base: Register,
        value: Register,
        field: PreparedField,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum ExecutionInstruction {
    Constant {
        dst: ScalarSlot,
        value: u64,
    },
    Move {
        dst: ScalarSlot,
        src: ScalarSlot,
    },
    Scalar {
        dst: ScalarSlot,
        lhs: ScalarSlot,
        rhs: ScalarSlot,
        kernel: ScalarKernel,
    },
    Jump(JumpTarget),
    Branch {
        cond: ScalarSlot,
        then_target: JumpTarget,
        else_target: JumpTarget,
    },
    Field(PreparedFieldOperation),
    Return {
        value: Option<OperandSlot>,
        representation: ValueType,
    },
    /// Consult the canonical instruction at the current logical PC after
    /// releasing the cursor, before a potentially allocating/reentrant operation.
    Boundary,
}

#[derive(Debug)]
pub(crate) struct ExecutionModule {
    pub functions: Vec<ExecutionFunction>,
    pub native_layouts: Vec<Arc<FrameLayout>>,
}

#[derive(Debug)]
pub(crate) struct ExecutionFunction {
    pub instructions: Box<[ExecutionInstruction]>,
    pub registers: Arc<FrameLayout>,
    /// Managed physical locations retained immediately before an await. Slot
    /// coalescing may share a location: any live logical alias keeps it alive.
    pub awaits: BTreeMap<usize, Box<[u64]>>,
    pub calls: BTreeMap<usize, PreparedCall>,
    pub interface_calls: usize,
    pub has_closed_interface_calls: bool,
}

impl ExecutionModule {
    // Only VerifiedProgram constructs this product, after artifact verification.
    // Identity normalization preserves instruction order and physical operands,
    // so the product can be shared across runtime-local definition scopes.
    pub(super) fn prepare(
        module: &BytecodeModule<DefinitionId>,
        suspensions: &[Vec<AwaitLiveness>],
        work: &mut usize,
    ) -> Self {
        Self {
            native_layouts: module
                .native_imports
                .iter()
                .map(|import| Arc::new(FrameLayout::native(import)))
                .collect(),
            functions: module
                .functions
                .iter()
                .zip(suspensions)
                .map(|(function, suspensions)| {
                    let registers = Arc::new(FrameLayout::prepare(function, work));
                    let awaits = suspensions
                        .iter()
                        .map(|point| {
                            let mut retained = vec![0; registers.managed_count.div_ceil(64)];
                            for logical in point.live_slots() {
                                let slot = registers.locations[logical].operand;
                                if slot.managed() {
                                    retained[slot.index() / 64] |= 1 << (slot.index() % 64);
                                }
                            }
                            (point.instruction(), retained.into_boxed_slice())
                        })
                        .collect();
                    let instructions = function
                        .instructions
                        .iter()
                        .map(|instruction| {
                            ExecutionInstruction::prepare(instruction, &registers, module)
                        })
                        .collect();
                    ExecutionFunction {
                        instructions,
                        registers,
                        awaits,
                        calls: BTreeMap::new(),
                        interface_calls: 0,
                        has_closed_interface_calls: false,
                    }
                })
                .collect(),
        }
    }
}

impl ExecutionInstruction {
    fn prepare(
        instruction: &BytecodeInstruction<DefinitionId>,
        registers: &FrameLayout,
        module: &BytecodeModule<DefinitionId>,
    ) -> Self {
        let location = |register: Register| {
            registers
                .location(register.index())
                .expect("verified register")
        };
        let slot = |register: Register| location(register).operand;
        let local_slot = |index: usize| {
            registers
                .location(registers.register_count + index)
                .expect("verified local")
                .operand
        };
        let scalar = |dst: Register, lhs: Register, rhs: Register, kernel: Option<ScalarKernel>| {
            let Some(kernel) = kernel else {
                return Self::Boundary;
            };
            let (Some(dst), Some(lhs), Some(rhs)) =
                (slot(dst).scalar(), slot(lhs).scalar(), slot(rhs).scalar())
            else {
                return Self::Boundary;
            };
            Self::Scalar {
                dst,
                lhs,
                rhs,
                kernel,
            }
        };

        match *instruction {
            BytecodeInstruction::LoadConst { dst, constant } => {
                let value = match module.constants[constant.index()] {
                    ConstantOperand::Unit => Value::Unit,
                    ConstantOperand::Bool(v) => Value::Bool(v),
                    ConstantOperand::I32(v) => Value::I32(v),
                    ConstantOperand::I64(v) => Value::I64(v),
                    ConstantOperand::U64(v) => Value::U64(v),
                    ConstantOperand::F32(v) => Value::F32(v),
                    ConstantOperand::F64(v) => Value::F64(v),
                    ConstantOperand::Str(_) => return Self::Boundary,
                };
                let Some(dst) = slot(dst).scalar() else {
                    return Self::Boundary;
                };
                Self::Constant {
                    dst,
                    value: scalar::encode(&value).expect("scalar constant"),
                }
            }
            BytecodeInstruction::LoadLocal { dst, local } => {
                Self::move_slots(slot(dst), local_slot(local.index()))
            }
            BytecodeInstruction::StoreLocal { local, src } => {
                Self::move_slots(local_slot(local.index()), slot(src))
            }
            BytecodeInstruction::Move { dst, src } => Self::move_slots(slot(dst), slot(src)),
            BytecodeInstruction::ReadAggregateField {
                dst,
                base,
                ref field,
            } => PreparedField::prepare(module, field).map_or(Self::Boundary, |field| {
                Self::Field(PreparedFieldOperation::Read { dst, base, field })
            }),
            BytecodeInstruction::WriteAggregateField {
                base,
                value,
                ref field,
            } => PreparedField::prepare(module, field).map_or(Self::Boundary, |field| {
                Self::Field(PreparedFieldOperation::Write { base, value, field })
            }),
            BytecodeInstruction::Unary { dst, op, operand } => {
                let ty = scalar_type(location(operand).representation);
                let kernel = match op {
                    UnaryOp::Neg => ty.and_then(payload::negation_kernel),
                    UnaryOp::Not => Some(payload::boolean_not as ScalarKernel),
                };
                scalar(dst, operand, operand, kernel)
            }
            BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
                let kernel = match op {
                    BinaryOp::Numeric(operation) => {
                        payload::integer_kernel(operation.op, operation.input, operation.rhs)
                    }
                    BinaryOp::IdentityEq | BinaryOp::IdentityNotEq => None,
                    _ => scalar_type(location(lhs).representation)
                        .and_then(|ty| payload::binary_kernel(binary_operation(op), ty)),
                };
                scalar(dst, lhs, rhs, kernel)
            }
            BytecodeInstruction::Convert {
                dst,
                src,
                conversion,
            } => scalar(
                dst,
                src,
                src,
                payload::conversion_kernel(conversion.source, conversion.target),
            ),
            BytecodeInstruction::Numeric {
                dst,
                operation,
                lhs,
                rhs,
            } => scalar(
                dst,
                lhs,
                rhs.unwrap_or(lhs),
                payload::integer_kernel(operation.op, operation.input, operation.rhs),
            ),
            BytecodeInstruction::Jump { target } => Self::Jump(target),
            BytecodeInstruction::Branch {
                cond,
                then_target,
                else_target,
            } => Self::Branch {
                cond: slot(cond).scalar().expect("verified bool location"),
                then_target,
                else_target,
            },
            BytecodeInstruction::Return(value) => Self::Return {
                value: value.map(slot),
                representation: value.map_or(ValueType::Unit, |register| {
                    location(register).representation
                }),
            },
            _ => Self::Boundary,
        }
    }

    fn move_slots(dst: OperandSlot, src: OperandSlot) -> Self {
        match (dst.scalar(), src.scalar()) {
            (Some(dst), Some(src)) => Self::Move { dst, src },
            _ => Self::Boundary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn physical_instruction_budget() {
        assert!(size_of::<ExecutionInstruction>() <= 24);
    }
}
