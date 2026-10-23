//! Sealed canonical locations map into disjoint scalar and managed banks.
use crate::{
    module::execution::{OperandSlot, allocation::SlotAllocation},
    value::{Value, scalar},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::Slot, module::BytecodeFunction};
use kagari_contract::{native_import::NativeImport, representation::semantic_representation};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{collections::BTreeMap, iter};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Location {
    pub operand: OperandSlot,
    pub representation: ValueType,
    pub semantic: Option<BuiltinType>,
}

impl Location {
    pub(crate) fn admits_payload(self, representation: ValueType, bits: u64) -> bool {
        if self.operand.managed() || self.representation != representation {
            return false;
        }
        scalar::admits_payload(representation, self.semantic, bits)
    }

    pub(crate) fn admits(self, value: &Value) -> bool {
        if !value.has_representation(self.representation) {
            return false;
        }
        // Managed cell slots retain their content semantic type. Only direct
        // scalar storage uses that type to admit numeric payloads. Cell contents
        // keep the separately checked capture-cell read/write contract.
        if self.operand.managed() {
            return true;
        }
        scalar::encode(value)
            .is_some_and(|bits| scalar::admits_payload(self.representation, self.semantic, bits))
    }
}

/// Cold public-frame access constraints; prepared operations retain only Locations.
#[derive(Debug)]
pub(crate) struct ReusedSlot {
    pub logical: usize,
    pub range: Option<(usize, usize)>,
    pub starts_with_definition: bool,
}

#[derive(Debug)]
pub(crate) struct FrameLayout {
    pub locations: Box<[Location]>,
    pub count: usize,
    pub scalar_count: usize,
    pub managed_count: usize,
    pub reused_slots: Box<[ReusedSlot]>,
}

impl FrameLayout {
    pub(super) fn native(import: &NativeImport) -> Self {
        let mut scalar_count = 0;
        let mut managed_count = 0;
        let locations = iter::once(&import.signature.result)
            .chain(import.signature.params.iter())
            .map(|ty| {
                let representation = semantic_representation(ty);
                Location {
                    operand: allocate(
                        scalar_type(representation).is_none(),
                        &mut scalar_count,
                        &mut managed_count,
                    ),
                    representation,
                    semantic: builtin(Some(ty)),
                }
            })
            .collect();
        Self {
            locations,
            count: 0,
            scalar_count,
            managed_count,
            reused_slots: Box::default(),
        }
    }

    pub(super) fn prepare(function: &BytecodeFunction, work: &mut usize) -> Self {
        let slot_layout = SlotAllocation::prepare(function, work);
        let mut scalar_count = 0;
        let mut managed_count = 0;
        let mut physical = BTreeMap::new();
        let mut locations = Vec::new();
        for (logical, &representation) in function.metadata.slots.iter().enumerate() {
            let managed = scalar_type(representation).is_none();
            let color = slot_layout.index(logical).expect("verified register");
            debug_assert!(color < slot_layout.count);
            let operand = if managed {
                allocate(true, &mut scalar_count, &mut managed_count)
            } else {
                *physical
                    .entry(color)
                    .or_insert_with(|| allocate(false, &mut scalar_count, &mut managed_count))
            };
            locations.push(Location {
                operand,
                representation,
                semantic: builtin(function.metadata.semantic.slots.get(&logical)),
            });
        }
        let mut aliases = vec![0usize; scalar_count];
        for location in &locations {
            if !location.operand.managed() {
                aliases[location.operand.index()] += 1;
            }
        }
        let reused_slots = locations
            .iter()
            .enumerate()
            .filter(|(_, location)| {
                !location.operand.managed() && aliases[location.operand.index()] > 1
            })
            .map(|(logical, _)| {
                let range = slot_layout.ranges.get(logical).copied().flatten();
                // Instruction order is not execution order: a join can precede
                // its defining predecessors. Only a write without an input
                // requires completion before reading at the interval's start.
                let starts_with_definition = range.is_some_and(|(start, _)| {
                    let instruction = &function.instructions[start];
                    instruction
                        .slot_output()
                        .is_some_and(|slot| slot.index() == logical)
                        && !instruction
                            .slot_inputs()
                            .iter()
                            .any(|slot| slot.index() == logical)
                });
                ReusedSlot {
                    logical,
                    range,
                    starts_with_definition,
                }
            })
            .collect();
        Self {
            locations: locations.into(),
            reused_slots,
            count: scalar_count + managed_count,
            scalar_count,
            managed_count,
        }
    }

    pub(crate) fn location(&self, logical: usize) -> Option<Location> {
        self.locations.get(logical).copied()
    }

    /// Side entry may preserve entry-initialized parameters, never acquire a later
    /// logical value merely because a previous alias initialized the physical bank.
    pub(crate) fn admits_side_entry(&self, from: usize, to: usize, parameters: &[Slot]) -> bool {
        self.reused_slots.iter().all(|slot| {
            slot.range.is_none_or(|(start, end)| {
                to < start
                    || to > end
                    || (start <= from
                        && from <= end
                        && parameters
                            .iter()
                            .any(|parameter| parameter.index() == slot.logical))
            })
        })
    }

    pub(crate) fn admits_logical_access(
        &self,
        logical: usize,
        pc: usize,
        reading: bool,
        parameter: bool,
    ) -> bool {
        match self
            .reused_slots
            .binary_search_by_key(&logical, |slot| slot.logical)
        {
            Ok(index) => self.reused_slots[index].range.is_some_and(|(start, end)| {
                start <= pc
                    && pc <= end
                    && (!reading
                        || start < pc
                        || parameter
                        || !self.reused_slots[index].starts_with_definition)
            }),
            Err(_) => true,
        }
    }
}

fn allocate(managed: bool, scalars: &mut usize, values: &mut usize) -> OperandSlot {
    let count = if managed { values } else { scalars };
    let slot = OperandSlot::new(*count, managed);
    *count += 1;
    slot
}

fn builtin(ty: Option<&Ty>) -> Option<BuiltinType> {
    match ty {
        Some(Ty::Builtin(ty)) => Some(*ty),
        _ => None,
    }
}

pub(crate) fn scalar_type(ty: ValueType) -> Option<BuiltinType> {
    Some(match ty {
        ValueType::Unit => BuiltinType::Unit,
        ValueType::Bool => BuiltinType::Bool,
        ValueType::Char => BuiltinType::Char,
        ValueType::I32 => BuiltinType::I32,
        ValueType::I64 => BuiltinType::I64,
        ValueType::U64 => BuiltinType::U64,
        ValueType::F32 => BuiltinType::F32,
        ValueType::F64 => BuiltinType::F64,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_bytecode::{
        instruction::{BytecodeInstruction as I, ConstantId, JumpTarget},
        module::FunctionMetadata,
    };

    #[test]
    fn char_locations_use_scalar_storage_and_reject_integer_domains() {
        assert_eq!(scalar_type(ValueType::Char), Some(BuiltinType::Char));
        let location = Location {
            operand: OperandSlot::new(0, false),
            representation: ValueType::Char,
            semantic: Some(BuiltinType::Char),
        };
        assert!(location.admits(&Value::Char('🙂')));
        assert!(!location.admits(&Value::I64(0x1f642)));
        assert!(location.admits_payload(ValueType::Char, 0x1f642));
        assert!(!location.admits_payload(ValueType::I64, 0x1f642));
        for bits in [0xd800, 0xdfff, 0x110000, 0x1_0000_0061] {
            assert!(!location.admits_payload(ValueType::Char, bits));
        }
        let incompatible = Location {
            semantic: Some(BuiltinType::U32),
            ..location
        };
        assert!(!incompatible.admits(&Value::Char('a')));
        assert!(!incompatible.admits_payload(ValueType::Char, 97));
    }

    #[test]
    fn backward_join_inputs_are_live_at_the_interval_start() {
        let function = BytecodeFunction {
            slot_count: 2,
            metadata: FunctionMetadata {
                slots: vec![ValueType::I32; 2],
                ..Default::default()
            },
            instructions: vec![
                I::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                I::Jump {
                    target: JumpTarget::new(3),
                },
                I::Return(Some(Slot::new(1))),
                I::LoadConst {
                    dst: Slot::new(1),
                    constant: ConstantId::new(0),
                },
                I::Jump {
                    target: JumpTarget::new(2),
                },
            ],
            ..Default::default()
        };
        let layout = FrameLayout::prepare(&function, &mut 0);
        assert_eq!(layout.scalar_count, 1, "retain physical slot reuse");
        assert!(!layout.admits_logical_access(0, 0, true, false));
        assert!(layout.admits_logical_access(0, 0, false, false));
        assert!(layout.admits_logical_access(1, 2, true, false));
        assert!(!layout.admits_logical_access(1, 1, true, false));
        assert!(!layout.admits_logical_access(0, 2, true, false));
        assert!(!layout.admits_side_entry(1, 2, &[]));
    }
}
