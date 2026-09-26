use kagari_common::{
    host_interface::{
        HostAssociatedTypeBinding, HostFunctionDeclaration, HostInterface, HostMethodDeclaration,
        HostParameter, HostPassingStyle, HostTraitImplementationDeclaration,
        HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership, HostValueType,
    },
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    source_database::SourceLayer,
};
use kagari_embed::{
    BytecodeArtifact, CompileOptions, ExecutionContext, HostExposurePolicy, KagariEngine,
};
use kagari_runtime::{
    CapabilitySet, LanguageProfile,
    host::{HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    value::Value,
};
use std::{cell::RefCell, rc::Rc};

const SOURCE: &str = concat!(
    include_str!("../../../examples/host-interfaces.kgr"),
    "\nfn answer() -> i32 { 42 }\npub fn fail() -> i32 { boxed().read(-1) }\n"
);

fn member(owner: &DefinitionId, kind: DefinitionKind, name: &str) -> DefinitionId {
    let mut id = owner.clone();
    id.path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    id
}

fn fixture() -> (
    KagariEngine,
    BytecodeArtifact,
    HostTypeDeclaration,
    HostFunctionDeclaration,
) {
    let engine = KagariEngine::default();
    let file = engine
        .set_source("mem://host-interface", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let trait_id = DefinitionId {
        module: engine
            .source_snapshot()
            .file(file)
            .unwrap()
            .module_identity()
            .clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Reader".into(),
            occurrence: 0,
        }],
    };
    let mut host = HostTypeDeclaration::new("demo.Counter");
    host.ownership = HostTypeOwnership::HostRoot;
    host.path_access = kagari_common::host_interface::PathAccess::ReadOnly;
    let method = HostMethodDeclaration::new(
        &host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    );
    let mut implementation = HostTraitImplementationDeclaration::new(
        trait_id.clone(),
        vec![],
        vec![HostTraitMethodBinding {
            trait_method: member(&trait_id, DefinitionKind::Method, "read"),
            host_method: method.id.clone(),
        }],
    );
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: member(&trait_id, DefinitionKind::AssociatedType, "Item"),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    host.methods.push(method);
    let make =
        HostFunctionDeclaration::new("demo.make", vec![], HostValueType::Opaque(host.id.clone()));
    let interface = HostInterface {
        types: vec![host.clone()],
        functions: vec![make.clone()],
        paths: vec![],
    };
    // Reading these declarations does not register any runtime callback.
    let interface = HostInterface::from_bytes(&interface.to_bytes().unwrap()).unwrap();
    engine.set_host_interface(interface).unwrap();
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: LanguageProfile {
                    allow_host_calls: true,
                    allow_jit: true,
                    ..Default::default()
                },
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    (engine, artifact, host, make)
}

fn context(jit: bool) -> ExecutionContext {
    ExecutionContext {
        language_profile: LanguageProfile {
            allow_host_calls: true,
            allow_jit: jit,
            ..Default::default()
        },
        capabilities: CapabilitySet {
            host_calls: true,
            jit,
            ..Default::default()
        },
        host_policy: HostExposurePolicy {
            allowed_host_functions: vec!["demo.make".into(), "demo.Counter.read".into()],
            ..Default::default()
        },
        jit_policy: if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        },
        ..Default::default()
    }
}

#[test]
fn host_associated_types_and_dynamic_interfaces_share_the_host_call_boundary() {
    let (engine, artifact, host, make) = fixture();
    assert_eq!(
        artifact.program.modules[artifact.program.root.index()]
            .interface_tables
            .len(),
        1
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = context(jit);
        let mut runtime = engine.runtime(context.clone());
        let ty = runtime
            .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |_, _| {
                calls.borrow_mut().push(0);
                Ok(Value::HostRoot(root))
            }))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(&host, &host.methods[0].id, move |ctx, args| {
                    let [Value::HostRoot(_), Value::I32(amount)] = args else {
                        panic!("invalid host arguments")
                    };
                    calls.borrow_mut().push(*amount);
                    assert!(
                        ctx.borrows()
                            .borrow_unique(root.object_id(), root.type_id())
                            .is_err()
                    );
                    Ok(Value::I32(*amount))
                })
                .unwrap(),
            )
            .unwrap();
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let result = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), [0, 20, 0, 22]);
    }
}

#[test]
fn host_child_interfaces_upcast_through_precompiled_parent_bridges() {
    let (engine, _, mut host, make) = fixture();
    let mut child = host.trait_implementations[0].trait_id.clone();
    child.path[0].name = "Child".into();
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            child,
            vec![],
            vec![],
        ));
    let interface = HostInterface {
        types: vec![host.clone()],
        functions: vec![make.clone()],
        paths: vec![],
    };
    engine.set_host_interface(interface).unwrap();
    let source = SOURCE.replace("pub fn boxed() -> Reader<Item = i32>", "trait Child: Reader<Item = i32> {}\npub fn boxed() -> Child")
        .replace("fn main() -> i32 { read(make()) + boxed().read(22) }", "fn main() -> i32 { val child = boxed(); val parent: Reader<Item = i32> = child; parent.read(20) + child.read(22) }");
    let file = engine
        .set_source("mem://host-interface", source, SourceLayer::Base)
        .unwrap();
    let context = context(false);
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: context.language_profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(make, move |_, _| {
            Ok(Value::HostRoot(root))
        }))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, |_, args| Ok(args[1].clone()))
                .unwrap(),
        )
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn invalid_host_associated_schemas_and_bridge_code_are_rejected() {
    use kagari_ir::{
        bytecode::{BytecodeInstruction, CallTarget, HostImportId},
        module::{PublicAbiItem, abi::AbiType},
    };
    let (_, artifact, _, _) = fixture();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[program.root.index()];
        match mutation {
            0 => module.host_interface.types[0].trait_implementations[0]
                .associated_types
                .clear(),
            1 => {
                module.host_interface.types[0].trait_implementations[0].associated_types[0].ty =
                    HostValueType::F32
            }
            2 => {
                let PublicAbiItem::InterfaceTable(table) = module
                    .public_items
                    .iter_mut()
                    .find(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                let AbiType::Trait(interface) = &mut table.trait_type else {
                    unreachable!()
                };
                *interface.associated_types.values_mut().next().unwrap() =
                    AbiType::Builtin(kagari_hir::types::BuiltinType::I64);
            }
            3 => {
                let PublicAbiItem::InterfaceTable(table) = module
                    .public_items
                    .iter_mut()
                    .find(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                table.host_bridge = false;
            }
            4 => {
                let index = module.interface_tables[0].methods[0].function.index();
                let function = &mut module.functions[index];
                let BytecodeInstruction::Call { args, .. } =
                    &function.instructions[function.metadata.params.len()]
                else {
                    unreachable!()
                };
                let arg = args[1];
                *function.instructions.last_mut().unwrap() = BytecodeInstruction::Return(Some(arg));
            }
            5 => {
                let host = &mut module.host_interface.types[0];
                let mut alternative = host.methods[0].clone();
                alternative.id.path.last_mut().unwrap().name = "other".into();
                alternative.name = "other".into();
                host.methods.push(alternative.clone());
                let contract = host.method_contract(&alternative.id).unwrap();
                let import = HostImportId::new(module.host_interface.functions.len());
                module.host_interface.functions.push(contract);
                let index = module.interface_tables[0].methods[0].function.index();
                let function = &mut module.functions[index];
                let BytecodeInstruction::Call { callee, .. } =
                    &mut function.instructions[function.metadata.params.len()]
                else {
                    unreachable!()
                };
                *callee = CallTarget::HostFunction(import);
            }
            _ => unreachable!(),
        }
        assert!(
            kagari_ir::bytecode::verify_program(&program).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(BytecodeArtifact::from_program(program, Default::default()).is_err());
    }
}

#[test]
fn rooted_host_interfaces_survive_gc_reentry_reload_and_trap_cleanup() {
    use kagari_runtime::{Runtime, RuntimeConfig, host::HostError};
    use kagari_vm::Vm;
    let (engine, artifact, host, make) = fixture();
    let context = context(false);
    let mut vm = Vm::new(Runtime::new(RuntimeConfig {
        security: context.security_context(),
        host_exposure: context.host_policy.clone(),
        ..Default::default()
    }));
    let ty = vm
        .runtime_mut()
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let root = vm
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(HostFunction::new(make, move |_, _| {
            Ok(Value::HostRoot(root))
        }))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, move |ctx, args| {
                let [Value::HostRoot(_), Value::I32(amount)] = args else {
                    panic!("host arguments")
                };
                assert!(
                    ctx.borrows()
                        .borrow_unique(root.object_id(), root.type_id())
                        .is_err()
                );
                if *amount == -1 {
                    return Err(HostError::new("expected host trap"));
                }
                if *amount == 21 {
                    let version = ctx.runtime().execution_root().unwrap();
                    let answer = version
                        .bytecode
                        .functions
                        .iter()
                        .find(|function| function.name == "answer")
                        .unwrap()
                        .id;
                    let value = kagari_vm::reenter(ctx, &version, answer, &[]).unwrap();
                    ctx.runtime().collect_garbage().unwrap();
                    return Ok(value.value());
                }
                Ok(Value::I32(*amount))
            })
            .unwrap(),
        )
        .unwrap();
    let loaded = vm
        .runtime_mut()
        .load_program("host-interfaces", artifact.program)
        .unwrap();
    let value = vm.execute(&loaded, "boxed").unwrap().return_value;
    let rooted = vm.runtime().root_value(value.clone()).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let method = host.trait_implementations[0].methods[0]
        .trait_method
        .clone();
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[Value::I32(21)])
            .unwrap(),
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "fail").is_err());
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    {
        let scope = vm.runtime().host_scope(&[Value::HostRoot(root)]).unwrap();
        assert!(
            scope
                .borrows()
                .borrow_unique(root.object_id(), root.type_id())
                .is_ok()
        );
    }
    let file = engine
        .set_source(
            "mem://host-interface",
            SOURCE.replace("fn answer() -> i32 { 42 }", "fn answer() -> i32 { 43 }"),
            SourceLayer::Base,
        )
        .unwrap();
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: context.language_profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let new = vm
        .reload_artifact(&loaded, "host-interfaces", artifact, &Default::default())
        .unwrap();
    let new_value = vm.execute(&new, "boxed").unwrap().return_value;
    let new_root = vm.runtime().root_value(new_value.clone()).unwrap();
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[Value::I32(21)])
            .unwrap(),
        Value::I32(42)
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &method, &[Value::I32(21)])
            .unwrap(),
        Value::I32(43)
    );
    let mut foreign = Runtime::default();
    let foreign_type = foreign
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let foreign_root = foreign
        .register_host_root(HostObjectId(7), foreign_type, HostSchemaEpoch::new(0))
        .unwrap();
    assert!(
        vm.runtime()
            .make_interface(&loaded, 0, Value::HostRoot(foreign_root))
            .is_err()
    );
    {
        let scope = vm.runtime().host_scope(&[Value::HostRoot(root)]).unwrap();
        let token = scope
            .borrows()
            .borrow_shared(root.object_id(), root.type_id())
            .unwrap();
        assert!(
            vm.runtime()
                .make_interface(&loaded, 0, Value::host_ref(token))
                .is_err()
        );
    }
    drop(rooted);
    drop(new_root);
    vm.runtime().collect_garbage().unwrap();
    assert!(
        vm.invoke_interface_method(&value, &method, &[Value::I32(21)])
            .is_err()
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
}

#[test]
fn host_associated_outputs_are_checked_against_trait_bounds_without_calls() {
    let (engine, _, mut host, make) = fixture();
    let reader_id = host.trait_implementations[0].trait_id.clone();
    let mut describe_id = reader_id.clone();
    describe_id.path[0].name = "Describe".into();
    let mut other = HostTypeDeclaration::new("demo.Other");
    let number = HostMethodDeclaration::new(&other.id, "number", vec![], HostValueType::I32);
    other.methods.push(number.clone());
    other
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            describe_id.clone(),
            vec![HostValueType::Opaque(host.id.clone())],
            vec![HostTraitMethodBinding {
                trait_method: member(&describe_id, DefinitionKind::Method, "number"),
                host_method: number.id,
            }],
        ));
    host.trait_implementations[0].methods.clear();
    host.trait_implementations[0].associated_types[0].ty = HostValueType::Opaque(other.id.clone());
    let file = engine.set_source("mem://host-interface", "trait Describe<T> { fn number(self) -> i32; } trait Reader { type Item: Describe<Self>; } use demo::Counter; fn accept(value: Counter) {} fn main() -> i32 { 42 }".into(), SourceLayer::Base).unwrap();
    engine
        .set_host_interface(HostInterface {
            types: vec![host, other],
            functions: vec![make],
            paths: vec![],
        })
        .unwrap();
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let mut program = artifact.program;
    let other = program.modules[program.root.index()]
        .host_interface
        .types
        .iter_mut()
        .find(|host| host.symbol == "demo.Other")
        .unwrap();
    other.trait_implementations.clear();
    assert!(kagari_ir::bytecode::verify_program(&program).is_err());
}

#[test]
fn imported_host_interfaces_preserve_generic_inputs_and_associated_outputs() {
    use kagari_common::identity::{ModuleIdentity, PackageId};
    let (engine, _, mut host, make) = fixture();
    let owner = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["api".into()],
    };
    engine
        .bind_module("mem://host-interface", owner.clone())
        .unwrap();
    let implementation = &mut host.trait_implementations[0];
    implementation.trait_id.module = owner.clone();
    implementation.trait_arguments = vec![HostValueType::I32];
    implementation.associated_types[0].declaration.module = owner.clone();
    implementation.methods[0].trait_method.module = owner;
    engine
        .set_host_interface(HostInterface {
            types: vec![host.clone()],
            functions: vec![make.clone()],
            paths: vec![],
        })
        .unwrap();
    engine.set_source("mem://host-interface", "pub trait Reader<T: HashKey> { type Item: HashKey; fn read(self, amount: T) -> Self::Item; }".into(), SourceLayer::Base).unwrap();
    engine
        .bind_module(
            "mem://consumer",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["consumer".into()],
            },
        )
        .unwrap();
    let root = engine.set_source("mem://consumer", "use pkg::api::Reader; use demo::make; fn main() -> i32 { val reader: Reader<i32, Item = i32> = make(); reader.read(42) }".into(), SourceLayer::Base).unwrap();
    let context = context(false);
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root,
            CompileOptions {
                language_profile: context.language_profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let value = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(make, move |_, _| {
            Ok(Value::HostRoot(value))
        }))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, |_, args| Ok(args[1].clone()))
                .unwrap(),
        )
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
}

#[test]
fn dynamic_host_calls_enforce_permissions_and_registered_output_contracts() {
    use kagari_runtime::{Runtime, RuntimeConfig};
    use kagari_vm::Vm;
    let (_, artifact, host, make) = fixture();
    let context = context(false);
    let mut vm = Vm::new(Runtime::new(RuntimeConfig {
        security: context.security_context(),
        host_exposure: context.host_policy.clone(),
        ..Default::default()
    }));
    let ty = vm
        .runtime_mut()
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let root = vm
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(HostFunction::new(make.clone(), move |_, _| {
            Ok(Value::HostRoot(root))
        }))
        .unwrap();
    let calls = Rc::new(RefCell::new(0));
    let recorded = calls.clone();
    vm.runtime_mut()
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, move |_, args| {
                *recorded.borrow_mut() += 1;
                Ok(args[1].clone())
            })
            .unwrap(),
        )
        .unwrap();
    let loaded = vm
        .runtime_mut()
        .load_program("permissions", artifact.program.clone())
        .unwrap();
    let value = vm.execute(&loaded, "boxed").unwrap().return_value;
    let rooted = vm.runtime().root_value(value).unwrap();
    let method = &host.trait_implementations[0].methods[0].trait_method;
    let mut denied = context.security_context();
    denied.capabilities.host_calls = false;
    vm.runtime_mut().set_security_context(denied);
    assert!(
        vm.invoke_interface_method(&rooted.value(), method, &[Value::I32(42)])
            .is_err()
    );
    vm.runtime_mut()
        .set_security_context(context.security_context());
    vm.runtime_mut()
        .set_host_exposure_policy(HostExposurePolicy {
            allowed_host_functions: vec!["demo.make".into()],
            ..Default::default()
        });
    assert!(
        vm.invoke_interface_method(&rooted.value(), method, &[Value::I32(42)])
            .is_err()
    );
    assert_eq!(*calls.borrow(), 0);
    vm.runtime_mut()
        .set_host_exposure_policy(context.host_policy.clone());
    assert_eq!(
        vm.invoke_interface_method(&rooted.value(), method, &[Value::I32(42)])
            .unwrap(),
        Value::I32(42)
    );
    assert_eq!(*calls.borrow(), 1);

    for mismatch in [false, true] {
        let mut registered = host.clone();
        if mismatch {
            registered.trait_implementations[0].associated_types[0].ty = HostValueType::I64;
        }
        let mut runtime = Runtime::new(RuntimeConfig {
            security: context.security_context(),
            host_exposure: context.host_policy.clone(),
            ..Default::default()
        });
        runtime
            .register_host_type(HostTypeRegistration::new(registered.clone(), "Counter"))
            .unwrap();
        runtime
            .register_host_function(HostFunction::new(make.clone(), |_, _| Ok(Value::Unit)))
            .unwrap();
        runtime
            .register_host_function(
                HostFunction::method(&registered, &registered.methods[0].id, |_, args| {
                    Ok(args[1].clone())
                })
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .load_program("registered-outputs", artifact.program.clone())
                .is_err(),
            mismatch
        );
    }
}

#[test]
fn interface_method_results_validate_nested_host_roots() {
    let (engine, _, mut host, make) = fixture();
    let source = SOURCE
        .replace("type Item: HashKey;", "type Item;")
        .replace("Item = i32", "Item = (Counter, i32)")
        .replace(
            "read(make()) + boxed().read(22)",
            "read(make())[1] + boxed().read(22)[1]",
        )
        .replace("boxed().read(-1)", "boxed().read(-1)[1]");
    let output = HostValueType::Tuple(vec![
        HostValueType::Opaque(host.id.clone()),
        HostValueType::I32,
    ]);
    host.methods[0].return_type = output.clone();
    host.trait_implementations[0].associated_types[0].ty = output;
    engine
        .set_host_interface(HostInterface {
            types: vec![host.clone()],
            functions: vec![make.clone()],
            paths: vec![],
        })
        .unwrap();
    let file = engine
        .set_source("mem://host-interface", source, SourceLayer::Base)
        .unwrap();
    let context = context(false);
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            file,
            CompileOptions {
                language_profile: context.language_profile,
            },
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(host.clone(), "Counter"))
        .unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(make, move |_, _| {
            Ok(Value::HostRoot(root))
        }))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(&host, &host.methods[0].id, move |_, args| {
                Ok(Value::Tuple(vec![Value::HostRoot(root), args[1].clone()]))
            })
            .unwrap(),
        )
        .unwrap();
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value,
        Value::I32(42)
    );
    let value = runtime
        .execute(&loaded, "boxed", &[], &context)
        .unwrap()
        .return_value;
    let method = runtime
        .runtime()
        .resolve_interface_method(
            &value,
            &host.trait_implementations[0].methods[0].trait_method,
        )
        .unwrap();
    runtime
        .runtime()
        .validate_interface_method_result(
            &method,
            &Value::Tuple(vec![Value::HostRoot(root), Value::I32(42)]),
        )
        .unwrap();
    let mut foreign = kagari_runtime::Runtime::default();
    let ty = foreign
        .register_host_type(HostTypeRegistration::new(host, "Counter"))
        .unwrap();
    let invalid = foreign
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    assert!(
        runtime
            .runtime()
            .validate_interface_method_result(
                &method,
                &Value::Tuple(vec![Value::HostRoot(invalid), Value::I32(42)])
            )
            .is_err()
    );
}
