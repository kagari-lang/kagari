use kagari_abi::{
    budget::LogicalBudgetCharge,
    ids::{DebugPointId, FunctionRef},
    representation::ValueType,
};
use kagari_bytecode::module::RootSlotLayout;
use kagari_bytecode::{
    instruction::{
        BytecodeInstruction, CallTarget, ConstantOperand, ModuleSlot, Register, RuntimeHelper,
    },
    module::{
        BytecodeFunction, BytecodeModule, BytecodeModuleSlot, FunctionMetadata, FunctionRecord,
        InstructionSourceSpan, SafeDebugPoint, SafeDebugPointKind,
    },
    program::ModuleRef,
};

use std::sync::{Arc, Mutex};

use kagari_common::span::Span;
use {
    kagari_common::host_interface::HostFunctionDeclaration,
    kagari_runtime::{
        Runtime, RuntimeConfig,
        error::RuntimeErrorKind,
        host::HostFunction,
        module::ModuleEpochRetention,
        resource::ResourcePolicy,
        value::{StructValueField, Value},
    },
};

use crate::{
    debug::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint},
    error::VmError,
    tests::common::{compile_test_bytecode, load_test_module},
    vm::Vm,
};

fn test_function(
    id: usize,
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeFunction {
    let metadata = FunctionMetadata {
        instruction_budgets: vec![LogicalBudgetCharge::Step; instructions.len()],
        return_type,
        roots: RootSlotLayout::from_types(&[], &registers),
        registers,
        ..FunctionMetadata::default()
    };
    BytecodeFunction {
        id: FunctionRef::new(id),
        identity: None,
        name: name.to_owned(),
        parameter_count: 0,
        register_count: metadata.registers.len() as u16,
        local_count: 0,
        metadata,
        instructions,
    }
}

fn verified_module(functions: Vec<BytecodeFunction>) -> BytecodeModule {
    let constants = functions
        .iter()
        .flat_map(|function| &function.instructions)
        .filter_map(|instruction| match instruction {
            BytecodeInstruction::LoadConst { constant, .. } => Some(constant.clone()),
            _ => None,
        })
        .fold(Vec::new(), |mut constants, constant| {
            if !constants.contains(&constant) {
                constants.push(constant);
            }
            constants
        });
    let mut types = vec![ValueType::Unit];
    for function in &functions {
        for ty in std::iter::once(function.metadata.return_type)
            .chain(function.metadata.params.iter().copied())
            .chain(function.metadata.locals.iter().copied())
            .chain(function.metadata.registers.iter().copied())
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

fn module_with_mutable_slot(value: i32) -> BytecodeModule {
    let mut module = verified_module(vec![
        test_function(
            0,
            "init",
            vec![
                BytecodeInstruction::LoadConst {
                    dst: Register::new(0),
                    constant: ConstantOperand::I32(value),
                },
                BytecodeInstruction::StoreModule {
                    slot: ModuleSlot::new(0),
                    src: Register::new(0),
                },
                BytecodeInstruction::Return(Some(Register::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
        ),
        test_function(
            1,
            "main",
            vec![
                BytecodeInstruction::LoadModule {
                    dst: Register::new(0),
                    slot: ModuleSlot::new(0),
                },
                BytecodeInstruction::Return(Some(Register::new(0))),
            ],
            ValueType::I32,
            vec![ValueType::I32],
        ),
    ]);
    module.module_slots = vec![BytecodeModuleSlot {
        name: "private".to_owned(),
        ty: ValueType::I32,
        mutable: true,
    }];
    module
}

fn reloadable_value_module(value: i32) -> BytecodeModule {
    verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(value),
            },
            BytecodeInstruction::Return(Some(Register::new(0))),
        ],
        ValueType::I32,
        vec![ValueType::I32],
    )])
}

fn host_call_runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn interface_instruction_module() -> BytecodeModule {
    use kagari_abi::{
        scalar::BuiltinType,
        types::{AbiType, InterfaceTableAbi, NominalAbiType, PublicAbiItem, TraitAbi},
    };
    use kagari_bytecode::{instruction::InterfaceTableRef, module::InterfaceTableRecord};
    use kagari_common::identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
    };

    let identity = ModuleIdentity::single_file("interface-instruction.kgr");
    let declaration = |kind, name: &str| DefinitionId {
        module: identity.clone(),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    };
    let trait_id = declaration(DefinitionKind::Trait, "Tag");
    let impl_id = declaration(DefinitionKind::Impl, "");
    let mut module = verified_module(vec![test_function(
        0,
        "main",
        vec![
            BytecodeInstruction::LoadConst {
                dst: Register::new(0),
                constant: ConstantOperand::I32(7),
            },
            BytecodeInstruction::MakeInterface {
                dst: Register::new(1),
                value: Register::new(0),
                module: ModuleRef::new(0),
                implementation: InterfaceTableRef::new(0),
            },
            BytecodeInstruction::Return(Some(Register::new(1))),
        ],
        ValueType::HeapObject,
        vec![ValueType::I32, ValueType::HeapObject],
    )]);
    module.identity = identity;
    module.public_items = vec![
        PublicAbiItem::Trait(TraitAbi {
            associated_consts: Vec::new(),
            supertraits: Vec::new(),
            associated_types: Vec::new(),
            name: "Tag".into(),
            generic_params: vec![],
            bounds: vec![],
            methods: vec![],
        }),
        PublicAbiItem::InterfaceTable(Box::new(InterfaceTableAbi {
            associated_type_families: Vec::new(),
            associated_consts: Vec::new(),
            host_bridge: false,
            declaration: impl_id.clone(),
            name: String::new(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: AbiType::Trait(NominalAbiType {
                associated_types: Default::default(),
                declaration: trait_id,
                arguments: vec![],
            }),
            for_type: AbiType::Builtin(BuiltinType::I32),
            methods: vec![],
        })),
    ];
    module.interface_tables = vec![InterfaceTableRecord {
        view: None,
        arguments: Vec::new(),
        declaration: impl_id,
        methods: vec![],
    }];
    module
}

mod debugger;
mod frames;
mod host_budgets;
mod interfaces;
