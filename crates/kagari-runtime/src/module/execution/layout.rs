//! Sealed canonical locations map into disjoint scalar and managed banks.
use crate::{
    module::execution::{
        OperandSlot,
        allocation::{SlotAllocation, SlotColor},
    },
    value::{Value, scalar},
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{instruction::Slot, module::BytecodeFunction};
use kagari_contract::{native_import::NativeImport, representation::semantic_representation};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::iter;

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
        let mut scalar_count = slot_layout.scalar_count;
        let mut managed_count = 0;
        let mut locations = Vec::new();
        for (logical, &representation) in function.metadata.slots.iter().enumerate() {
            let operand = match slot_layout.slots[logical] {
                SlotColor::Managed => allocate(true, &mut scalar_count, &mut managed_count),
                SlotColor::Scalar(color) => {
                    debug_assert!(color < scalar_count);
                    OperandSlot::new(color, false)
                }
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
    use crate::test_support::native::program::ProgramFixture;
    use crate::{
        Runtime,
        frame::{
            arguments::FrameArguments,
            values::{ExecutionValues, FrameMetadata},
        },
        module::ExecutableModule,
    };
    use kagari_bytecode::{
        instruction::{BytecodeInstruction as I, ConstantId, JumpTarget},
        module::{BytecodeModule, FunctionMetadata, LocalLiveRange},
        program::ModuleRef,
    };

    use std::sync::Arc;

    fn storage_control(types: Vec<ValueType>, debug: bool) -> BytecodeFunction {
        let mut function = BytecodeFunction {
            slot_count: 6,
            metadata: FunctionMetadata {
                slots: types,
                ..Default::default()
            },
            instructions: vec![
                I::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                I::LoadConst {
                    dst: Slot::new(1),
                    constant: ConstantId::new(0),
                },
                I::LoadConst {
                    dst: Slot::new(2),
                    constant: ConstantId::new(0),
                },
                I::Move {
                    dst: Slot::new(1),
                    src: Slot::new(1),
                },
                I::LoadConst {
                    dst: Slot::new(3),
                    constant: ConstantId::new(0),
                },
                I::LoadConst {
                    dst: Slot::new(4),
                    constant: ConstantId::new(0),
                },
                I::Move {
                    dst: Slot::new(3),
                    src: Slot::new(3),
                },
                I::LoadConst {
                    dst: Slot::new(5),
                    constant: ConstantId::new(0),
                },
                I::Return(None),
            ],
            ..Default::default()
        };
        if debug {
            function
                .metadata
                .debug
                .local_live_ranges
                .push(LocalLiveRange {
                    slot: Slot::new(0),
                    binding: 0,
                    name: String::new(),
                    span: Default::default(),
                    start: 1,
                    end: 6,
                    ty: function.metadata.slots[0],
                    is_parameter: false,
                });
        }
        function
    }

    fn storage_module(runtime: &mut Runtime) -> ExecutableModule {
        let program = runtime
            .load_program(
                "rl01",
                ProgramFixture {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                    exports: Default::default(),
                }
                .metadata(runtime.host().definitions(), &Default::default())
                .unwrap(),
            )
            .unwrap();
        runtime.select_program(&program).unwrap()
    }

    #[test]
    fn storage_classes_determine_actual_bank_capacity_without_losing_ranges() {
        let mut runtime = Runtime::default();
        let module = storage_module(&mut runtime);
        let mixed = vec![
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
            ValueType::I32,
            ValueType::HeapObject,
        ];
        for (types, debug, initial_work, expected) in [
            (mixed.clone(), false, 0, (1, 3)),
            (vec![ValueType::I32; 6], false, 0, (2, 0)),
            (vec![ValueType::HeapObject; 6], false, 0, (0, 6)),
            (mixed.clone(), true, 0, (2, 3)),
            (mixed, false, usize::MAX, (3, 3)),
        ] {
            let function = storage_control(types, debug);
            let mut allocation_work = initial_work;
            let allocation = SlotAllocation::prepare(&function, &mut allocation_work);
            let mut layout_work = initial_work;
            let layout = FrameLayout::prepare(&function, &mut layout_work);
            assert_eq!((layout.scalar_count, layout.managed_count), expected);
            assert_eq!(layout.count, expected.0 + expected.1);
            assert_eq!(allocation.scalar_count, expected.0);
            let managed: Vec<_> = layout
                .locations
                .iter()
                .filter(|location| location.operand.managed())
                .map(|location| location.operand.index())
                .collect();
            assert_eq!(managed, (0..expected.1).collect::<Vec<_>>());
            for (logical, location) in layout.locations.iter().enumerate() {
                assert_eq!(
                    allocation.slots[logical] == SlotColor::Managed,
                    location.operand.managed()
                );
            }
            if initial_work == usize::MAX {
                assert!(allocation.ranges.is_empty());
                assert!(layout.reused_slots.is_empty());
            } else {
                assert_eq!(
                    allocation.ranges.as_ref(),
                    &[
                        Some((0, if debug { 5 } else { 0 })),
                        Some((1, 3)),
                        Some((2, 2)),
                        Some((4, 6)),
                        Some((5, 5)),
                        Some((7, 7)),
                    ]
                );
            }
            if expected == (1, 3) {
                for logical in [0, 2, 4] {
                    assert_eq!(layout.locations[logical].operand.index(), 0);
                }
            } else if debug {
                assert_ne!(
                    layout.locations[0].operand.index(),
                    layout.locations[4].operand.index(),
                    "debug use and definition at PC 5 have closed overlapping endpoints"
                );
            }
            let mut values = ExecutionValues::default();
            let frame = values
                .allocate(
                    6,
                    &[],
                    &FrameArguments::plain(&[]),
                    FrameMetadata {
                        program: module.clone(),
                        environment: None,
                        invocation: None,
                    },
                    Some(Arc::new(layout)),
                )
                .unwrap();
            assert_eq!(
                (
                    values.payloads.len(),
                    values.initialized.len(),
                    values.values.len()
                ),
                (expected.0, expected.0, expected.1)
            );
            values.release(frame).unwrap();
        }
    }

    #[test]
    fn unused_unit_char_and_managed_capture_slots_keep_their_physical_class() {
        let runtime = Runtime::default();
        let cell = Value::Cell(
            runtime
                .gc()
                .alloc_cell(ValueType::Char, Value::Char('🙂'))
                .unwrap(),
        );
        for types in [
            vec![],
            vec![ValueType::HeapObject; 2],
            vec![
                ValueType::Unit,
                ValueType::Char,
                ValueType::HeapObject,
                ValueType::HeapObject,
            ],
        ] {
            let mut function = BytecodeFunction {
                slot_count: types.len() as u32,
                metadata: FunctionMetadata {
                    slots: types,
                    ..Default::default()
                },
                instructions: vec![I::Return(None)],
                ..Default::default()
            };
            for (slot, &representation) in function.metadata.slots.iter().enumerate() {
                if representation == ValueType::HeapObject {
                    // Capture cells keep a scalar content type, not scalar storage.
                    function
                        .metadata
                        .semantic
                        .slots
                        .insert(slot, Ty::Builtin(BuiltinType::Char));
                }
            }
            for initial_work in [0, usize::MAX] {
                let mut work = initial_work;
                let layout = FrameLayout::prepare(&function, &mut work);
                let has_scalars = function.slot_count == 4;
                assert_eq!(
                    layout.scalar_count,
                    if has_scalars {
                        if initial_work == 0 { 1 } else { 2 }
                    } else {
                        0
                    }
                );
                assert_eq!(
                    layout.managed_count,
                    if function.slot_count == 0 { 0 } else { 2 }
                );
                for (logical, location) in layout.locations.iter().enumerate() {
                    if function.metadata.slots[logical] == ValueType::HeapObject {
                        assert!(location.operand.managed());
                        assert_eq!(location.semantic, Some(BuiltinType::Char));
                        assert!(location.admits(&cell));
                        assert!(!location.admits(&Value::Char('🙂')));
                    } else if initial_work == 0 {
                        assert!(!layout.admits_logical_access(logical, 0, true, false));
                    }
                }
            }
            if function.slot_count == 4 {
                function.metadata.parameter_slots = vec![Slot::new(0), Slot::new(1)];
                let allocation = SlotAllocation::prepare(&function, &mut 0);
                let layout = FrameLayout::prepare(&function, &mut 0);
                assert_eq!(
                    layout.scalar_count, 2,
                    "unused entry parameters still overlap at PC 0"
                );
                assert_eq!(
                    allocation.ranges.as_ref(),
                    &[Some((0, 0)), Some((0, 0)), None, None]
                );
                assert!(layout.locations[0].admits(&Value::Unit));
                assert!(layout.locations[1].admits(&Value::Char('🙂')));
            }
        }
    }

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
