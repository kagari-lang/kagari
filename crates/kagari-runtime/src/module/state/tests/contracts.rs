use super::*;
use crate::value::EnumTag;
use kagari_bytecode::{
    artifact::KbcArtifact,
    instruction::{CallTarget, EnumId},
    program::verified::VerifiedBytecodeProgram,
};
use kagari_contract::{
    effects::EffectSet,
    layout::{EnumLayout, EnumVariantLayout},
};
use kagari_types::ty::NominalTy;

fn imported_program(context: &DefinitionContext) -> BytecodeProgram {
    let mut supplier = BytecodeModule::default();
    supplier.identity.path.push("supplier".into());
    add_static(
        context,
        &mut supplier,
        "shared",
        scalar(),
        true,
        Visibility::Public,
    );
    let mut reader = supplier.functions[0].clone();
    reader.name = "read".into();
    reader.slot_count = 1;
    reader.metadata.slots = vec![ValueType::I32];
    reader.metadata.semantic.slots.insert(0, scalar());
    reader.metadata.effects = EffectSet::module_read();
    reader.instructions = vec![
        BytecodeInstruction::LoadModule {
            dst: Slot::new(0),
            slot: ModuleSlot::new(0),
        },
        BytecodeInstruction::Return(Some(Slot::new(0))),
    ];
    reader.metadata.origins = (0..2)
        .map(|instruction| InstructionOrigin {
            instruction,
            span: Default::default(),
            line: None,
            column: None,
        })
        .collect();
    let mut consumer = BytecodeModule {
        module_slots: supplier.module_slots.clone(),
        dependencies: vec![ModuleRef::new(0)],
        types: vec![ValueType::I32],
        function_table: vec![FunctionRecord {
            id: reader.id,
            identity: None,
            name: reader.name.clone(),
            params: vec![],
            return_type: ValueType::I32,
            effects: reader.metadata.effects,
        }],
        functions: vec![reader],
        ..Default::default()
    };
    consumer.identity.path.push("consumer".into());
    let mut alias = consumer.clone();
    alias.identity.path = vec!["alias".into()];
    consumer.dependencies.push(ModuleRef::new(2));
    BytecodeProgram {
        exports: Default::default(),
        root: ModuleRef::new(1),
        modules: vec![supplier, consumer, alias],
    }
}

#[test]
fn source_free_imports_round_trip_and_bind_to_one_cell_without_initializing() {
    let context = DefinitionContext::new().unwrap();
    let code = imported_program(&context);
    let verified = VerifiedBytecodeProgram::new(code, &context, &Default::default()).unwrap();
    let artifact =
        KbcArtifact::from_verified(verified, Default::default(), &Default::default()).unwrap();
    let bytes = artifact.records().to_bytes(artifact.definitions()).unwrap();
    let decoded = KbcArtifact::from_bytes(&bytes).unwrap();
    let other = DefinitionContext::new().unwrap();
    let adopted = decoded.import_into(&other, &Default::default()).unwrap();
    let verified = adopted
        .into_records()
        .into_verified(&Default::default(), &other, &Default::default())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime
        .load_verified_program(
            "imports",
            VerifiedProgram::from_bytecode(verified.into_bytecode()),
        )
        .unwrap();
    let owner = loaded.member(ModuleRef::new(0)).unwrap();
    let alias = loaded.member(ModuleRef::new(2)).unwrap();
    assert_eq!(
        cell(&runtime.modules, &owner, 0),
        cell(&runtime.modules, &loaded, 0)
    );
    assert_eq!(
        cell(&runtime.modules, &alias, 0),
        cell(&runtime.modules, &loaded, 0)
    );
    assert!(matches!(
        runtime
            .module_instance_snapshot(&loaded)
            .unwrap()
            .module_slots[0],
        StaticState::Uninitialized(_)
    ));
    assert!(
        runtime
            .begin_execution(&loaded, runtime.execution_options())
            .is_err()
    );
    ready(&runtime, &owner, 0, Value::I32(7));
    assert_eq!(
        runtime
            .read_module_slot(&alias, ModuleSlot::new(0))
            .unwrap(),
        Value::I32(7)
    );
    runtime
        .write_module_slot(&loaded, ModuleSlot::new(0), Value::I32(42))
        .unwrap();
    assert_eq!(
        runtime
            .read_module_slot(&owner, ModuleSlot::new(0))
            .unwrap(),
        Value::I32(42)
    );
}

#[test]
fn source_free_verification_rejects_forged_static_contracts_and_initializer_authority() {
    for mutation in 0..9 {
        let context = DefinitionContext::new().unwrap();
        let mut code = imported_program(&context);
        VerifiedBytecodeProgram::new(code.clone(), &context, &Default::default()).unwrap();
        match mutation {
            0 => code.modules[1].dependencies.clear(),
            1 => code.modules[1].module_slots[0].contract.ty = Ty::Builtin(BuiltinType::I8),
            2 => code.modules[1].module_slots[0].initializer = FunctionRef::new(99),
            3 => code.modules[0].functions[0].metadata.semantic.result = None,
            4 => code.modules[1].functions[0]
                .metadata
                .semantic
                .slots
                .insert(0, Ty::Builtin(BuiltinType::I8))
                .map(|_| ())
                .unwrap_or(()),
            5 => {
                code.modules[0].module_slots[0].contract.mutable = false;
                let contract = code.modules[0].module_slots[0].contract.clone();
                code.modules[0].public_items = vec![PublicItem::Static(contract)];
                code.modules[1].module_slots = code.modules[0].module_slots.clone();
                code.modules[2].module_slots = code.modules[0].module_slots.clone();
                let function = &mut code.modules[1].functions[0];
                function.instructions.insert(
                    1,
                    BytecodeInstruction::StoreModule {
                        slot: ModuleSlot::new(0),
                        src: Slot::new(0),
                    },
                );
                function.metadata.origins.push(InstructionOrigin {
                    instruction: 2,
                    span: Default::default(),
                    line: None,
                    column: None,
                });
                // With mutability restored this is valid: rejection must not be
                // caused by a missing terminator, origin or effect declaration.
                let mut writable = code.clone();
                for module in &mut writable.modules {
                    module.module_slots[0].contract.mutable = true;
                }
                writable.modules[0].public_items = vec![PublicItem::Static(
                    writable.modules[0].module_slots[0].contract.clone(),
                )];
                VerifiedBytecodeProgram::new(writable, &context, &Default::default()).unwrap();
            }
            6 => {
                code.modules[1].functions[0].instructions[0] = BytecodeInstruction::Call {
                    dst: Some(Slot::new(0)),
                    callee: CallTarget::ModuleFunction {
                        module: ModuleRef::new(0),
                        function: FunctionRef::new(0),
                    },
                    args: vec![],
                }
            }
            7 => code.modules[0].functions[0].metadata.effects.may_suspend = true,
            _ => code.modules[0].module_slots[0].contract.visibility = Visibility::Private,
        }
        assert!(
            VerifiedBytecodeProgram::new(code, &context, &Default::default()).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn persistent_cell_callback_cycles_follow_joint_graph_edges_and_retirement() {
    let mut runtime = Runtime::default();
    let callback = Ty::Function {
        params: vec![],
        result: Box::new(Ty::Builtin(BuiltinType::Unit)),
    };
    let code = program(callback, true);
    let old = runtime.load_program("callbacks", code.clone()).unwrap();
    let closure = runtime
        .make_closure(&old, FunctionRef::new(0), vec![], None)
        .unwrap();
    ready(&runtime, &old, 0, closure);
    let candidate = runtime
        .stage_reload_program(&old, "callbacks", code.clone())
        .unwrap();
    let current = runtime.publish_staged_reload(candidate).unwrap();
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    let replacement = runtime
        .make_closure(&current, FunctionRef::new(0), vec![], None)
        .unwrap();
    runtime
        .write_module_slot(&current, ModuleSlot::new(0), replacement)
        .unwrap();
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&old.key())
    );
    assert!(!runtime.gc().validate_value(&closure));
    let root = runtime.root_value(replacement).unwrap();
    // An ordinary new installation retires the old domain, unlike compatible reload.
    let fresh = runtime.load_program("callbacks", code).unwrap();
    assert!(
        !runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&current.key())
    );
    drop(root);
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&current.key())
    );
    assert!(!runtime.gc().validate_value(&replacement));
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 1);
    assert!(matches!(
        runtime
            .module_instance_snapshot(&fresh)
            .unwrap()
            .module_slots[0],
        StaticState::Uninitialized(_)
    ));
}

#[test]
fn empty_variant_does_not_hide_changed_private_payload_schema() {
    let mut runtime = Runtime::default();
    let context = DefinitionContext::new().unwrap();
    let mut module = BytecodeModule::default();
    let root = context.intern_root(&module.identity).unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Enum, "Choice", 0)
        .unwrap();
    let empty = context
        .intern_child(declaration, DefinitionKind::Variant, "Empty", 0)
        .unwrap();
    let data = context
        .intern_child(declaration, DefinitionKind::Variant, "Data", 0)
        .unwrap();
    module.enumerations.push(EnumLayout {
        declaration,
        arguments: vec![],
        variants: vec![
            EnumVariantLayout {
                declaration: empty,
                payload: vec![],
                reports_failure: false,
            },
            EnumVariantLayout {
                declaration: data,
                payload: vec![array_type()],
                reports_failure: false,
            },
        ],
    });
    add_static(
        &context,
        &mut module,
        "choice",
        Ty::Enum(NominalTy {
            declaration,
            arguments: vec![],
            associated_types: Default::default(),
        }),
        true,
        Visibility::Private,
    );
    let mut code = BytecodeProgram {
        root: ModuleRef::new(0),
        modules: vec![module],
        exports: Default::default(),
    };
    let old = runtime
        .load_program(
            "choice",
            DefinitionMetadata::checked(context.snapshot(), code.clone(), &Default::default())
                .unwrap(),
        )
        .unwrap();
    let empty_value = Value::Enum(
        runtime
            .alloc_enum(
                EnumTag::Declared(old.enum_variant(EnumId::new(0), 0).unwrap()),
                vec![],
            )
            .unwrap(),
    );
    ready(&runtime, &old, 0, empty_value);
    let compatible = runtime
        .stage_reload_program(
            &old,
            "choice",
            DefinitionMetadata::checked(context.snapshot(), code.clone(), &Default::default())
                .unwrap(),
        )
        .unwrap();
    let current = runtime.publish_staged_reload(compatible).unwrap();
    code.modules[0].enumerations[0].variants[1].payload =
        vec![Ty::Array(Box::new(Ty::Builtin(BuiltinType::Bool)))];
    VerifiedProgram::new(
        DefinitionMetadata::checked(context.snapshot(), code.clone(), &Default::default()).unwrap(),
        &context,
    )
    .unwrap();
    assert!(
        runtime
            .stage_reload_program(
                &current,
                "choice",
                DefinitionMetadata::checked(context.snapshot(), code, &Default::default()).unwrap()
            )
            .is_err()
    );
    assert_eq!(
        runtime
            .read_module_slot(&current, ModuleSlot::new(0))
            .unwrap(),
        empty_value
    );
    assert_eq!(
        runtime.modules.latest("choice").unwrap().key(),
        current.key()
    );
}

#[test]
fn allocation_failure_invalidates_candidate_records_without_touching_published_cells() {
    let mut runtime = Runtime::default();
    let code = program(scalar(), true);
    let old = runtime.load_program("exhaustion", code.clone()).unwrap();
    ready(&runtime, &old, 0, Value::I32(42));
    runtime.modules.inner.borrow_mut().next_cell = u64::MAX;
    assert!(
        runtime
            .stage_reload_program(&old, "exhaustion", code)
            .is_err()
    );
    let unavailable = runtime
        .modules
        .inner
        .borrow()
        .records
        .keys()
        .copied()
        .filter(|key| *key != old.key())
        .collect::<Vec<_>>();
    assert_eq!(unavailable.len(), 1);
    assert!(runtime.modules.loaded(unavailable[0]).is_none());
    assert_eq!(
        runtime.modules.latest("exhaustion").unwrap().key(),
        old.key()
    );
    assert_eq!(
        runtime.read_module_slot(&old, ModuleSlot::new(0)).unwrap(),
        Value::I32(42)
    );
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .contains(&unavailable[0])
    );
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 1);
}

#[test]
fn cycles_between_retired_installation_cells_are_collectible_together() {
    let mut runtime = Runtime::default();
    let code = program(
        Ty::Function {
            params: vec![],
            result: Box::new(Ty::Builtin(BuiltinType::Unit)),
        },
        true,
    );
    let left = runtime.load_program("left", code.clone()).unwrap();
    let right = runtime.load_program("right", code.clone()).unwrap();
    let from_left = runtime
        .make_closure(&left, FunctionRef::new(0), vec![], None)
        .unwrap();
    let from_right = runtime
        .make_closure(&right, FunctionRef::new(0), vec![], None)
        .unwrap();
    ready(&runtime, &left, 0, from_right);
    ready(&runtime, &right, 0, from_left);
    runtime.load_program("left", code.clone()).unwrap();
    assert!(
        runtime
            .collect_garbage()
            .unwrap()
            .reclaimed_modules
            .is_empty()
    );
    runtime.load_program("right", code).unwrap();
    let reclaimed = runtime.collect_garbage().unwrap();
    assert!(reclaimed.reclaimed_modules.contains(&left.key()));
    assert!(reclaimed.reclaimed_modules.contains(&right.key()));
    assert_eq!(reclaimed.live_objects, 0);
    assert_eq!(runtime.modules.inner.borrow().cells.len(), 2);
}
