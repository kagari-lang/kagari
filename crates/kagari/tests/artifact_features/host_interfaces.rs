#[cfg(feature = "native")]
use bytecode::load_bytecode;
use kagari::program::native::NativeCompiler;
use kagari_stdlib as stdlib;
// Share the Runtime-owned verified bytecode preparation helper.
#[path = "../../../kagari-runtime/tests/support/bytecode.rs"]
mod bytecode;
// The parent test target selects this file with #[path]; keep its fixture nested.
#[path = "host_interfaces/common.rs"]
mod common;

use kagari::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::Compilation,
};
use kagari_bytecode::{
    instruction::NativeImportId,
    module::CallableTarget,
    program::{BytecodeProgram, identity, verify_program},
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::identity::{
    ModuleIdentity, PackageId, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_contract::types::{InterfaceBridge, PublicItem};
use kagari_runtime::execution::reentry::reenter;
use kagari_runtime::{
    error::RuntimeErrorKind,
    host::{HostError, HostObjectId},
    module::entry::ProgramEntry,
    native::{builder::ModuleBuilder, conversion::context::ConversionContext, host, types::Type},
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_source::source_database::SourceLayer;
use kagari_stdlib::declarations::StandardDeclarations;
use kagari_types::{
    declaration::host::{
        HostCallPolicy, HostPassingStyle, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy,
        PathAccess,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    slice,
    sync::{Arc, Mutex},
};

const SOURCE: &str = concat!(
    include_str!("../../../../examples/host-interfaces.kgr"),
    "\nfn answer() -> i32 { 42 }\npub fn fail() -> i32 { boxed().read(-1) }\n"
);

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
    let fixture = common::Fixture::new(Default::default(), |_, args| Ok(args[1]));
    let native_module = fixture
        .artifact
        .records()
        .program
        .modules
        .iter()
        .find(|module| module.identity == fixture.module.declaration().identity)
        .unwrap();
    assert_eq!(native_module.interface_tables.len(), 1);
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        fixture.trace.lock().unwrap().clear();
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(
                &fixture
                    .artifact
                    .records()
                    .to_bytes(fixture.artifact.definitions())
                    .unwrap(),
            )
            .unwrap()
        } else {
            fixture.artifact.clone()
        };
        let context = context(jit);
        let mut runtime = fixture.engine.runtime(context.clone());
        common::register_root(&mut runtime);
        let loaded_program =
            Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
        let loaded = runtime
            .load_program(loaded_program.program(), Default::default())
            .unwrap();
        runtime.collect_garbage().unwrap();
        let roots = runtime.gc_stats().root_registrations;
        let result = if jit {
            let mut backend = CraneliftBackend::for_host().unwrap();
            let prepared = NativeCompiler::default()
                .prepare(
                    &loaded_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &context.cancellation,
                )
                .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
                .unwrap();
            runtime.execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, ProgramEntry::Main, &[], &context)
        }
        .unwrap();
        assert_eq!(result.return_value.as_i32().unwrap(), 42);
        assert_eq!(*fixture.trace.lock().unwrap(), [0, 20, 0, 22]);
        drop(result);
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc_stats().root_registrations, roots);
    }
}

#[test]
fn host_child_interfaces_upcast_through_precompiled_parent_bridges() {
    let mut config = common::Config {
        source: SOURCE.replace("pub fn boxed() -> Reader<Item = i32>", "trait Child: Reader<Item = i32> {}\npub fn boxed() -> Child")
            .replace("fn main() -> i32 { read(make()) + boxed().read(22) }", "fn main() -> i32 { val child = boxed(); val parent: Reader<Item = i32> = child; parent.read(20) + child.read(22) }"),
        child: true,
        ..Default::default()
    };
    config
        .trait_source
        .push_str("\ntrait Child: Reader<Item = i32> {}");
    let fixture = common::Fixture::new(config, |_, args| Ok(args[1]));
    let artifact = BytecodeArtifact::from_bytes(
        &fixture
            .artifact
            .records()
            .to_bytes(fixture.artifact.definitions())
            .unwrap(),
    )
    .unwrap();
    let context = context(false);
    let mut runtime = fixture.engine.runtime(context.clone());
    common::register_root(&mut runtime);
    let program =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(result.return_value.as_i32().unwrap(), 42);
    assert_eq!(*fixture.trace.lock().unwrap(), [0, 20, 22]);
    drop(result);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
}

#[test]
fn invalid_host_associated_schemas_and_dispatch_targets_are_rejected() {
    let fixture = common::Fixture::new(Default::default(), |_, args| Ok(args[1]));
    let definitions = DefinitionContext::new().unwrap();
    let artifact = fixture
        .artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    let native = artifact
        .program
        .modules
        .iter()
        .position(|module| module.identity == fixture.module.declaration().identity)
        .unwrap();
    let make = artifact
        .program
        .modules
        .iter()
        .flat_map(|module| &module.native_imports)
        .find(|import| {
            table
                .resolve(import.instance.declaration)
                .unwrap()
                .last()
                .is_some_and(|part| part.name == "make")
        })
        .unwrap()
        .clone();
    for mutation in 0..6 {
        let mut program = artifact.program.clone();
        let module = &mut program.modules[native];
        let interface = module
            .public_items
            .iter_mut()
            .find_map(|item| {
                if let PublicItem::InterfaceTable(table) = item {
                    Some(table)
                } else {
                    None
                }
            })
            .unwrap();
        let Ty::Trait(applied) = &mut interface.trait_type else {
            unreachable!()
        };
        match mutation {
            0 => applied.associated_types.clear(),
            1 => {
                *applied.associated_types.values_mut().next().unwrap() =
                    Ty::Builtin(BuiltinType::F32)
            }
            2 => {
                *applied.associated_types.values_mut().next().unwrap() =
                    Ty::Builtin(BuiltinType::I64)
            }
            3 => interface.bridge = Some(InterfaceBridge::Intrinsic),
            4 => module.interface_tables[0].methods.clear(),
            5 => {
                let target = NativeImportId::new(module.native_imports.len());
                module.native_imports.push(make.clone());
                module.interface_tables[0].methods[0].target = CallableTarget::Native(target);
            }
            _ => unreachable!(),
        }
        assert!(
            verify_program(&program, &definitions.snapshot(), &Default::default()).is_err(),
            "accepted mutation {mutation}"
        );
        assert!(
            BytecodeArtifact::from_program(
                program,
                Default::default(),
                &definitions,
                &Default::default()
            )
            .is_err()
        );
    }
}

#[test]
fn rooted_host_interfaces_survive_gc_reentry_reload_and_trap_cleanup() {
    let source = SOURCE.replace(
        "pub fn boxed() -> Reader<Item = i32> { make() }",
        "pub fn boxed() -> Reader<Item = i32> { val eraser: EraseReader = 0; eraser.erase(make()) }",
    );
    let fixture = common::Fixture::new(
        common::Config {
            source: source.clone(),
            ..Default::default()
        },
        |ctx, args| {
            let amount = args[1].as_i32().unwrap();
            if amount == -1 {
                return Err(HostError::new("expected host trap"));
            }
            if amount == 21 {
                let version = ctx.runtime().execution_root().unwrap().program_root();
                let answer = version
                    .bytecode
                    .functions
                    .iter()
                    .find(|function| function.name == "answer")
                    .unwrap()
                    .id;
                let arguments = ctx.retain_temporaries(&[]).unwrap();
                let value = reenter(ctx, &version, answer, arguments).unwrap();
                ctx.runtime_mut().collect_garbage().unwrap();
                return ctx
                    .import(&value)
                    .map_err(|error| HostError::new(error.to_string()));
            }
            Ok(args[1])
        },
    );
    let mut vm = fixture.runtime();
    let root = common::register_root(&mut vm);
    let loaded = load_bytecode(
        &mut vm,
        "host-interfaces",
        artifact_program(fixture.artifact.clone()),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    let value = vm
        .execute_module(&loaded, "boxed", vm.execution_options())
        .unwrap()
        .return_value;
    vm.collect_garbage().unwrap();
    let method = fixture.method(&loaded);
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[RootedValue::from(21_i32)])
            .unwrap()
            .as_i32()
            .unwrap(),
        42
    );
    assert!(
        vm.execute_module(&loaded, "fail", vm.execution_options())
            .is_err()
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, roots + 1);
    {
        let host_value = vm.host_root_value(root).unwrap();
        let scope = vm.host_scope(slice::from_ref(&host_value)).unwrap();
        assert!(
            scope
                .borrows()
                .borrow_unique(root.object_id(), root.type_id())
                .is_ok()
        );
    }
    let file = fixture
        .engine
        .set_source(
            "mem://host-interface",
            source.replace("fn answer() -> i32 { 42 }", "fn answer() -> i32 { 43 }"),
            SourceLayer::Base,
        )
        .unwrap();
    let checked = fixture
        .engine
        .compile_snapshot(fixture.engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = fixture
        .engine
        .emit_artifact(&checked, Default::default())
        .unwrap();
    vm.stage_reload_artifact(
        &_loaded_program,
        Some("host-interfaces".into()),
        artifact,
        &Default::default(),
    )
    .and_then(|candidate| vm.publish_staged_reload(candidate))
    .unwrap();
    let _new_program = _loaded_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let current_method = fixture.method(&new);
    let new_value = vm
        .execute_module(&new, "boxed", vm.execution_options())
        .unwrap()
        .return_value;
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[RootedValue::from(21_i32)])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    assert_eq!(
        vm.invoke_interface_method(&new_value, &current_method, &[RootedValue::from(21_i32)])
            .unwrap()
            .as_i32()
            .unwrap(),
        43
    );
    let table_owner = fixture.table_owner(&new);
    let host_value = vm.host_root_value(root).unwrap();
    drop(vm.make_interface(&table_owner, 0, &host_value).unwrap());
    let mut foreign = fixture.runtime();
    let foreign_handle = common::register_root(&mut foreign);
    let foreign_value = foreign.host_root_value(foreign_handle).unwrap();
    assert_eq!(
        vm.make_interface(&table_owner, 0, &foreign_value)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    drop(host_value);
    drop(value);
    drop(new_value);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, roots);
    assert!(
        vm.modules().loaded(loaded.key()).is_none(),
        "retained interface values release their old executable after the last root"
    );
}

#[test]
fn host_associated_outputs_are_checked_against_trait_bounds_without_calls() {
    let source = "trait Describe<T> { fn number(self) -> i32; } trait Reader { type Item: Describe<Self>; } use demo::Counter; fn accept(value: Counter) {} fn main() -> i32 { 42 }";
    let contract = common::source_contract(source.split(" use demo").next().unwrap(), None);
    let source_identity = contract.records().program.modules
        [contract.records().program.root.index()]
    .identity
    .clone();
    let catalog = StandardDeclarations::default().catalog().unwrap();
    let mut module = ModuleBuilder::new("demo", &catalog).unwrap();
    let reader = common::reference_trait(&mut module, &contract, "Reader");
    let describe = common::reference_trait(&mut module, &contract, "Describe");
    let mut counter = module.define_type("Counter").unwrap();
    counter
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    let counter = counter.finish().unwrap();
    let mut other = module.define_type("Other").unwrap();
    other
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::Opaque,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        })
        .unwrap();
    let other = other.finish().unwrap();
    module
        .implement(other.clone(), |group| {
            group.trait_impl(describe.apply([counter.apply([])?])?, |methods| {
                methods.host_policy(
                    "number",
                    HostCallPolicy {
                        passing: vec![HostPassingStyle::SharedBorrow],
                        effects: Default::default(),
                    },
                )?;
                methods.bind(
                    "number",
                    host::callback(|_, _| panic!("cold bound checking called Rust")),
                )
            })
        })
        .unwrap();
    module
        .implement(counter, |group| {
            group.trait_impl(reader.apply([])?, |methods| {
                methods.associated_type("Item", other.apply([])?)
            })
        })
        .unwrap();
    let engine = KagariEngine::with_native_modules(
        Default::default(),
        stdlib::modules()
            .unwrap()
            .into_iter()
            .chain(vec![module.finish().unwrap()])
            .collect(),
    )
    .unwrap();
    let file = engine.bind_module(common::URI, source_identity).unwrap();
    engine
        .set_source(common::URI, source.into(), SourceLayer::Base)
        .unwrap();
    let checked = engine
        .compile_snapshot(engine.source_snapshot(), file, &Default::default())
        .unwrap();
    let artifact = engine.emit_artifact(&checked, Default::default()).unwrap();
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let table = identity::collect(
        &artifact.program.modules.iter().collect::<Vec<_>>(),
        &definitions,
        &Default::default(),
    )
    .unwrap();
    verify_program(&artifact.program, &table, &Default::default()).unwrap();
    let mut program = artifact.program;
    let module = program
        .modules
        .iter_mut()
        .find(|module| module.identity.package.0 == "demo")
        .unwrap();
    let removed = module
        .public_items
        .iter()
        .find_map(|item| {
            if let PublicItem::InterfaceTable(table) = item {
                let Ty::Trait(trait_type) = &table.trait_type else {
                    unreachable!()
                };
                definitions
                    .snapshot()
                    .resolve(trait_type.declaration)
                    .unwrap()
                    .last()
                    .is_some_and(|part| part.name == "Describe")
                    .then_some(table.declaration)
            } else {
                None
            }
        })
        .unwrap();
    module.public_items.retain(
        |item| !matches!(item, PublicItem::InterfaceTable(table) if table.declaration == removed),
    );
    assert!(verify_program(&program, &definitions.snapshot(), &Default::default()).is_err());
    assert!(
        BytecodeArtifact::from_program(
            program,
            Default::default(),
            &definitions,
            &Default::default()
        )
        .is_err()
    );
}

#[test]
fn imported_host_interfaces_preserve_generic_inputs_and_associated_outputs() {
    let source = "use std::hash::Hash; pub trait Reader<T: Eq + Hash> { type Item: Eq + Hash; fn read(self, amount: T) -> Self::Item; }";
    let fixture = common::Fixture::new(common::Config {
        source: source.into(),
        trait_source: source.into(),
        owner: Some(ModuleIdentity {package: PackageId("pkg".into()), path: vec!["api".into()]}),
        reader_arguments: vec![Type::i32()],
        consumer: Some("use pkg::api::Reader; use demo::make; fn main() -> i32 { val reader: Reader<i32, Item = i32> = make(); reader.read(42) }".into()),
        ..Default::default()
    }, |_, args| Ok(args[1]));
    let artifact = BytecodeArtifact::from_bytes(
        &fixture
            .artifact
            .records()
            .to_bytes(fixture.artifact.definitions())
            .unwrap(),
    )
    .unwrap();
    let context = context(false);
    let mut runtime = fixture.engine.runtime(context.clone());
    common::register_root(&mut runtime);
    let program =
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    runtime.collect_garbage().unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let result = runtime
        .execute(&loaded, ProgramEntry::Main, &[], &context)
        .unwrap();
    assert_eq!(result.return_value.as_i32().unwrap(), 42);
    assert_eq!(*fixture.trace.lock().unwrap(), [0, 42]);
    drop(result);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
}

#[test]
fn dynamic_host_calls_enforce_permissions_and_registered_output_contracts() {
    let calls = Arc::new(Mutex::new(0));
    let recorded = calls.clone();
    let fixture = common::Fixture::new(Default::default(), move |_, args| {
        *recorded.lock().unwrap() += 1;
        Ok(args[1])
    });
    let mut vm = fixture.runtime();
    common::register_root(&mut vm);
    let loaded = load_bytecode(
        &mut vm,
        "permissions",
        artifact_program(fixture.artifact.clone()),
    )
    .unwrap();
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let roots = vm.gc_stats().root_registrations;
    let value = vm
        .execute_module(&loaded, "boxed", vm.execution_options())
        .unwrap()
        .return_value;
    let method = fixture.method(&loaded);
    assert_eq!(
        vm.invoke_interface_method(&value, &method, &[RootedValue::from(42_i32)])
            .unwrap()
            .as_i32()
            .unwrap(),
        42
    );
    assert_eq!(*calls.lock().unwrap(), 1);
    drop(value);
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().root_registrations, roots);

    // Same source-owned trait ABI, different installed associated output. The
    // portable requirement rejects it before any callback can run.
    for mismatch in [false, true] {
        let mut config = common::Config::default();
        if mismatch {
            config.output = |_| Type::i64();
            config.source = SOURCE
                .replace("Item = i32", "Item = i64")
                .replace("-> i32", "-> i64");
            // Keep the trait input and the ordinary reentry helper unchanged.
            config.source = config.source.replace("amount: i64", "amount: i32");
        }
        let registered = common::Fixture::new(config, |_, _| panic!("load called Rust"));
        let mut runtime = registered.runtime();
        assert_eq!(
            load_bytecode(
                &mut runtime,
                "registered-outputs",
                artifact_program(fixture.artifact.clone())
            )
            .is_err(),
            mismatch
        );
    }
}

#[test]
fn interface_method_results_validate_nested_host_roots() {
    let mut config = common::Config::default();
    config.source = SOURCE
        .replace("type Item: Eq + Hash;", "type Item;")
        .replace("Item = i32", "Item = (Counter, i32)")
        .replace(
            "read(make()) + boxed().read(22)",
            "read(make())[1] + boxed().read(22)[1]",
        )
        .replace("boxed().read(-1)", "boxed().read(-1)[1]");
    config.trait_source = config
        .trait_source
        .replace("type Item: Eq + Hash;", "type Item;");
    config.output = |counter| Type::tuple([counter.apply([]).unwrap(), Type::i32()]).unwrap();
    let fixture = common::Fixture::new(config, |cx, args| {
        let root = cx.runtime().host().root(HostObjectId(7)).unwrap();
        let root = cx.runtime_mut().host_root_value(root).unwrap();
        let amount = args[1].as_i32().unwrap();
        let owner = cx.runtime().execution_root().unwrap();
        cx.convert_value(&owner, |conversion| {
            let root = conversion.import(&root)?;
            conversion.allocate_tuple(&[root, ValueView::from(amount)])
        })
        .map_err(|error| HostError::new(error.to_string()))
    });
    let context = context(false);
    let mut runtime = fixture.engine.runtime(context.clone());
    let root = common::register_root(&mut runtime);
    let loaded_program = Compilation::from_artifact(
        fixture.artifact.clone(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let loaded = runtime
        .load_program(loaded_program.program(), Default::default())
        .unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .as_i32()
            .unwrap(),
        42
    );
    let value = runtime
        .execute(&loaded, "boxed", &[], &context)
        .unwrap()
        .return_value;
    let method = runtime
        .resolve_interface_method(
            &value,
            &fixture.method(&runtime.select_program(&loaded).unwrap()),
        )
        .unwrap();
    let selected = runtime.select_program(&loaded).unwrap();
    let host_value = runtime.host_root_value(root).unwrap();
    let output = {
        let mut cx = ConversionContext::new(&mut runtime, &selected).unwrap();
        let host_value = cx.import(&host_value).unwrap();
        let tuple = cx
            .allocate_tuple(&[host_value, ValueView::from(42_i32)])
            .unwrap();
        cx.retain(&tuple).unwrap()
    };
    runtime
        .validate_interface_method_result(&method, &output)
        .unwrap();
    let mut foreign = fixture.engine.runtime(context.clone());
    let foreign_root = common::register_root(&mut foreign);
    let foreign_program = foreign
        .load_program(loaded_program.program(), Default::default())
        .unwrap();
    let foreign_owner = foreign.select_program(&foreign_program).unwrap();
    let foreign_value = foreign.host_root_value(foreign_root).unwrap();
    let invalid_output = {
        let mut cx = ConversionContext::new(&mut foreign, &foreign_owner).unwrap();
        let root = cx.import(&foreign_value).unwrap();
        let tuple = cx.allocate_tuple(&[root, ValueView::from(42_i32)]).unwrap();
        cx.retain(&tuple).unwrap()
    };
    assert_eq!(
        runtime
            .validate_interface_method_result(&method, &invalid_output)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(!runtime.is_quarantined());
}

fn artifact_program(
    artifact: DefinitionMetadata<BytecodeArtifact>,
) -> DefinitionMetadata<BytecodeProgram> {
    let (definitions, artifact) = artifact.into_parts();
    DefinitionMetadata::checked(definitions, artifact.program, &Default::default()).unwrap()
}
