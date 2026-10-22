use crate::{
    debug::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint},
    error::VmError,
    tests::common::{compile_test_bytecode, load_test_module, standard_runtime},
    vm::Vm,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::instruction::ConstructionTarget as BytecodeConstruction;
use kagari_bytecode::instruction::Slot;
use kagari_bytecode::{
    instruction::{
        BytecodeInstruction, CallTarget, ConstantId, ConstantOperand, InterfaceTableRef,
        ModuleSlot, RuntimeHelper,
    },
    module::{
        BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, InstructionSourceSpan,
        InterfaceTableRecord, RootSlotLayout, SafeDebugPoint, SafeDebugPointKind,
    },
    program::ModuleRef,
};
use kagari_common::{
    identity::{
        DefinitionKind, ModuleIdentity, map::DefinitionContext, metadata::DefinitionMetadata,
    },
    span::Span,
};
use kagari_contract::statics::{StaticContract, StaticDescriptor};
use kagari_contract::{
    ids::{DebugPointId, FunctionRef},
    types::{InterfaceTable, PublicItem},
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    module::ModuleEpochRetention,
    resource::RuntimeLimits,
    value::{StructValueField, Value},
};
use kagari_types::visibility::Visibility;
use kagari_types::{
    declaration::TraitDef,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

fn test_function(
    id: usize,
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeFunction {
    let metadata = FunctionMetadata {
        roots: RootSlotLayout::from_types(&[registers.to_vec(), [].to_vec()].concat()),
        return_type,
        slots: registers,
        ..FunctionMetadata::default()
    };
    {
        let fixture_instructions = instructions;
        let mut fixture_metadata: kagari_bytecode::module::FunctionMetadata = metadata;

        if fixture_metadata.origins.is_empty() {
            fixture_metadata.origins = (0..fixture_instructions.len())
                .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                    span: Default::default(),
                    line: None,
                    column: None,
                    instruction,
                })
                .collect();
        }
        BytecodeFunction {
            id: FunctionRef::new(id),
            identity: None,
            name: name.to_owned(),
            parameter_count: 0,
            slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
            metadata: fixture_metadata,
            instructions: fixture_instructions,
        }
    }
}

fn verified_module(
    functions: Vec<BytecodeFunction>,
    constants: Vec<ConstantOperand>,
) -> BytecodeModule {
    let mut types = vec![ValueType::Unit];
    for function in &functions {
        for ty in std::iter::once(function.metadata.return_type)
            .chain(function.metadata.params.iter().copied())
            .chain(function.metadata.slots.iter().copied())
            .chain(function.metadata.slots.iter().copied())
        {
            if !types.contains(&ty) {
                types.push(ty);
            }
        }
    }
    let function_table = functions
        .iter()
        .map(|function| FunctionRecord {
            id: function.id,
            identity: function.identity.clone(),
            name: function.name.clone(),
            params: function.metadata.params.clone(),
            return_type: function.metadata.return_type,
            effects: function.metadata.effects,
        })
        .collect();
    BytecodeModule {
        constants,
        types,
        function_table,
        functions,
        ..BytecodeModule::default()
    }
}

fn module_with_mutable_slot(context: &DefinitionContext, value: i32) -> BytecodeModule {
    let mut module = verified_module(
        vec![
            test_function(
                0,
                "init",
                vec![
                    BytecodeInstruction::LoadConst {
                        dst: Slot::new(0),
                        constant: ConstantId::new(0),
                    },
                    BytecodeInstruction::Return(Some(Slot::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
            test_function(
                1,
                "main",
                vec![
                    BytecodeInstruction::LoadModule {
                        dst: Slot::new(0),
                        slot: ModuleSlot::new(0),
                    },
                    BytecodeInstruction::Return(Some(Slot::new(0))),
                ],
                ValueType::I32,
                vec![ValueType::I32],
            ),
        ],
        vec![ConstantOperand::I32(value)],
    );
    let root = context.intern_root(&module.identity).unwrap();
    module.module_slots = vec![StaticDescriptor {
        contract: StaticContract {
            declaration: context
                .intern_child(root, DefinitionKind::Static, "private", 0)
                .unwrap(),
            supplier: module.identity.clone(),
            name: "private".into(),
            ty: Ty::Builtin(BuiltinType::I32),
            mutable: true,
            visibility: Visibility::Private,
        },
        initializer: FunctionRef::new(0),
    }];
    for function in &mut module.functions {
        function.metadata.semantic.result = Some(Ty::Builtin(BuiltinType::I32));
        function
            .metadata
            .semantic
            .slots
            .insert(0, Ty::Builtin(BuiltinType::I32));
    }
    module
}

fn reloadable_value_module(value: i32) -> BytecodeModule {
    verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Return(Some(Slot::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
        )],
        vec![ConstantOperand::I32(value)],
    )
}

fn host_call_runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn debug_runtime(_module_name: &str) -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn interface_instruction_module() -> DefinitionMetadata<BytecodeModule> {
    let identity = ModuleIdentity::single_file("interface-instruction.kgr");
    let context = DefinitionContext::new().unwrap();
    let root = context.intern_root(&identity).unwrap();
    let declaration = |kind, name: &str| context.intern_child(root, kind, name, 0).unwrap();
    let trait_id = declaration(DefinitionKind::Trait, "Tag");
    let impl_id = declaration(DefinitionKind::Impl, "");
    let mut module = verified_module(
        vec![test_function(
            0,
            "main",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::MakeInterface {
                    dst: Slot::new(1),
                    value: Slot::new(0),
                    target: BytecodeConstruction::Declared {
                        module: ModuleRef::new(0),
                        implementation: InterfaceTableRef::new(0),
                        arguments: vec![],
                    },
                },
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ],
            ValueType::HeapObject,
            vec![ValueType::I32, ValueType::HeapObject],
        )],
        vec![ConstantOperand::I32(7)],
    );
    module.identity = identity;
    module.public_items = vec![
        PublicItem::Trait(TraitDef {
            conversion_adapter: None,
            associated_consts: Vec::new(),
            supertraits: Vec::new(),
            associated_types: Vec::new(),
            name: "Tag".into(),
            generic_params: vec![],
            bounds: vec![],
            methods: vec![],
        }),
        PublicItem::InterfaceTable(Box::new(InterfaceTable {
            associated_type_families: Vec::new(),
            associated_consts: Vec::new(),
            bridge: None,
            declaration: impl_id,
            name: String::new(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: Ty::Trait(NominalTy {
                associated_types: Default::default(),
                declaration: trait_id,
                arguments: vec![],
            }),
            for_type: Ty::Builtin(BuiltinType::I32),
            methods: vec![],
        })),
    ];
    module.interface_tables = vec![InterfaceTableRecord {
        parents: vec![],
        view: None,
        arguments: Vec::new(),
        declaration: impl_id,
        methods: vec![],
    }];
    DefinitionMetadata::checked(context.snapshot(), module, &Default::default()).unwrap()
}

mod debugger;
mod frames;
mod host_effects;
mod interfaces;
mod statics;
