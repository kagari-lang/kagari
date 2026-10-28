//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with canonical code. Caller ordinals and physical
//! operands are shared across definition scopes. Runtime links own identity-bearing
//! call contracts; other cold boundary operands remain in canonical code.
pub(crate) mod aggregates;
pub(crate) mod allocation;
pub(crate) mod calls;
pub(crate) mod fields;
pub(crate) mod frames;
pub(crate) mod indices;
pub(crate) mod layout;
pub mod managed;
pub(crate) mod scalar;

use crate::{
    module::execution::{
        aggregates::{AggregateOperand, LayoutSite},
        calls::PreparedCall,
        fields::PreparedFieldOperation,
        frames::CalleeLayout,
        indices::{IndexAccess, PreparedIndexOperation},
        layout::{FrameLayout, Location},
        managed::{ManagedOperation, PreparedManagedOperation},
    },
    value::{Value, scalar as scalar_value},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, ConstantOperand, JumpTarget, Slot},
    module::{BytecodeModule, CallableTarget},
    suspension::AwaitLiveness,
};
use kagari_types::{
    integer::IntegerOp,
    payload::{
        comparison::{Comparison, ComparisonDomain},
        operation::{FloatOperation, FloatType, ShiftKind},
    },
    scalar::IntegerType,
};
use std::collections::BTreeMap;

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

macro_rules! define_execution {
    (binary { $(($bp:pat, $brr:ident, $brk:ident, $bt:ty, $be:path),)* }
     shift { $(($sk:ident, $srr:ident, $srk:ident, $se:path),)* }
     compare { $(($cd:ident, $crr:ident, $crk:ident, $jrr:ident, $jrk:ident, $ct:ty, $ce:path),)* }
     simple_compare { $(($sd:ident, $qrr:ident, $qjrr:ident, $qe:path),)* }
     unary { $(($u:ident, $ur:ident, $ue:path),)* }
     convert { $(($cv:ident, $cs:ty, $ctarget:ty),)* }) => {
        /// Closed prepared operations with admitted physical operands.
        #[derive(Debug, Clone, Copy)]
        pub enum ExecutionInstruction {
            Constant { dst: ScalarSlot, value: u64 },
            Move { dst: ScalarSlot, src: ScalarSlot },
            $(
                $brr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot },
                $brk { dst: ScalarSlot, lhs: ScalarSlot, rhs: $bt },
            )*
            $(
                $srr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, count_domain: IntegerType },
                $srk { dst: ScalarSlot, lhs: ScalarSlot, rhs: u64, count_domain: IntegerType },
            )*
            $(
                $crr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, condition: Comparison },
                $crk { dst: ScalarSlot, lhs: ScalarSlot, rhs: $ct, condition: Comparison },
                $jrr { lhs: ScalarSlot, rhs: ScalarSlot, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
                $jrk { lhs: ScalarSlot, rhs: $ct, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
            )*
            $(
                $qrr { dst: ScalarSlot, lhs: ScalarSlot, rhs: ScalarSlot, condition: Comparison },
                $qjrr { lhs: ScalarSlot, rhs: ScalarSlot, then_target: JumpTarget, else_target: JumpTarget, condition: Comparison },
            )*
            $($ur { dst: ScalarSlot, src: ScalarSlot },)*
            $($cv { dst: ScalarSlot, src: ScalarSlot, source: $cs, target: $ctarget },)*
            BoolToInteger { dst: ScalarSlot, src: ScalarSlot, target: IntegerType },
            IntegerBinaryKR { dst: ScalarSlot, lhs: u64, rhs: ScalarSlot, op: IntegerOp, domain: IntegerType },
            IntegerBinaryKK { dst: ScalarSlot, lhs: u64, rhs: u64, op: IntegerOp, domain: IntegerType },
            FloatBinaryKR { dst: ScalarSlot, lhs: u64, rhs: ScalarSlot, op: FloatOperation, domain: FloatType },
            FloatBinaryKK { dst: ScalarSlot, lhs: u64, rhs: u64, op: FloatOperation, domain: FloatType },
            ShiftKR { dst: ScalarSlot, lhs: u64, rhs: ScalarSlot, kind: ShiftKind, count_domain: IntegerType },
            ShiftKK { dst: ScalarSlot, lhs: u64, rhs: u64, kind: ShiftKind, count_domain: IntegerType },
            CompareStoreKR { dst: ScalarSlot, lhs: u64, rhs: ScalarSlot, domain: ComparisonDomain, condition: Comparison },
            CompareStoreKK { dst: ScalarSlot, lhs: u64, rhs: u64, domain: ComparisonDomain, condition: Comparison },
            CompareBranchKR { lhs: u64, rhs: ScalarSlot, domain: ComparisonDomain, condition: Comparison, then_target: JumpTarget, else_target: JumpTarget },
            Jump(JumpTarget),
            Branch { cond: ScalarSlot, then_target: JumpTarget, else_target: JumpTarget },
            Managed(PreparedManagedOperation),
            Return { value: Option<ScalarSlot>, representation: ValueType },
            /// Release the cursor before canonical operations requiring a boundary.
            Boundary,
        }
    };
}

scalar_catalog!(define_execution);

#[derive(Debug)]
pub(crate) struct ExecutionModule {
    pub functions: Vec<ExecutionFunction>,
    pub native_frames: Vec<CalleeLayout>,
}

#[derive(Debug)]
pub(crate) struct ExecutionFunction {
    pub instructions: Box<[ExecutionInstruction]>,
    pub frame: CalleeLayout,
    /// Managed physical locations retained immediately before an await. Slot
    /// coalescing may share a location: any live logical alias keeps it alive.
    pub awaits: BTreeMap<usize, Box<[u64]>>,
    pub calls: Box<[PreparedCall]>,
    pub fields: Box<[PreparedFieldOperation]>,
    pub indices: Box<[PreparedIndexOperation]>,
    pub native_calls: usize,
    pub has_call_contracts: bool,
    has_linked_constants: bool,
    pub(crate) has_layout_operands: bool,
}

impl ExecutionFunction {
    pub(crate) fn needs_runtime_links(&self) -> bool {
        self.has_call_contracts
            || self.has_linked_constants
            || self.has_layout_operands
            || self.native_calls != 0
    }
}

#[derive(Default)]
struct OperationTables {
    fields: Vec<PreparedFieldOperation>,
    indices: Vec<PreparedIndexOperation>,
}

impl ExecutionModule {
    pub(crate) fn callee(&self, target: CallableTarget) -> Option<&CalleeLayout> {
        match target {
            CallableTarget::Script(function) => Some(&self.functions.get(function.index())?.frame),
            CallableTarget::Native(import) => self.native_frames.get(import.index()),
        }
    }

    // Only VerifiedProgram constructs this product, after artifact verification.
    // Identity normalization preserves instruction order and physical operands,
    // so the product can be shared across runtime-local definition scopes.
    pub(super) fn prepare(
        module: &BytecodeModule,
        suspensions: &[Vec<AwaitLiveness>],
        work: &mut usize,
    ) -> Self {
        Self {
            native_frames: module
                .native_imports
                .iter()
                .map(CalleeLayout::native)
                .collect(),
            functions: module
                .functions
                .iter()
                .zip(suspensions)
                .map(|(function, suspensions)| {
                    let frame = CalleeLayout::script(function, work);
                    let slot_layout = &frame.slot_layout;
                    let awaits = suspensions
                        .iter()
                        .map(|point| {
                            let mut retained = vec![0; slot_layout.managed_count.div_ceil(64)];
                            for logical in point.live_slots() {
                                let slot = slot_layout.locations[logical].operand;
                                if slot.managed() {
                                    retained[slot.index() / 64] |= 1 << (slot.index() % 64);
                                }
                            }
                            (point.instruction(), retained.into_boxed_slice())
                        })
                        .collect();
                    let mut sites = OperationTables::default();
                    let mut native_calls = 0;
                    let mut layout_sites = 0;
                    let instructions: Box<[_]> = function
                        .instructions
                        .iter()
                        .map(|instruction| {
                            let layout_site =
                                AggregateOperand::from_instruction(instruction).map(|_| {
                                    let site = LayoutSite(layout_sites);
                                    layout_sites += 1;
                                    site
                                });
                            if let Some(operation) =
                                scalar::prepare(instruction, function, module, slot_layout)
                            {
                                return operation;
                            }
                            ExecutionInstruction::prepare(
                                layout_site,
                                instruction,
                                slot_layout,
                                module,
                                &mut sites.fields,
                                &mut sites.indices,
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
                        frame,
                        awaits,
                        calls: Box::default(),
                        has_layout_operands: layout_sites != 0,
                        fields: sites.fields.into_boxed_slice(),
                        indices: sites.indices.into_boxed_slice(),
                        native_calls,
                        has_call_contracts: false,
                    }
                })
                .collect(),
        }
    }
}

impl ExecutionInstruction {
    fn prepare(
        layout_site: Option<LayoutSite>,
        instruction: &BytecodeInstruction,
        slot_layout: &FrameLayout,
        module: &BytecodeModule,
        fields: &mut Vec<PreparedFieldOperation>,
        indices: &mut Vec<PreparedIndexOperation>,
        native_calls: &mut usize,
    ) -> Self {
        let location = |register: Slot| {
            slot_layout
                .location(register.index())
                .expect("verified register")
        };
        let slot = |register: Slot| location(register).operand;

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
                    ConstantOperand::Char(v) => Value::Char(v),
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
                    value: scalar_value::encode(&value).expect("scalar constant"),
                }
            }
            BytecodeInstruction::Move { dst, src } => {
                Self::move_slots(location(dst), location(src))
            }
            BytecodeInstruction::ReadAggregateField { .. }
            | BytecodeInstruction::WriteAggregateField { .. } => {
                let index = fields.len();
                fields.push(PreparedFieldOperation::prepare(
                    layout_site.expect("field has an aggregate operand"),
                    instruction,
                    slot_layout,
                ));
                Self::managed(ManagedOperation::Field { index })
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
        module::linked_execution::{
            LinkedFunction, LinkedNativeCall,
            layouts::{AggregateLayout, AppliedLayouts, FunctionLayouts},
        },
        native::primitive::PrimitiveResult,
    };
    use std::{mem::size_of, sync::OnceLock};

    #[test]
    fn physical_instruction_budget() {
        assert!(size_of::<ExecutionInstruction>() <= 24);
        eprintln!(
            "layout metadata bytes: function={}, application={}, operand={}, application_cell={}",
            size_of::<FunctionLayouts>(),
            size_of::<AppliedLayouts>(),
            size_of::<AggregateLayout>(),
            size_of::<OnceLock<AggregateLayout>>()
        );
        eprintln!(
            "linked native bytes: slot={}, operand={}, result={}",
            size_of::<LinkedNativeCall>(),
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
