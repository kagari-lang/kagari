use std::sync::{Arc, Mutex};

use kagari_abi::representation::ValueType;
use kagari_common::Span;
use kagari_ir::bytecode::{
    BytecodeFunction, BytecodeInstruction, BytecodeModule, BytecodeModuleSlot, CallTarget,
    ConstantOperand, DebugPointId, FunctionMetadata, FunctionRecord, FunctionRef,
    InstructionSourceSpan, ModuleSlot, Register, RuntimeHelper, SafeDebugPoint, SafeDebugPointKind,
};
use kagari_runtime::value::{StructValueField, Value};
use kagari_runtime::{
    CapabilitySet, DebugVisibilityPolicy, LanguageProfile, ModuleEpochRetention, ResourcePolicy,
    Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext,
};
use {kagari_runtime::HostFunctionDeclaration, kagari_runtime::host::HostFunction};

use crate::tests::common::{compile_test_bytecode, load_test_module};
use crate::{DebugPauseReason, DebugSession, DebugWatch, SourceBreakpoint, Vm, VmError};

fn test_function(
    id: usize,
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    registers: Vec<ValueType>,
) -> BytecodeFunction {
    let metadata = FunctionMetadata {
        return_type,
        roots: kagari_ir::bytecode::RootSlotLayout::from_types(&[], &registers),
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
        security: kagari_runtime::SecurityContext {
            profile: kagari_runtime::LanguageProfile {
                allow_host_calls: true,
                ..kagari_runtime::LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: kagari_runtime::HostExposurePolicy {
            allow_host_functions: true,
            ..kagari_runtime::HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn debug_runtime(module_name: &str) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_debugger: true,
                ..LanguageProfile::default()
            },
            capabilities: debug_capabilities(),
        },
        debug_visibility: DebugVisibilityPolicy {
            visible_modules: vec![module_name.to_owned()],
            ..DebugVisibilityPolicy::default()
        },
        ..RuntimeConfig::default()
    })
}

fn debug_capabilities() -> CapabilitySet {
    CapabilitySet {
        debug_attach: true,
        debug_breakpoints: true,
        debug_pause: true,
        debug_stack_inspection: true,
        debug_value_inspection: true,
        debug_watch_evaluation: true,
        ..CapabilitySet::default()
    }
}

fn interface_instruction_module() -> BytecodeModule {
    use kagari_abi::scalar::BuiltinType;
    use kagari_common::identity::{
        DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity,
    };
    use kagari_ir::bytecode::{InterfaceTableRecord, InterfaceTableRef};
    use kagari_ir::module::InterfaceTableAbi;
    use kagari_ir::module::PublicAbiItem;
    use kagari_ir::module::TraitAbi;
    use kagari_ir::module::abi::AbiType;
    use kagari_ir::module::abi::NominalAbiType;

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
                module: kagari_ir::bytecode::ModuleRef::new(0),
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
            default_methods: Vec::new(),
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
            native_bridge: false,
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
