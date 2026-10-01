//! Build portable defaults from checked native records; source authoring is separate.
// The shared fixture owns actual registered handlers and selected callbacks.
#[path = "fixtures/native_selected_api.rs"]
mod fixture_api;

use kagari_abi::{
    callable::{CallableImplementation, MethodPolicy, NativeDefaultApplication},
    scalar::BuiltinType,
    types::{
        AbiType, FunctionAbi, InterfaceTableAbi, NominalAbiType, ParameterAbi, PublicAbiItem,
        TraitAbi,
    },
};
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{BytecodeInstruction, CallTarget, InterfaceTableRef, NativeImportId},
    module::{CallableTarget, InterfaceMethodSlot, InterfaceTableRecord},
    program::BytecodeProgram,
};
use kagari_common::identity::{DefinitionId, DefinitionKind, DefinitionPathSegment};
use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use std::collections::BTreeMap;

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_selected.kbc");

fn program() -> BytecodeProgram {
    let mut program = KbcArtifact::from_bytes(ARTIFACT).unwrap().program;
    let root_ref = program.root;
    let root = &mut program.modules[root_ref.index()];
    let target = root
        .native_imports
        .iter()
        .position(|import| {
            import
                .instance
                .declaration
                .path
                .last()
                .is_some_and(|part| part.name == "external_scalar")
                && import.instance.arguments == [AbiType::Builtin(BuiltinType::Bool)]
        })
        .unwrap();
    let template = root.native_imports[target].instance.declaration.clone();
    let parent = root.native_imports[target].callables[0]
        .requirement
        .interface
        .clone();
    let mut owner = DefinitionId {
        module: root.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "DefaultWrapper".into(),
            occurrence: 0,
        }],
    };
    let interface = NominalAbiType {
        declaration: owner.clone(),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let declared = FunctionAbi {
        name: "echo".into(),
        implementation: CallableImplementation::NativeDefault(NativeDefaultApplication {
            declaration: template,
            arguments: vec![AbiType::SelfType(owner.clone())],
        }),
        method_policy: MethodPolicy {
            override_allowed: false,
        },
        generic_params: vec![],
        bounds: vec![],
        params: vec![ParameterAbi {
            name: "self".into(),
            ty: AbiType::SelfType(owner.clone()),
            mutable: false,
        }],
        return_type: AbiType::Builtin(BuiltinType::I32),
    };
    let trait_ = TraitAbi {
        name: "DefaultWrapper".into(),
        generic_params: vec![],
        bounds: vec![],
        supertraits: vec![parent],
        associated_types: vec![],
        associated_consts: vec![],
        methods: vec![declared.clone()],
    };
    owner.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "echo".into(),
        occurrence: 0,
    });
    let method = owner;
    let implementation = DefinitionId {
        module: root.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Impl,
            name: String::new(),
            occurrence: 1,
        }],
    };
    let mut applied = declared;
    applied.params[0].ty = AbiType::Builtin(BuiltinType::Bool);
    let CallableImplementation::NativeDefault(application) = &mut applied.implementation else {
        unreachable!()
    };
    application.arguments = vec![AbiType::Builtin(BuiltinType::Bool)];
    root.public_items.push(PublicAbiItem::Trait(trait_));
    root.public_items
        .push(PublicAbiItem::InterfaceTable(Box::new(InterfaceTableAbi {
            declaration: implementation.clone(),
            name: "default_wrapper".into(),
            generic_params: vec![],
            bounds: vec![],
            trait_type: AbiType::Trait(interface.clone()),
            for_type: AbiType::Builtin(BuiltinType::Bool),
            methods: vec![applied],
            associated_consts: vec![],
            associated_type_families: vec![],
            host_bridge: false,
            native_bridge: false,
        })));
    let table = InterfaceTableRef::new(root.interface_tables.len());
    root.interface_tables.push(InterfaceTableRecord {
        declaration: implementation,
        arguments: vec![],
        methods: vec![InterfaceMethodSlot {
            method,
            target: CallableTarget::Native(NativeImportId::new(target)),
        }],
    });
    let function = root
        .functions
        .iter_mut()
        .find(|function| function.name == "external_dynamic_main")
        .unwrap();
    let mut conversions = 0;
    let mut calls = 0;
    for instruction in &mut function.instructions {
        match instruction {
            BytecodeInstruction::MakeInterface {
                module,
                implementation,
                ..
            } => {
                *module = root_ref;
                *implementation = table;
                conversions += 1;
            }
            BytecodeInstruction::Call {
                callee:
                    CallTarget::InterfaceMethod {
                        module,
                        interface: target,
                        ..
                    },
                ..
            } => {
                *module = root_ref;
                *target = interface.clone();
                calls += 1;
            }
            _ => {}
        }
    }
    assert_eq!((conversions, calls), (1, 1));
    for ty in function
        .metadata
        .semantic
        .registers
        .values_mut()
        .chain(function.metadata.semantic.locals.values_mut())
    {
        if matches!(ty, AbiType::Trait(_)) {
            *ty = AbiType::Trait(interface.clone());
        }
    }
    program
}

fn engine() -> KagariEngine {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    KagariEngine::builder()
        .config(config)
        .install_standard_library(false)
        .install(fixture_api::api())
        .build()
        .unwrap()
}

#[test]
fn portable_native_defaults_execute_dynamic_calls_on_the_existing_driver() {
    let artifact = KbcArtifact::from_program(program(), Default::default()).unwrap();
    let prepared = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "external_dynamic_main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn portable_default_slots_reject_script_substitutes_and_changed_arguments() {
    for case in 0..3 {
        let mut program = program();
        let root = &mut program.modules[program.root.index()];
        let table = root.interface_tables.last_mut().unwrap();
        match case {
            0 => table.methods[0].target = CallableTarget::Script(root.functions[0].id),
            1 | 2 => {
                let abi = root
                    .public_items
                    .iter_mut()
                    .find_map(|item| match item {
                        PublicAbiItem::InterfaceTable(abi)
                            if abi.declaration == table.declaration =>
                        {
                            Some(abi)
                        }
                        _ => None,
                    })
                    .unwrap();
                let CallableImplementation::NativeDefault(application) =
                    &mut abi.methods[0].implementation
                else {
                    unreachable!()
                };
                if case == 1 {
                    application.arguments[0] = AbiType::Builtin(BuiltinType::I32);
                } else {
                    application.declaration.path.last_mut().unwrap().name = "external_echo".into();
                }
            }
            _ => unreachable!(),
        }
        assert!(
            KbcArtifact::from_program(program, Default::default()).is_err(),
            "{case}"
        );
    }
}

#[test]
fn final_default_members_can_share_one_checked_native_instance() {
    let mut program = program();
    let root = &mut program.modules[program.root.index()];
    for item in &mut root.public_items {
        match item {
            PublicAbiItem::Trait(contract) if contract.name == "DefaultWrapper" => {
                let mut alias = contract.methods[0].clone();
                alias.name = "again".into();
                contract.methods.push(alias);
            }
            PublicAbiItem::InterfaceTable(table) if table.name == "default_wrapper" => {
                let mut alias = table.methods[0].clone();
                alias.name = "again".into();
                table.methods.push(alias);
            }
            _ => {}
        }
    }
    let table = root.interface_tables.last_mut().unwrap();
    let mut alias = table.methods[0].clone();
    alias.method.path.last_mut().unwrap().name = "again".into();
    table.methods.push(alias);
    let function = root
        .functions
        .iter_mut()
        .find(|function| function.name == "external_dynamic_main")
        .unwrap();
    for instruction in &mut function.instructions {
        if let BytecodeInstruction::Call {
            callee: CallTarget::InterfaceMethod { method_slot, .. },
            ..
        } = instruction
        {
            *method_slot = 1;
        }
    }
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "external_dynamic_main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn portable_default_budget_cuts_release_interface_and_callback_state() {
    let artifact = KbcArtifact::from_program(program(), Default::default()).unwrap();
    let prepared =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine().runtime(context.clone());
    let loaded = runtime.load_program(&prepared, Default::default()).unwrap();
    let mut finished = false;
    for limit in 0..100 {
        let mut limited = context.clone();
        limited.resources.max_instruction_steps = Some(limit);
        match runtime.execute(&loaded, "external_dynamic_main", &[], &limited) {
            Ok(report) => {
                assert_eq!(report.return_value, Value::I32(42));
                finished = true;
            }
            Err(error) => assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED"),
        }
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        assert!(!runtime.runtime().is_quarantined());
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
        if finished {
            break;
        }
    }
    assert!(finished);
}
