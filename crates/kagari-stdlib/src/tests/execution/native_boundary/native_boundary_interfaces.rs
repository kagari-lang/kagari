use super::{compile, compile_program};
#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{
    declarations::StandardDeclarations,
    tests::execution::native_boundary::support::round_trip_program,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    module::entry::ProgramEntry,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::NativeCall,
        declarations::{CallableRequirement, FunctionDecl, MethodDecl},
        module::NativeModule,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::view::ValueView,
};

use kagari_bytecode::{program::verify_program, verifier::BytecodeVerificationError};
use kagari_contract::types::ConcreteFunctionIdentity;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn generic_native_default_uses_the_interface_callers_type_arguments() {
    let mut module = ModuleBuilder::new(
        "example::generic_defaults",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = module.define_trait("Identity").unwrap();
    declaration.type_parameter("Item").unwrap();
    let identity = declaration
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    declaration
        .method(&identity, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone())?;
            method.returns(value)?;
            Ok(())
        })
        .unwrap();
    declaration
        .bind(
            identity,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, |cx| {
                cx.argument(1)
            }),
        )
        .unwrap();
    let contract = declaration.finish().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    module
        .implement(Type::i32(), |group| {
            group.trait_impl(contract.apply([Type::i32()])?, |methods| {
                methods.bind(
                    "identity",
                    NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                        observed.fetch_add(1, Ordering::SeqCst);
                        cx.argument(1)
                    }),
                )
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = compile_program(
        include_str!("../../../../tests/fixtures/native/generic-native-default.kgr"),
        Some(&module),
    );
    for mutation in 0..3 {
        let (definitions, mut forged) = program.clone().into_parts();
        let import = forged
            .modules
            .iter_mut()
            .flat_map(|owner| &mut owner.native_imports)
            .find(|import| import.generic.is_some())
            .unwrap();
        match mutation {
            0 => import.generic = None,
            1 => import.generic.as_mut().unwrap().parameters[0].position += 1,
            _ => import.signature.result = Type::bool().abi().clone(),
        }
        assert!(
            verify_program(&forged, &definitions, &Default::default()).is_err(),
            "native mutation {mutation}"
        );
    }
    let decoded = round_trip_program(program);
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "shared-native", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(
        vm.execute_module(&loaded, "overridden", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        vm.execute_module(&loaded, "nested", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(
        vm.execute_module(&loaded, "bounded", vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn registered_default_calls_the_selected_receiver_operation() {
    let mut module = ModuleBuilder::new(
        "example::defaults",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = module.define_trait("Read").unwrap();
    let read = declaration
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let twice = declaration
        .define_method(MethodDecl::instance("twice").returns(Type::i32()))
        .unwrap();
    let operation = declaration.operation(&read).unwrap();
    let selected = declaration
        .method(&twice, |method| method.requires(operation))
        .unwrap();
    declaration
        .bind(
            twice,
            NativeBinding::new(vec![Codec::Value], Codec::Exact(Type::i32()), move |cx| {
                let receiver = cx.argument(0)?;
                let target = cx.selected(&selected)?;
                let Some(value) = cx.call_values(target, &[receiver])?.as_i32() else {
                    return Err(RuntimeError::module_validation("checked Read result"));
                };
                Ok(ValueView::from(value * 2))
            }),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/interface-selected-default.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    for entry in [ProgramEntry::Main, ProgramEntry::Export("run_dynamic")] {
        assert_eq!(
            vm.execute_module(&loaded, entry, vm.execution_options())
                .unwrap()
                .return_value
                .as_i32(),
            Some(42)
        );
    }
}

#[test]
fn dynamic_list_iteration_adapts_the_concrete_cursor_result() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/dynamic-list.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn dynamic_iterator_adapters_round_trip_and_reject_forged_tables() {
    let program = compile_program(
        include_str!("../../../../tests/fixtures/native/dynamic-forged-table.kgr"),
        None,
    );
    let decoded = round_trip_program(program.clone());
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "round-trip", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    let (owner, table) = program
        .records()
        .modules
        .iter()
        .enumerate()
        .find_map(|(owner, module)| {
            module
                .interface_tables
                .iter()
                .position(|table| table.view.is_some())
                .map(|table| (owner, table))
        })
        .unwrap();
    for corruption in 0..5 {
        let (definitions, mut forged) = program.clone().into_parts();
        let table = &mut forged.modules[owner].interface_tables[table];
        let identity = ConcreteFunctionIdentity {
            declaration: table.declaration,
            arguments: table.arguments.clone(),
        };
        let view = table.view.as_mut().unwrap();
        match corruption {
            0 => view.results.clear(),
            1 => view.results[0].implementation = identity,
            2 => view.results.push(view.results[0].clone()),
            3 => view.interface.associated_types.clear(),
            _ => table.view = None,
        }
        assert!(
            matches!(
                verify_program(&forged, &definitions, &Default::default()),
                Err(BytecodeVerificationError::InvalidInterfaceTable)
            ),
            "corruption {corruption}"
        );
    }
}

#[test]
fn dynamic_iteration_preserves_gc_roots_and_detects_invalidation() {
    let mut builder = ModuleBuilder::new(
        "example::collection_gc",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let collect = builder
        .define_function(FunctionDecl::new("collect").returns(Type::unit()))
        .unwrap();
    builder
        .bind(collect, |cx: &mut NativeCall<'_>| -> NativeResult<()> {
            cx.collect_garbage()?;
            Ok(())
        })
        .unwrap();
    let module = builder.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/dynamic-list-gc.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    vm.collect_garbage().unwrap();
    let baseline = vm.gc_stats();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    assert!(
        vm.execute_module(&loaded, "trap", vm.execution_options())
            .is_err()
    );
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
    vm.collect_garbage().unwrap();
    assert_eq!(vm.gc_stats().objects, baseline.objects);
    assert_eq!(
        vm.gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn script_iterator_results_use_the_same_dynamic_adapter() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/dynamic-script-iterator.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn default_map_and_set_iterate_through_readonly_parent_views() {
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/dynamic-map-set.kgr"),
        None,
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn generic_native_default_uses_the_callers_ordering_operation() {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::comparison",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = module.define_trait("Compare").unwrap();
    let compare = declaration
        .define_method(MethodDecl::instance("compare").returns(language.ordering().unwrap()))
        .unwrap();
    let selected = declaration
        .method(&compare, |method| {
            let key = method.type_parameter("K")?.ty();
            method.parameter("left", key.clone())?;
            method.parameter("right", key.clone())?;
            method.bound(key.clone(), language.ord().apply([])?)?;
            method.requires(CallableRequirement::method(
                key,
                language.ord().method("cmp")?,
            )?)
        })
        .unwrap();
    declaration
        .bind(
            compare,
            NativeBinding::new(
                vec![Codec::Value, Codec::Value, Codec::Value],
                Codec::Value,
                move |cx| {
                    let args = [cx.argument(1)?, cx.argument(2)?];
                    let target = cx.selected(&selected)?;
                    cx.call_values(target, &args)
                },
            ),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let program = compile_program(
        include_str!("../../../../tests/fixtures/native/interface-ordering.kgr"),
        Some(&module),
    );
    let decoded = round_trip_program(program);
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "native-constraint", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn a_shared_method_calls_an_ordinary_generic_native_function() {
    let module = generic_identity_module();
    let program = compile_program(
        include_str!("../../../../tests/fixtures/native/shared-generic-native.kgr"),
        Some(&module),
    );
    let decoded = round_trip_program(program);
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "shared-helper", decoded).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

pub(super) fn generic_identity_module() -> NativeModule {
    let language = StandardDeclarations::default();
    let mut module = ModuleBuilder::new(
        "example::generic_helper",
        &language.catalog().expect("explicit standard providers"),
    )
    .unwrap();
    let identity = module
        .define_function(FunctionDecl::new("identity"))
        .unwrap();
    module
        .function(&identity, |function| {
            let value = function.type_parameter("T")?.ty();
            function.parameter("value", value.clone())?;
            function.returns(value)?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            identity,
            NativeBinding::new(vec![Codec::Value], Codec::Value, |cx| cx.argument(0)),
        )
        .unwrap();
    let singleton = module
        .define_function(FunctionDecl::new("singleton"))
        .unwrap();
    module
        .function(&singleton, |function| {
            let item = function.type_parameter("T")?.ty();
            function.parameter("value", item.clone())?;
            function.returns(Type::tuple([item])?)?;
            Ok(())
        })
        .unwrap();
    module
        .bind(
            singleton,
            |_cx: &mut NativeContext<'_>,
             (item,): (ScriptValue,)|
             -> NativeResult<(ScriptValue,)> { Ok((item,)) },
        )
        .unwrap();
    module.finish().unwrap()
}

#[test]
fn shared_native_returns_preserve_the_callers_layout_generation() {
    let module = generic_identity_module();
    let source = include_str!("../../../../tests/fixtures/native/shared-native-layout.kgr");
    let mut runtime = Runtime::default();
    kagari_runtime::native::module::NativeModule::install_all(
        &crate::modules().unwrap(),
        &mut runtime,
    )
    .unwrap();
    module.install(&mut runtime).unwrap();
    let old = load_bytecode(
        &mut runtime,
        "native-scope",
        compile_program(source, Some(&module)),
    )
    .unwrap();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let mut vm = runtime;
    let receiver = vm
        .execute_module(&old, "receiver", vm.execution_options())
        .unwrap()
        .return_value;
    let next_source = source
        .replace("val value: i32", "val value: i32, val extra: bool")
        .replace("value: 42 }", "value: 42, extra: true }");
    assert!(
        vm.stage_reload_program(
            &_old_program,
            None,
            compile_program(&next_source, Some(&module))
        )
        .is_err()
    );
    let next_source = source.replace("value: 42", "value: 43");
    let candidate = vm
        .stage_reload_program(
            &_old_program,
            Some("native-scope".into()),
            compile_program(&next_source, Some(&module)),
        )
        .unwrap();
    let current = candidate.module().clone();
    vm.publish_staged_reload(candidate).unwrap();
    let _current_program = _old_program.clone();
    let runner = vm
        .execute_module(&current, "runner", vm.execution_options())
        .unwrap()
        .return_value;
    let method = current
        .bytecode
        .interface_tables
        .iter()
        .flat_map(|table| &table.methods)
        .find(|method| {
            current
                .definitions()
                .resolve(method.method)
                .unwrap()
                .segments()
                .last()
                .unwrap()
                .name
                == "run"
        })
        .unwrap()
        .method;
    assert_eq!(
        vm.invoke_interface_method(&runner, &method, std::slice::from_ref(&receiver))
            .unwrap()
            .as_i32(),
        Some(43)
    );
    assert!(vm.validate_executable_module(&old).is_ok());
    drop(receiver);
    // Retire the linked construction cache as well as its old receiver value.
    let candidate = vm
        .stage_reload_program(
            &_old_program,
            None,
            compile_program(&next_source, Some(&module)),
        )
        .unwrap();
    vm.publish_staged_reload(candidate).unwrap();
    vm.collect_garbage().unwrap();
    assert!(vm.validate_executable_module(&old).is_err());
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn generic_native_default_calls_the_selected_receiver_operation() {
    let mut module = ModuleBuilder::new(
        "example::receiver",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = module.define_trait("Read").unwrap();
    let read = declaration
        .define_method(MethodDecl::instance("read").returns(Type::i32()))
        .unwrap();
    let echo = declaration
        .define_method(MethodDecl::instance("echo"))
        .unwrap();
    let operation = declaration.operation(&read).unwrap();
    let selected = declaration
        .method(&echo, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone())?;
            method.returns(value)?;
            method.requires(operation)
        })
        .unwrap();
    declaration
        .bind(
            echo,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                let receiver = cx.argument(0)?;
                let target = cx.selected(&selected)?;
                if cx.call_values(target, &[receiver])?.as_i32() != Some(42) {
                    return Err(RuntimeError::module_validation(
                        "selected receiver returned wrong value",
                    ));
                }
                cx.argument(1)
            }),
        )
        .unwrap();
    declaration.finish().unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/interface-selected-receiver.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}

#[test]
fn a_shared_native_helper_calls_a_generic_constraint_member() {
    let mut module = ModuleBuilder::new(
        "example::generic_operation",
        &StandardDeclarations::default()
            .catalog()
            .expect("explicit standard providers"),
    )
    .unwrap();
    let mut declaration = module.define_trait("Identity").unwrap();
    let method = declaration
        .define_method(MethodDecl::instance("identity"))
        .unwrap();
    declaration
        .method(&method, |method| {
            let value = method.type_parameter("T")?.ty();
            method.parameter("value", value.clone())?;
            method.returns(value)?;
            Ok(())
        })
        .unwrap();
    let contract = declaration.finish().unwrap();
    let invoke = module.define_function(FunctionDecl::new("invoke")).unwrap();
    let selected = module
        .function(&invoke, |function| {
            let source = function.type_parameter("S")?.ty();
            let value = function.type_parameter("T")?.ty();
            function.parameter("source", source.clone())?;
            function.parameter("value", value.clone())?;
            function.returns(value.clone())?;
            function.bound(source.clone(), contract.apply([])?)?;
            function.requires(
                CallableRequirement::method(source, contract.method("identity")?)?
                    .arguments([value])?,
            )
        })
        .unwrap();
    module
        .bind(
            invoke,
            NativeBinding::new(vec![Codec::Value, Codec::Value], Codec::Value, move |cx| {
                let arguments = [cx.argument(0)?, cx.argument(1)?];
                cx.call_values(cx.selected(&selected)?, &arguments)
            }),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let (mut vm, loaded) = compile(
        include_str!("../../../../tests/fixtures/native/shared-generic-constraint.kgr"),
        Some(&module),
    );
    let _loaded_program = loaded;
    let loaded = vm.select_program(&_loaded_program).unwrap();
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .as_i32(),
        Some(42)
    );
}
