//! Sealed canonical locations map into disjoint scalar and managed banks.
use crate::{
    module::execution::{OperandSlot, allocation::RegisterAllocation},
    value::Value,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::module::BytecodeFunction;
use kagari_common::identity::table::DefinitionId;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Location {
    pub operand: OperandSlot,
    pub representation: ValueType,
    pub semantic: Option<BuiltinType>,
}

impl Location {
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
        let Some((min, max)) = self.semantic.and_then(BuiltinType::integer_bounds) else {
            return true;
        };
        let value = match value {
            Value::I32(v) => i128::from(*v),
            Value::I64(v) => i128::from(*v),
            Value::U64(v) => i128::from(*v),
            _ => return false,
        };
        value >= min && value <= max
    }
}

#[derive(Debug)]
pub(crate) struct FrameLayout {
    pub locations: Box<[Location]>,
    pub register_count: usize,
    pub count: usize,
    pub scalar_count: usize,
    pub managed_count: usize,
}

impl FrameLayout {
    pub(super) fn prepare(function: &BytecodeFunction<DefinitionId>, work: &mut usize) -> Self {
        let registers = RegisterAllocation::prepare(function, work);
        let mut scalar_count = 0;
        let mut managed_count = 0;
        let mut physical = BTreeMap::new();
        let mut locations = Vec::new();
        for (logical, &representation) in function.metadata.registers.iter().enumerate() {
            let managed = scalar_type(representation).is_none();
            let color = registers.index(logical).expect("verified register");
            debug_assert!(color < registers.count);
            let operand = *physical
                .entry((color, managed))
                .or_insert_with(|| allocate(managed, &mut scalar_count, &mut managed_count));
            locations.push(Location {
                operand,
                representation,
                semantic: builtin(function.metadata.semantic.registers.get(&logical)),
            });
        }
        let count = scalar_count + managed_count;
        for (logical, &representation) in function.metadata.locals.iter().enumerate() {
            locations.push(Location {
                operand: allocate(
                    scalar_type(representation).is_none(),
                    &mut scalar_count,
                    &mut managed_count,
                ),
                representation,
                semantic: builtin(
                    function
                        .metadata
                        .semantic
                        .locals
                        .get(&logical)
                        .or_else(|| function.metadata.semantic.params.get(&logical)),
                ),
            });
        }
        Self {
            locations: locations.into(),
            register_count: usize::from(function.register_count),
            count,
            scalar_count,
            managed_count,
        }
    }

    pub(crate) fn location(&self, logical: usize) -> Option<Location> {
        self.locations.get(logical).copied()
    }
}

fn allocate(managed: bool, scalars: &mut usize, values: &mut usize) -> OperandSlot {
    let count = if managed { values } else { scalars };
    let slot = OperandSlot::new(*count, managed);
    *count += 1;
    slot
}

fn builtin(ty: Option<&Ty<DefinitionId>>) -> Option<BuiltinType> {
    match ty {
        Some(Ty::Builtin(ty)) => Some(*ty),
        _ => None,
    }
}

pub(crate) fn scalar_type(ty: ValueType) -> Option<BuiltinType> {
    Some(match ty {
        ValueType::Unit => BuiltinType::Unit,
        ValueType::Bool => BuiltinType::Bool,
        ValueType::I32 => BuiltinType::I32,
        ValueType::I64 => BuiltinType::I64,
        ValueType::U64 => BuiltinType::U64,
        ValueType::F32 => BuiltinType::F32,
        ValueType::F64 => BuiltinType::F64,
        _ => return None,
    })
}
