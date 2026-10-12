use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, NativeImportId},
    module::CallableTarget,
    program::{BytecodeProgram, identity, verify_program},
};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext,
    metadata::DefinitionMetadata, table::DefinitionId,
};
use kagari_contract::{native_import::NativeImport, types::PublicItem};
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    host::{HostError, HostFunction, HostObjectId, HostSchemaEpoch, HostTypeRegistration},
    module::LoadedModule,
    native::module::NativeModule,
    value::Value,
};
use kagari_source::source_database::SourceLayer;
use kagari_types::host_interface::{
    HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
    type_declaration::{
        HostAssociatedTypeBinding, HostMethodDeclaration, HostTraitImplementationDeclaration,
        HostTraitMethodBinding, HostTypeDeclaration, HostTypeOwnership, PathAccess,
    },
    value_type::HostValueType,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::{reentry::reenter, vm::Vm};
use std::sync::{Arc, Mutex};

const SOURCE: &str = concat!(
    include_str!("../../../examples/host-interfaces.kgr"),
    "\nfn answer() -> i32 { 42 }\npub fn fail() -> i32 { boxed().read(-1) }\n"
);

struct Fixture {
    engine: KagariEngine,
    artifact: DefinitionMetadata<BytecodeArtifact>,
    definitions: DefinitionContext,
    host: HostTypeDeclaration,
    make: HostFunctionDeclaration,
}

fn member(
    definitions: &DefinitionContext,
    owner: DefinitionId,
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    definitions.intern_child(owner, kind, name, 0).unwrap()
}

fn fixture() -> Fixture {
    let definitions = DefinitionContext::new().unwrap();
    let engine = KagariEngine::default();
    let file = engine
        .set_source("mem://host-interface", SOURCE.into(), SourceLayer::Base)
        .unwrap();
    let owner = engine
        .source_snapshot()
        .file(file)
        .unwrap()
        .module_identity()
        .clone();
    let root = definitions.intern_root(&owner).unwrap();
    let trait_id = member(&definitions, root, DefinitionKind::Trait, "Reader");
    let mut host = HostTypeDeclaration::new(&definitions, "demo.Counter").unwrap();
    host.ownership = HostTypeOwnership::HostRoot;
    host.path_access = PathAccess::ReadOnly;
    let method = HostMethodDeclaration::new(
        &definitions,
        host.id,
        "read",
        vec![HostParameter {
            name: "amount".into(),
            ty: HostValueType::I32,
            passing: HostPassingStyle::Owned,
        }],
        HostValueType::I32,
    )
    .unwrap();
    let mut implementation = HostTraitImplementationDeclaration::new(
        trait_id,
        vec![],
        vec![HostTraitMethodBinding {
            trait_method: member(&definitions, trait_id, DefinitionKind::Method, "read"),
            host_method: method.id,
        }],
    );
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: member(
                &definitions,
                trait_id,
                DefinitionKind::AssociatedType,
                "Item",
            ),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    host.methods.push(method);
    let make = HostFunctionDeclaration::new(
        &definitions,
        "demo.make",
        vec![],
        HostValueType::Opaque(host.id),
    )
    .unwrap();
    let interface = HostInterface {
        types: vec![host.clone()],
        functions: vec![make.clone()],
        paths: vec![],
    };
    // Reading these declarations does not register any runtime callback.
    let interface =
        HostInterface::from_bytes(&interface.to_bytes(&definitions.snapshot()).unwrap()).unwrap();
    engine.set_host_interface(interface).unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    Fixture {
        engine,
        artifact,
        definitions,
        host,
        make,
    }
}

fn context(jit: bool) -> ExecutionContext {
    ExecutionContext {
        jit_policy: if jit {
            JitPolicy::Enabled
        } else {
            JitPolicy::Disabled
        },
        ..Default::default()
    }
}

#[test]
fn host_associated_types_and_dynamic_interfaces_share_the_host_call_boundary() {
    let Fixture {
        engine,
        artifact,
        definitions,
        host,
        make,
    } = fixture();
    assert_eq!(
        artifact.records().program.modules[artifact.records().program.root.index()]
            .interface_tables
            .len(),
        1
    );
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap()
        } else {
            artifact.clone()
        };
        let context = context(jit);
        let mut runtime = engine.runtime(context.clone());
        let ty = runtime
            .register_host_type(HostTypeRegistration::new(
                DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                "Counter",
            ))
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                DefinitionMetadata::checked(
                    definitions.snapshot(),
                    make.clone(),
                    &Default::default(),
                )
                .unwrap(),
                move |cx, _| {
                    calls.lock().unwrap().push(0);
                    Ok(cx.runtime().gc().alloc_host_root(root).unwrap())
                },
            ))
            .unwrap();
        let calls = trace.clone();
        runtime
            .register_host_function(
                HostFunction::method(
                    &DefinitionMetadata::checked(
                        definitions.snapshot(),
                        host.clone(),
                        &Default::default(),
                    )
                    .unwrap(),
                    host.methods[0].id,
                    move |ctx, args| {
                        let [Value::HostRoot(_), Value::I32(amount)] = args else {
                            panic!("invalid host arguments")
                        };
                        calls.lock().unwrap().push(*amount);
                        assert!(
                            ctx.borrows()
                                .borrow_unique(root.object_id(), root.type_id())
                                .is_err()
                        );
                        Ok(Value::I32(*amount))
                    },
                )
                .unwrap(),
            )
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let result = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(
            result
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(42)
        );
        assert_eq!(*trace.lock().unwrap(), [0, 20, 0, 22]);
    }
}

#[test]
fn host_child_interfaces_upcast_through_precompiled_parent_bridges() {
    let Fixture {
        engine,
        definitions,
        mut host,
        make,
        ..
    } = fixture();
    let parent = definitions
        .snapshot()
        .parent(host.trait_implementations[0].trait_id)
        .unwrap()
        .unwrap();
    let child = member(&definitions, parent, DefinitionKind::Trait, "Child");
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
    engine
        .set_host_interface(
            DefinitionMetadata::checked(definitions.snapshot(), interface, &Default::default())
                .unwrap(),
        )
        .unwrap();
    let source = SOURCE.replace("pub fn boxed() -> Reader<Item = i32>", "trait Child: Reader<Item = i32> {}\npub fn boxed() -> Child")
        .replace("fn main() -> i32 { read(make()) + boxed().read(22) }", "fn main() -> i32 { val child = boxed(); val parent: Reader<Item = i32> = child; parent.read(20) + child.read(22) }");
    let file = engine
        .set_source("mem://host-interface", source, SourceLayer::Base)
        .unwrap();
    let context = context(false);
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            DefinitionMetadata::checked(definitions.snapshot(), make, &Default::default()).unwrap(),
            move |cx, _| Ok(cx.runtime().gc().alloc_host_root(root).unwrap()),
        ))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(
                &DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                host.methods[0].id,
                |_, args| Ok(args[1]),
            )
            .unwrap(),
        )
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn invalid_host_associated_schemas_and_bridge_code_are_rejected() {
    let Fixture { artifact, .. } = fixture();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &definitions,
    )
    .unwrap();
    verify_program(&artifact.program, &table).unwrap();
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
                let PublicItem::InterfaceTable(table) = module
                    .public_items
                    .iter_mut()
                    .find(|item| matches!(item, PublicItem::InterfaceTable(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                let Ty::Trait(interface) = &mut table.trait_type else {
                    unreachable!()
                };
                *interface.associated_types.values_mut().next().unwrap() =
                    Ty::Builtin(BuiltinType::I64);
            }
            3 => {
                let PublicItem::InterfaceTable(table) = module
                    .public_items
                    .iter_mut()
                    .find(|item| matches!(item, PublicItem::InterfaceTable(_)))
                    .unwrap()
                else {
                    unreachable!()
                };
                table.host_bridge = false;
            }
            4 => {
                let CallableTarget::Script(function) = module.interface_tables[0].methods[0].target
                else {
                    panic!("host script shim")
                };
                let index = function.index();
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
                alternative.id = member(&definitions, host.id, DefinitionKind::Method, "other");
                alternative.name = "other".into();
                host.methods.push(alternative.clone());
                let contract = host
                    .method_contract(&alternative.id, &definitions.snapshot())
                    .unwrap();
                let import = NativeImportId::new(module.native_imports.len());
                module
                    .native_imports
                    .push(NativeImport::from_host(&contract));
                module.host_interface.functions.push(contract);
                let CallableTarget::Script(function) = module.interface_tables[0].methods[0].target
                else {
                    panic!("host script shim")
                };
                let index = function.index();
                let function = &mut module.functions[index];
                let BytecodeInstruction::Call { callee, .. } =
                    &mut function.instructions[function.metadata.params.len()]
                else {
                    unreachable!()
                };
                *callee = CallTarget::Native(import);
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&program, &definitions.snapshot()).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(BytecodeArtifact::from_program(program, Default::default(), &definitions).is_err());
    }
}

#[test]
fn rooted_host_interfaces_survive_gc_reentry_reload_and_trap_cleanup() {
    let Fixture {
        engine,
        artifact,
        definitions,
        host,
        make,
    } = fixture();
    let _context = context(false);
    let mut vm = Vm::new({
        let mut registered = Runtime::new(RuntimeConfig {
            ..Default::default()
        });
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    });
    let ty = vm
        .runtime_mut()
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let root = vm
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(HostFunction::new(
            DefinitionMetadata::checked(definitions.snapshot(), make, &Default::default()).unwrap(),
            move |cx, _| Ok(cx.runtime().gc().alloc_host_root(root).unwrap()),
        ))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(
            HostFunction::method(
                &DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                host.methods[0].id,
                move |ctx, args| {
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
                        let value = reenter(ctx, &version, answer, &[]).unwrap();
                        ctx.runtime().collect_garbage().unwrap();
                        return Ok(value.value(ctx.runtime().gc()).unwrap());
                    }
                    Ok(Value::I32(*amount))
                },
            )
            .unwrap(),
        )
        .unwrap();
    let loaded = vm
        .runtime_mut()
        .load_program("host-interfaces", artifact_program(artifact))
        .unwrap();
    let value = vm
        .execute(&loaded, "boxed")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let rooted = vm.runtime().root_value(value).unwrap();
    vm.runtime().collect_garbage().unwrap();
    let method = loaded_method(
        &loaded,
        &definitions,
        host.trait_implementations[0].methods[0].trait_method,
    );
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[Value::I32(21)])
            .unwrap(),
        Value::I32(42)
    );
    assert!(vm.execute(&loaded, "fail").is_err());
    assert_eq!(vm.runtime().gc().active_roots(), 1);
    {
        let scope = vm
            .runtime()
            .host_scope(&[vm.runtime().gc().alloc_host_root(root).unwrap()])
            .unwrap();
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
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let new = vm
        .reload_artifact(&loaded, "host-interfaces", artifact, &Default::default())
        .unwrap();
    let new_value = vm
        .execute(&new, "boxed")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let new_root = vm.runtime().root_value(new_value).unwrap();
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
    let mut foreign = {
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let foreign_type = foreign
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let foreign_root = foreign
        .register_host_root(HostObjectId(7), foreign_type, HostSchemaEpoch::new(0))
        .unwrap();
    assert!(
        vm.runtime()
            .make_interface(
                &loaded,
                0,
                vm.runtime().gc().alloc_host_root(foreign_root).unwrap()
            )
            .is_err()
    );
    {
        let scope = vm
            .runtime()
            .host_scope(&[vm.runtime().gc().alloc_host_root(root).unwrap()])
            .unwrap();
        let token = scope
            .borrows()
            .borrow_shared(root.object_id(), root.type_id())
            .unwrap();
        assert!(
            vm.runtime()
                .make_interface(&loaded, 0, vm.runtime().gc().alloc_host_ref(token).unwrap())
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
    let Fixture {
        engine,
        definitions,
        mut host,
        make,
        ..
    } = fixture();
    let reader_id = host.trait_implementations[0].trait_id;
    let parent = definitions.snapshot().parent(reader_id).unwrap().unwrap();
    let describe_id = member(&definitions, parent, DefinitionKind::Trait, "Describe");
    let mut other = HostTypeDeclaration::new(&definitions, "demo.Other").unwrap();
    let number =
        HostMethodDeclaration::new(&definitions, other.id, "number", vec![], HostValueType::I32)
            .unwrap();
    other.methods.push(number.clone());
    other
        .trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            describe_id,
            vec![HostValueType::Opaque(host.id)],
            vec![HostTraitMethodBinding {
                trait_method: member(&definitions, describe_id, DefinitionKind::Method, "number"),
                host_method: number.id,
            }],
        ));
    host.trait_implementations[0].methods.clear();
    host.trait_implementations[0].associated_types[0].ty = HostValueType::Opaque(other.id);
    let file = engine.set_source("mem://host-interface", "trait Describe<T> { fn number(self) -> i32; } trait Reader { type Item: Describe<Self>; } use demo::Counter; fn accept(value: Counter) {} fn main() -> i32 { 42 }".into(), SourceLayer::Base).unwrap();
    engine
        .set_host_interface(
            DefinitionMetadata::checked(
                definitions.snapshot(),
                HostInterface {
                    types: vec![host, other],
                    functions: vec![make],
                    paths: vec![],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &definitions,
    )
    .unwrap();
    verify_program(&artifact.program, &table).unwrap();
    let mut program = artifact.program;
    let other = program.modules[program.root.index()]
        .host_interface
        .types
        .iter_mut()
        .find(|host| host.symbol == "demo.Other")
        .unwrap();
    other.trait_implementations.clear();
    assert!(verify_program(&program, &definitions.snapshot()).is_err());
}

#[test]
fn imported_host_interfaces_preserve_generic_inputs_and_associated_outputs() {
    let Fixture {
        engine,
        definitions,
        mut host,
        make,
        ..
    } = fixture();
    let owner = ModuleIdentity {
        package: PackageId("pkg".into()),
        path: vec!["api".into()],
    };
    engine
        .bind_module("mem://host-interface", owner.clone())
        .unwrap();
    let implementation = &mut host.trait_implementations[0];
    let root = definitions.intern_root(&owner).unwrap();
    implementation.trait_id = member(&definitions, root, DefinitionKind::Trait, "Reader");
    implementation.trait_arguments = vec![HostValueType::I32];
    implementation.associated_types[0].declaration = member(
        &definitions,
        implementation.trait_id,
        DefinitionKind::AssociatedType,
        "Item",
    );
    implementation.methods[0].trait_method = member(
        &definitions,
        implementation.trait_id,
        DefinitionKind::Method,
        "read",
    );
    engine
        .set_host_interface(
            DefinitionMetadata::checked(
                definitions.snapshot(),
                HostInterface {
                    types: vec![host.clone()],
                    functions: vec![make.clone()],
                    paths: vec![],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    engine.set_source("mem://host-interface", "use std::hash::Hash; pub trait Reader<T: Eq + Hash> { type Item: Eq + Hash; fn read(self, amount: T) -> Self::Item; }".into(), SourceLayer::Base).unwrap();
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
        .compile_snapshot(engine.source_snapshot(), root, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let value = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            DefinitionMetadata::checked(definitions.snapshot(), make, &Default::default()).unwrap(),
            move |cx, _| Ok(cx.runtime().gc().alloc_host_root(value).unwrap()),
        ))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(
                &DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                host.methods[0].id,
                |_, args| Ok(args[1]),
            )
            .unwrap(),
        )
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
}

#[test]
fn dynamic_host_calls_enforce_permissions_and_registered_output_contracts() {
    let Fixture {
        artifact,
        definitions,
        host,
        make,
        ..
    } = fixture();
    let _context = context(false);
    let mut vm = Vm::new({
        let mut registered = Runtime::new(RuntimeConfig {
            ..Default::default()
        });
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    });
    let ty = vm
        .runtime_mut()
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let root = vm
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    vm.runtime_mut()
        .register_host_function(HostFunction::new(
            DefinitionMetadata::checked(definitions.snapshot(), make.clone(), &Default::default())
                .unwrap(),
            move |cx, _| Ok(cx.runtime().gc().alloc_host_root(root).unwrap()),
        ))
        .unwrap();
    let calls = Arc::new(Mutex::new(0));
    let recorded = calls.clone();
    vm.runtime_mut()
        .register_host_function(
            HostFunction::method(
                &DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                host.methods[0].id,
                move |_, args| {
                    *recorded.lock().unwrap() += 1;
                    Ok(args[1])
                },
            )
            .unwrap(),
        )
        .unwrap();
    let loaded = vm
        .runtime_mut()
        .load_program("permissions", artifact_program(artifact.clone()))
        .unwrap();
    let value = vm
        .execute(&loaded, "boxed")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result");
    let rooted = vm.runtime().root_value(value).unwrap();
    let method = loaded_method(
        &loaded,
        &definitions,
        host.trait_implementations[0].methods[0].trait_method,
    );

    assert_eq!(
        vm.invoke_interface_method(
            &rooted.value(vm.runtime().gc()).unwrap(),
            &method,
            &[Value::I32(42)]
        )
        .unwrap(),
        Value::I32(42)
    );
    assert_eq!(*calls.lock().unwrap(), 1);

    for mismatch in [false, true] {
        let mut registered = host.clone();
        if mismatch {
            registered.trait_implementations[0].associated_types[0].ty = HostValueType::I64;
        }
        let mut runtime = {
            let mut registered = Runtime::new(RuntimeConfig {
                ..Default::default()
            });
            NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
            registered
        };
        runtime
            .register_host_type(HostTypeRegistration::new(
                DefinitionMetadata::checked(
                    definitions.snapshot(),
                    registered.clone(),
                    &Default::default(),
                )
                .unwrap(),
                "Counter",
            ))
            .unwrap();
        runtime
            .register_host_function(HostFunction::new(
                DefinitionMetadata::checked(
                    definitions.snapshot(),
                    make.clone(),
                    &Default::default(),
                )
                .unwrap(),
                |_, _| Ok(Value::Unit),
            ))
            .unwrap();
        runtime
            .register_host_function(
                HostFunction::method(
                    &DefinitionMetadata::checked(
                        definitions.snapshot(),
                        registered.clone(),
                        &Default::default(),
                    )
                    .unwrap(),
                    registered.methods[0].id,
                    |_, args| Ok(args[1]),
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(
            runtime
                .load_program("registered-outputs", artifact_program(artifact.clone()))
                .is_err(),
            mismatch
        );
    }
}

#[test]
fn interface_method_results_validate_nested_host_roots() {
    let Fixture {
        engine,
        definitions,
        mut host,
        make,
        ..
    } = fixture();
    let source = SOURCE
        .replace("type Item: Eq + Hash;", "type Item;")
        .replace("Item = i32", "Item = (Counter, i32)")
        .replace(
            "read(make()) + boxed().read(22)",
            "read(make())[1] + boxed().read(22)[1]",
        )
        .replace("boxed().read(-1)", "boxed().read(-1)[1]");
    let output = HostValueType::Tuple(vec![HostValueType::Opaque(host.id), HostValueType::I32]);
    host.methods[0].return_type = output.clone();
    host.trait_implementations[0].associated_types[0].ty = output;
    engine
        .set_host_interface(
            DefinitionMetadata::checked(
                definitions.snapshot(),
                HostInterface {
                    types: vec![host.clone()],
                    functions: vec![make.clone()],
                    paths: vec![],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let file = engine
        .set_source("mem://host-interface", source, SourceLayer::Base)
        .unwrap();
    let context = context(false);
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let mut runtime = engine.runtime(context.clone());
    let ty = runtime
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host.clone(), &Default::default())
                .unwrap(),
            "Counter",
        ))
        .unwrap();
    let root = runtime
        .runtime_mut()
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            DefinitionMetadata::checked(definitions.snapshot(), make, &Default::default()).unwrap(),
            move |cx, _| Ok(cx.runtime().gc().alloc_host_root(root).unwrap()),
        ))
        .unwrap();
    runtime
        .register_host_function(
            HostFunction::method(
                &DefinitionMetadata::checked(
                    definitions.snapshot(),
                    host.clone(),
                    &Default::default(),
                )
                .unwrap(),
                host.methods[0].id,
                move |cx, args| {
                    Ok(cx
                        .runtime()
                        .gc()
                        .alloc_tuple(vec![
                            cx.runtime().gc().alloc_host_root(root).unwrap(),
                            args[1],
                        ])
                        .unwrap())
                },
            )
            .unwrap(),
        )
        .unwrap();
    let loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &context)
            .unwrap()
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    let value = runtime
        .execute(&loaded, "boxed", &[], &context)
        .unwrap()
        .return_value
        .value(runtime.runtime().gc())
        .expect("retained execution result");
    let method = runtime
        .runtime()
        .resolve_interface_method(
            &value,
            &loaded_method(
                &loaded,
                &definitions,
                host.trait_implementations[0].methods[0].trait_method,
            ),
        )
        .unwrap();
    runtime
        .runtime()
        .validate_interface_method_result(
            &method,
            &runtime
                .runtime()
                .gc()
                .alloc_tuple(vec![
                    runtime.runtime().gc().alloc_host_root(root).unwrap(),
                    Value::I32(42),
                ])
                .unwrap(),
        )
        .unwrap();
    let mut foreign = {
        let mut registered = Runtime::default();
        NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut registered).unwrap();
        registered
    };
    let ty = foreign
        .register_host_type(HostTypeRegistration::new(
            DefinitionMetadata::checked(definitions.snapshot(), host, &Default::default()).unwrap(),
            "Counter",
        ))
        .unwrap();
    let invalid = foreign
        .register_host_root(HostObjectId(7), ty, HostSchemaEpoch::new(0))
        .unwrap();
    assert!(
        runtime
            .runtime()
            .validate_interface_method_result(
                &method,
                &runtime
                    .runtime()
                    .gc()
                    .alloc_tuple(vec![
                        runtime.runtime().gc().alloc_host_root(invalid).unwrap(),
                        Value::I32(42)
                    ])
                    .unwrap()
            )
            .is_err()
    );
}

fn artifact_program(
    artifact: DefinitionMetadata<BytecodeArtifact>,
) -> DefinitionMetadata<BytecodeProgram> {
    let (definitions, artifact) = artifact.into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
}

fn loaded_method(
    loaded: &LoadedModule,
    definitions: &DefinitionContext,
    method: DefinitionId,
) -> DefinitionId {
    let query = definitions.snapshot().resolve(method).unwrap().to_path();
    loaded.definitions().lookup(&query).unwrap()
}
