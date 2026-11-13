//! Compact physical operations derived once from sealed, verified bytecode.
//! Logical PCs remain one-to-one with canonical code. Caller ordinals and physical
//! operands are shared across definition scopes. Runtime links own identity-bearing
//! call contracts; other cold boundary operands remain in canonical code.
//! Verified self-tail transfers retain the terminal Return for ordinary fallback.
pub(crate) mod aggregates;
pub(crate) mod allocation;
pub(crate) mod calls;
pub(crate) mod fields;
pub(crate) mod frames;
pub(crate) mod indices;
pub mod instruction;
pub(crate) mod interfaces;
pub(crate) mod layout;
pub mod managed;
pub mod operands;
pub(crate) mod scalar;
pub(crate) mod tail;

use crate::{
    module::execution::{
        aggregates::{AggregateOperand, LayoutSite},
        calls::PreparedCall,
        fields::PreparedFieldOperation,
        frames::CalleeLayout,
        indices::{IndexAccess, PreparedIndexOperation},
        instruction::ExecutionInstruction,
        interfaces::PreparedUpcast,
        layout::{FrameLayout, Location},
        managed::{ManagedOperation, PreparedManagedOperation},
        tail::TailCall,
    },
    value::{carriers::ScopedValue, scalar as scalar_value},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ConstantOperand, Slot},
    module::{BytecodeModule, CallableTarget},
    suspension::AwaitLiveness,
};
use kagari_types::ty::Ty;
use std::collections::BTreeMap;

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
    pub self_tail_calls: Box<[TailCall]>,
    pub fields: Box<[PreparedFieldOperation]>,
    pub indices: Box<[PreparedIndexOperation]>,
    pub upcasts: Box<[PreparedUpcast]>,
    pub has_call_contracts: bool,
    has_linked_constants: bool,
    pub(crate) has_layout_operands: bool,
}

impl ExecutionFunction {
    pub(crate) fn needs_runtime_links(&self) -> bool {
        self.has_call_contracts
            || self.has_linked_constants
            || self.has_layout_operands
            || !self.upcasts.is_empty()
    }
}

#[derive(Default)]
struct OperationTables {
    fields: Vec<PreparedFieldOperation>,
    indices: Vec<PreparedIndexOperation>,
    upcasts: Vec<PreparedUpcast>,
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
                                &mut sites,
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
                        self_tail_calls: Box::default(),
                        has_layout_operands: layout_sites != 0,
                        fields: sites.fields.into_boxed_slice(),
                        indices: sites.indices.into_boxed_slice(),
                        upcasts: sites.upcasts.into_boxed_slice(),
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
        tables: &mut OperationTables,
    ) -> Self {
        let OperationTables {
            fields,
            indices,
            upcasts,
        } = tables;
        let location = |register: Slot| {
            slot_layout
                .location(register.index())
                .expect("verified register")
        };
        let slot = |register: Slot| location(register).operand;

        match *instruction {
            BytecodeInstruction::UpcastInterface {
                dst,
                value,
                ref source,
                ref target,
            } => {
                let index = upcasts.len();
                upcasts.push(PreparedUpcast {
                    dst: location(dst),
                    value: location(value),
                    source: Ty::Trait(source.clone()),
                    target: Ty::Trait(target.clone()),
                });
                Self::managed(ManagedOperation::Upcast { index })
            }
            BytecodeInstruction::LoadConst { dst, constant } => {
                if slot(dst).is_unit() {
                    debug_assert!(matches!(
                        module.constants[constant.index()],
                        ConstantOperand::Unit
                    ));
                    return Self::Unit;
                }
                let value = match module.constants[constant.index()] {
                    ConstantOperand::Unit => ScopedValue::unit(),
                    ConstantOperand::Bool(v) => ScopedValue::from(v),
                    ConstantOperand::Char(v) => ScopedValue::from(v),
                    ConstantOperand::I32(v) => ScopedValue::from(v),
                    ConstantOperand::I64(v) => ScopedValue::from(v),
                    ConstantOperand::U64(v) => ScopedValue::from(v),
                    ConstantOperand::F32(v) => ScopedValue::from(v),
                    ConstantOperand::F64(v) => ScopedValue::from(v),
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
                value: value.and_then(|value| {
                    let operand = slot(value);
                    if operand.is_unit() {
                        None
                    } else {
                        Some(operand.scalar().expect("classified scalar return"))
                    }
                }),
                representation: value.map_or(ValueType::Unit, |register| {
                    location(register).representation
                }),
            },
            _ => Self::Boundary,
        }
    }

    fn move_slots(dst: Location, src: Location) -> Self {
        if dst.operand.is_unit() && src.operand.is_unit() {
            return Self::Unit;
        }
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
            LinkedFunction,
            layouts::{AggregateLayout, AppliedLayouts, FunctionLayouts},
        },
        value::{carriers::StoredValue, place::PublishedResult},
    };
    use std::{mem::size_of, sync::OnceLock};

    #[test]
    fn physical_instruction_budget() {
        assert!(size_of::<ExecutionInstruction>() <= 24);
        assert!(size_of::<ExecutionFrame>() <= 320);
        eprintln!(
            "layout metadata bytes: function={}, application={}, operand={}, application_cell={}",
            size_of::<FunctionLayouts>(),
            size_of::<AppliedLayouts>(),
            size_of::<AggregateLayout>(),
            size_of::<OnceLock<AggregateLayout>>()
        );
        eprintln!(
            "call publication bytes: operand={}, result={}",
            size_of::<Location>(),
            size_of::<PublishedResult<'static>>()
        );
        eprintln!(
            "region bytes: error={}, exit={}, result={}",
            size_of::<RegionError>(),
            size_of::<RegionExit>(),
            size_of::<Result<RegionExit, RegionError>>()
        );
        eprintln!(
            "execution metadata bytes: instruction={}, frame={}, linked_function={}, field_operation={}, index_operation={}, execution_function={}, constant_cell={}",
            size_of::<ExecutionInstruction>(),
            size_of::<ExecutionFrame>(),
            size_of::<LinkedFunction>(),
            size_of::<PreparedFieldOperation>(),
            size_of::<PreparedIndexOperation>(),
            size_of::<ExecutionFunction>(),
            size_of::<StoredValue>()
        );
    }
}
