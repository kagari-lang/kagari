use crate::{
    tests::common::{
        compile_test_bytecode, load_bytecode_module, load_bytecode_module_with_runtime,
        load_bytecode_program_with_runtime, load_test_module, standard_runtime,
        test_function_module,
    },
    vm::Vm,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, ConstantOperand, PathId, Register,
        RuntimeHelper, StructId,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, FunctionRecord, PathRecord},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        HostCallContext, HostError, HostObjectId, HostPathAdapter, HostPathDescriptorRegistration,
        HostSchemaEpoch, PreparedHostPathWrite,
    },
    module::LoadedModule,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, module::NativeModule},
    value::Value,
};

use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostCallPolicy, HostFieldPolicy, HostFunctionEffects, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{Arc, Mutex};

// Share the cold declaration-to-program fixture with runtime boundary tests.
#[path = "../../../kagari-runtime/tests/support/native/program.rs"]
mod native_program;

fn host_runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn reflection_runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        ..RuntimeConfig::default()
    })
}

fn load_reflection_bytecode_module(
    name: &str,
    bytecode: DefinitionMetadata<BytecodeModule>,
) -> (Runtime, LoadedModule) {
    load_bytecode_module_with_runtime(reflection_runtime(), name, bytecode)
}

fn load_reflection_test_module(source_text: &str) -> (Runtime, LoadedModule) {
    load_bytecode_program_with_runtime(
        reflection_runtime(),
        "test.kgr",
        compile_test_bytecode(source_text),
    )
}

fn player_module() -> NativeModule {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    player
        .field(
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                FieldDef {
                    name: "hp".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    visibility: Visibility::Public,
                    mutable: true,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadWrite,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player = player.finish().unwrap();
    let entry = module
        .define_function(
            FunctionDecl::new("player")
                .returns(player.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    module
        .bind(entry, |cx: &HostCallContext<'_>, _: &[Value]| {
            let root = cx.runtime().host().root(HostObjectId(1)).unwrap();
            Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
        })
        .unwrap();
    module.finish().unwrap()
}

fn register_vm_host_path_runtime(access: PathAccess) -> (Runtime, Arc<Mutex<i32>>) {
    let mut runtime = host_runtime();
    let module = player_module();
    let declaration = module.to_declaration();
    let player = declaration
        .records()
        .definition(DefinitionKind::Struct, "Player", declaration.definitions())
        .unwrap();
    let field = declaration
        .definitions()
        .lookup_child(player, DefinitionKind::Field, "hp", 0)
        .unwrap();
    let field = declaration.definitions().resolve(field).unwrap().to_path();
    module.install(&mut runtime).unwrap();
    let player_id = runtime.types().get_by_name("game::Player").unwrap().id;
    let i32_id = runtime.types().get_by_name("i32").unwrap().id;
    runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let definitions = runtime.host().definitions();
    let field = definitions.lookup(&field).unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions,
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentDeclaration::Field(field)],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap();
    assert_eq!(descriptor_id.index(), 0);

    let hp = Arc::new(Mutex::new(10));
    let read_hp = Arc::clone(&hp);
    let write_hp = Arc::clone(&hp);
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(Value::I32(*read_hp.lock().unwrap())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    let write_hp = write_hp.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        *write_hp.lock().unwrap() = value;
                    }))
                }),
        )
        .unwrap();

    (runtime, hp)
}

fn path_module(
    name: &str,
    instructions: Vec<BytecodeInstruction>,
    return_type: ValueType,
    constants: Vec<ConstantOperand>,
) -> DefinitionMetadata<BytecodeModule> {
    let provider = player_module();
    let (definitions, mut program) =
        native_program::free_function_program(&provider.to_declaration()).into_parts();
    let module = program.modules.remove(0);
    let metadata = FunctionMetadata {
        return_type,
        registers: vec![
            ValueType::HostHandle,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::I32,
            ValueType::HostHandle,
        ],
        ..Default::default()
    };
    let root = module.external_types[0].declaration;
    let field = definitions
        .lookup_child(root, DefinitionKind::Field, "hp", 0)
        .unwrap();
    let path = HostPathDeclaration {
        root,
        segments: vec![HostPathSegmentDeclaration::Field(field)],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    let fingerprint = path
        .contract(&module.external_types, &definitions)
        .unwrap()
        .fingerprint(&definitions)
        .unwrap();
    let module = BytecodeModule {
        path_declarations: vec![path],
        module_slots: vec![],
        constants,
        types: vec![ValueType::Unit, ValueType::HostHandle, ValueType::I32],
        paths: vec![PathRecord {
            contract_fingerprint: fingerprint,
            id: PathId::new(0),
            root_ty: ValueType::HostHandle,
            result_ty: ValueType::I32,
            read_only: false,
            debug_name: "game.Player.hp".to_owned(),
        }],
        function_table: vec![FunctionRecord {
            id: FunctionRef::new(0),
            identity: None,
            name: name.to_owned(),
            params: metadata.params.clone(),
            return_type: metadata.return_type,
            effects: metadata.effects,
        }],
        functions: vec![BytecodeFunction {
            id: FunctionRef::new(0),
            identity: None,
            name: name.to_owned(),
            parameter_count: 0,
            register_count: metadata.registers.len() as u16,
            local_count: 0,
            metadata,
            instructions,
        }],
        ..module
    };
    DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
}

/// Select an explicit portable path requirement; physical records derive from its contract.
fn with_path_contract(
    module: DefinitionMetadata<BytecodeModule>,
    path: DefinitionMetadata<HostPathDeclaration>,
) -> DefinitionMetadata<BytecodeModule> {
    let context = DefinitionContext::new().unwrap();
    let mut module = module
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let path = path
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let definitions = context.snapshot();
    let contract = path.contract(&module.external_types, &definitions).unwrap();
    module.paths[0].contract_fingerprint = contract.fingerprint(&definitions).unwrap();
    module.paths[0].read_only = path.access == PathAccess::ReadOnly;
    module.path_declarations = vec![path];
    DefinitionMetadata::checked(definitions, module, &Default::default()).unwrap()
}

mod host_paths;
mod reflection;
mod standard;
