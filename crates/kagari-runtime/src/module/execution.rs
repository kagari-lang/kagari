//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with the canonical code. Identity-bearing and
//! variable-length operands stay in that immutable code, addressed by the PC.
pub(crate) mod allocation;
pub(crate) mod calls;
pub(crate) mod fields;
pub(crate) mod indices;
pub(crate) mod layout;
pub mod managed;

use crate::{
    frame::values::scalar,
    module::execution::{
        calls::PreparedCall,
        fields::PreparedFieldOperation,
        indices::{IndexAccess, PreparedIndexOperation},
        layout::{FrameLayout, Location, scalar_type},
        managed::{ManagedOperation, PreparedManagedOperation},
    },
    numeric::binary_operation,
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, JumpTarget, Register, UnaryOp,
    },
    module::BytecodeModule,
    suspension::AwaitLiveness,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::payload::{self, ScalarKernel};
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
    Managed(PreparedManagedOperation),
    Return {
        value: Option<ScalarSlot>,
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
    pub fields: Box<[PreparedFieldOperation]>,
    pub indices: Box<[PreparedIndexOperation]>,
    pub native_calls: usize,
    pub interface_calls: usize,
    pub has_closed_interface_calls: bool,
    has_linked_constants: bool,
    pub(crate) has_scoped_fields: bool,
}

impl ExecutionFunction {
    pub(crate) fn needs_runtime_links(&self) -> bool {
        self.has_closed_interface_calls
            || self.has_linked_constants
            || self.has_scoped_fields
            || self.native_calls != 0
    }
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
                    let mut fields = Vec::new();
                    let mut indices = Vec::new();
                    let mut native_calls = 0;
                    let instructions: Box<[_]> = function
                        .instructions
                        .iter()
                        .enumerate()
                        .map(|(pc, instruction)| {
                            ExecutionInstruction::prepare(
                                pc,
                                instruction,
                                &registers,
                                module,
                                &mut fields,
                                &mut indices,
                                &mut native_calls,
                            )
                        })
                        .collect();
                    ExecutionFunction {
                        has_linked_constants: instructions.iter().any(|operation| {
                            matches!(
                                operation,
                                ExecutionInstruction::Managed(PreparedManagedOperation(
                                    ManagedOperation::Constant { .. }
                                ))
                            )
                        }),
                        instructions,
                        registers,
                        awaits,
                        calls: BTreeMap::new(),
                        has_scoped_fields: fields.iter().any(|field| !field.concrete),
                        fields: fields.into_boxed_slice(),
                        indices: indices.into_boxed_slice(),
                        native_calls,
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
        pc: usize,
        instruction: &BytecodeInstruction<DefinitionId>,
        registers: &FrameLayout,
        module: &BytecodeModule<DefinitionId>,
        fields: &mut Vec<PreparedFieldOperation>,
        indices: &mut Vec<PreparedIndexOperation>,
        native_calls: &mut usize,
    ) -> Self {
        let location = |register: Register| {
            registers
                .location(register.index())
                .expect("verified register")
        };
        let slot = |register: Register| location(register).operand;
        let local_location = |index: usize| {
            registers
                .location(registers.register_count + index)
                .expect("verified local")
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
            BytecodeInstruction::Call {
                callee: CallTarget::Native(_),
                ..
            } => {
                let index = *native_calls;
                *native_calls += 1;
                Self::managed(ManagedOperation::Native { index })
            }
            BytecodeInstruction::LoadConst { dst, constant } => {
                let value = match module.constants[constant.index()] {
                    ConstantOperand::Unit => Value::Unit,
                    ConstantOperand::Bool(v) => Value::Bool(v),
                    ConstantOperand::I32(v) => Value::I32(v),
                    ConstantOperand::I64(v) => Value::I64(v),
                    ConstantOperand::U64(v) => Value::U64(v),
                    ConstantOperand::F32(v) => Value::F32(v),
                    ConstantOperand::F64(v) => Value::F64(v),
                    ConstantOperand::Str(_) => {
                        return Self::managed(ManagedOperation::Constant {
                            dst: location(dst),
                            constant,
                        });
                    }
                };
                let Some(dst) = slot(dst).scalar() else {
                    return Self::managed(ManagedOperation::Constant {
                        dst: location(dst),
                        constant,
                    });
                };
                Self::Constant {
                    dst,
                    value: scalar::encode(&value).expect("scalar constant"),
                }
            }
            BytecodeInstruction::LoadLocal { dst, local } => {
                Self::move_slots(location(dst), local_location(local.index()))
            }
            BytecodeInstruction::StoreLocal { local, src } => {
                Self::move_slots(local_location(local.index()), location(src))
            }
            BytecodeInstruction::Move { dst, src } => {
                Self::move_slots(location(dst), location(src))
            }
            BytecodeInstruction::ReadAggregateField { .. }
            | BytecodeInstruction::WriteAggregateField { .. } => {
                let index = fields.len();
                fields.push(PreparedFieldOperation::prepare(
                    pc,
                    instruction,
                    registers,
                    module,
                ));
                Self::managed(ManagedOperation::Field { index })
            }
            BytecodeInstruction::Unary { dst, op, operand } => {
                let ty = scalar_type(location(operand).representation);
                let kernel = match op {
                    UnaryOp::Neg => ty.and_then(payload::negation_kernel),
                    UnaryOp::Not => Some(payload::boolean_not as ScalarKernel),
                };
                scalar(dst, operand, operand, kernel)
            }
            BytecodeInstruction::ReadAggregateIndex { dst, base, index } => {
                let ordinal = indices.len();
                indices.push(PreparedIndexOperation {
                    base: location(base),
                    index: location(index),
                    access: IndexAccess::Read { dst: location(dst) },
                });
                Self::managed(ManagedOperation::Index { index: ordinal })
            }
            BytecodeInstruction::WriteAggregateIndex { base, index, value } => {
                let ordinal = indices.len();
                indices.push(PreparedIndexOperation {
                    base: location(base),
                    index: location(index),
                    access: IndexAccess::Write {
                        value: location(value),
                    },
                });
                Self::managed(ManagedOperation::Index { index: ordinal })
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
            BytecodeInstruction::Return(Some(value)) if slot(value).managed() => {
                Self::managed(ManagedOperation::Return { value: slot(value) })
            }
            BytecodeInstruction::Return(value) => Self::Return {
                value: value.map(|value| slot(value).scalar().expect("classified scalar return")),
                representation: value.map_or(ValueType::Unit, |register| {
                    location(register).representation
                }),
            },
            _ => Self::Boundary,
        }
    }

    fn move_slots(dst: Location, src: Location) -> Self {
        match (dst.operand.scalar(), src.operand.scalar()) {
            (Some(dst), Some(src)) => Self::Move { dst, src },
            _ => Self::managed(ManagedOperation::Copy { dst, src }),
        }
    }

    fn managed(operation: ManagedOperation) -> Self {
        Self::Managed(PreparedManagedOperation(operation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        frame::{
            ExecutionFrame,
            cursor::kernel::{RegionError, RegionExit},
        },
        module::linked_execution::{LinkedFunction, LinkedPrimitive},
        native::primitive::PrimitiveResult,
    };
    use std::{mem::size_of, sync::OnceLock};

    #[test]
    fn physical_instruction_budget() {
        assert!(size_of::<ExecutionInstruction>() <= 24);
        eprintln!(
            "linked primitive bytes: slot={}, operand={}, result={}",
            size_of::<Option<LinkedPrimitive>>(),
            size_of::<Location>(),
            size_of::<PrimitiveResult>()
        );
        eprintln!(
            "region bytes: error={}, exit={}, result={}",
            size_of::<RegionError>(),
            size_of::<RegionExit>(),
            size_of::<Result<RegionExit, RegionError>>()
        );
        eprintln!(
            "execution metadata bytes: instruction={}, frame={}, linked_function={}, field_operation={}, index_operation={}, execution_function={}, constant_cell={}, previous_constant_cell={}",
            size_of::<ExecutionInstruction>(),
            size_of::<ExecutionFrame>(),
            size_of::<LinkedFunction>(),
            size_of::<PreparedFieldOperation>(),
            size_of::<PreparedIndexOperation>(),
            size_of::<ExecutionFunction>(),
            size_of::<OnceLock<Value>>(),
            size_of::<Option<Value>>()
        );
    }
}
