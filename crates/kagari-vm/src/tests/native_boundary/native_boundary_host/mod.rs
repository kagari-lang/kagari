use super::compile_program;
use crate::vm::Vm;
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{HostCallContext, HostObjectId, HostSchemaEpoch},
    metadata::TypeId,
    module::{ExecutableModule, entry::ProgramEntry},
    native::{
        builder::ModuleBuilder, conversion::context::ConversionContext, declarations::FunctionDecl,
        host::HostCallback, module::NativeModule, typed::NativeContext, types::Type,
    },
    value::Value,
};
use kagari_stdlib::{
    collections::{map::ScriptMap, set::ScriptSet},
    identity, namespaces,
};
use kagari_types::{
    declaration::{
        host::{
            HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
            HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
    },
    language::binding,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

// Reuse the runtime contract owners without another invocation or ABI implementation.
#[path = "../../../../../kagari-runtime/tests/support/native/invoke.rs"]
mod native_invocation;
use crate::tests::native_program::{self, ProgramFixture};

fn host_policy(parameters: usize) -> HostCallPolicy {
    HostCallPolicy {
        passing: vec![HostPassingStyle::Owned; parameters],
        effects: HostFunctionEffects {
            may_allocate: true,
            ..Default::default()
        },
    }
}

fn native_slot(
    runtime: &Runtime,
    owner: &ExecutableModule,
    provider: &NativeModule,
    name: &str,
) -> usize {
    let query = super::support::declaration_query(provider, DefinitionKind::Function, name);
    let declaration = runtime.host().definitions().lookup(&query).unwrap();
    owner
        .bytecode
        .native_imports
        .iter()
        .position(|import| import.instance.declaration == declaration)
        .unwrap()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::default());
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn allocation_owner(runtime: &mut Runtime) -> ExecutableModule {
    let program = runtime
        .load_program(
            "allocation-owner",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(
                DefinitionContext::new().unwrap().snapshot(),
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    runtime.select_program(&program).unwrap()
}

#[test]
fn nested_arguments_and_results_obey_the_complete_host_signature() {
    let mut runtime = runtime();
    let fixture_source = r#"use std::collections::{HashMap, HashSet, List};

        pub fn fixtures() -> ([i32], [bool], HashMap<String,[i32]>, HashMap<String,[bool]>, HashSet<String>, HashSet<i32>, Vec<i32>, List<i32>) {
            val array = [7]; val wrong_array = [true];
            val map: HashMap<String,[i32]> = HashMap::new(); map.insert("k", array);
            val wrong_map: HashMap<String,[bool]> = HashMap::new(); wrong_map.insert("k", wrong_array);
            val set: HashSet<String> = HashSet::new(); set.insert("ok");
            val wrong_set: HashSet<i32> = HashSet::new(); wrong_set.insert(7);
            val vector = Vec::from(array);
            val list: List<i32> = vector;
            (array, wrong_array, map, wrong_map, set, wrong_set, vector, list)
        }
    "#;
    let program = compile_program(fixture_source, None);
    let loaded = runtime.load_program("host-composites", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = Vm::new(runtime);
    let fixture_root = vm.execute(&loaded, "fixtures").unwrap().return_value;
    // SAFETY: fixture_root retains every input across the collecting host calls below.
    let Value::Tuple(values) = unsafe { fixture_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("fixtures")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [
        array,
        wrong_array,
        map,
        wrong_map,
        set,
        wrong_set,
        vector,
        list,
    ] = values.as_slice()
    else {
        panic!("eight fixtures")
    };
    let runtime = vm.runtime_mut();
    let definitions = runtime.host().definitions();
    let enumeration = |ty, member, fields| {
        let applied = runtime
            .resolve_type_arguments(&loaded, &[ty])
            .unwrap()
            .remove(0);
        unsafe { runtime.make_enum_member_transport(&loaded, &applied, member, fields) }.unwrap()
    };
    let some = |value| {
        let ty = match value {
            Value::Bool(_) => BuiltinType::Bool,
            _ => BuiltinType::I32,
        };
        enumeration(
            binding::option(&definitions, Ty::Builtin(ty)).unwrap(),
            "Some",
            vec![value],
        )
    };
    let result = |member, value| {
        let error = if member == "Ok" || matches!(value, Value::Str(_)) {
            BuiltinType::String
        } else {
            BuiltinType::I32
        };
        enumeration(
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(error),
            )
            .unwrap(),
            member,
            vec![value],
        )
    };
    let cases = [
        (
            Ty::Tuple(vec![
                Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                Ty::Builtin(BuiltinType::String),
            ]),
            unsafe {
                runtime.gc().alloc_tuple_transport(vec![
                    *array,
                    runtime.gc().alloc_string_transport("ok".into()).unwrap(),
                ])
            }
            .unwrap(),
            unsafe {
                runtime.gc().alloc_tuple_transport(vec![
                    *wrong_array,
                    runtime.gc().alloc_string_transport("ok".into()).unwrap(),
                ])
            }
            .unwrap(),
        ),
        (
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            *array,
            *wrong_array,
        ),
        (
            Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
            *array,
            *vector,
        ),
        (
            Ty::NativeObject(NominalTy {
                declaration: ModuleDecl::new(namespaces::type_owner("Vec"))
                    .definition(DefinitionKind::AssociatedType, "Vec", &definitions)
                    .unwrap(),
                arguments: vec![Ty::Builtin(BuiltinType::I32)],
                associated_types: Default::default(),
            }),
            *vector,
            *array,
        ),
        (
            Ty::Trait(NominalTy {
                declaration: definitions.lookup(&identity::trait_id("List")).unwrap(),
                arguments: vec![Ty::Builtin(BuiltinType::I32)],
                associated_types: Default::default(),
            }),
            *list,
            *vector,
        ),
        (
            Ty::NativeObject(NominalTy {
                declaration: definitions
                    .lookup(&identity::type_id(
                        "HashMap",
                        DefinitionKind::AssociatedType,
                    ))
                    .unwrap(),
                arguments: vec![
                    Ty::Builtin(BuiltinType::String),
                    Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
                ],
                associated_types: Default::default(),
            }),
            *map,
            *wrong_map,
        ),
        (
            Ty::NativeObject(NominalTy {
                declaration: definitions
                    .lookup(&identity::type_id(
                        "HashSet",
                        DefinitionKind::AssociatedType,
                    ))
                    .unwrap(),
                arguments: vec![Ty::Builtin(BuiltinType::String)],
                associated_types: Default::default(),
            }),
            *set,
            *wrong_set,
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            some(Value::I32(7)),
            some(Value::Bool(true)),
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
                "None",
                vec![],
            ),
            result("Ok", Value::I32(7)),
        ),
        (
            binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
                "None",
                vec![],
            ),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::Bool)).unwrap(),
                "None",
                vec![],
            ),
        ),
        (
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap(),
            result("Ok", Value::I32(7)),
            result("Err", Value::I32(7)),
        ),
        (
            binding::result(
                &definitions,
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap(),
            result(
                "Err",
                unsafe { runtime.gc().alloc_string_transport("error".into()) }.unwrap(),
            ),
            some(Value::I32(7)),
        ),
    ];
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("fixtures::composites", &catalog).unwrap();
    let mut source = format!("{fixture_source}\nuse fixtures::composites;\n");
    let syntax = [
        "([i32], String)",
        "[i32]",
        "[i32]",
        "Vec<i32>",
        "List<i32>",
        "HashMap<String,[i32]>",
        "HashSet<String>",
        "Option<i32>",
        "Option<i32>",
        "Option<i32>",
        "Result<i32,String>",
        "Result<i32,String>",
    ];
    let mut checks = Vec::new();
    for (index, (ty, good, bad)) in cases.into_iter().enumerate() {
        let ty = Type::from_semantic(ty, &definitions).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let echo = builder
            .define_function(
                FunctionDecl::new(format!("echo{index}"))
                    .parameter("value", ty.clone())
                    .returns(ty.clone())
                    .host_policy(host_policy(1)),
            )
            .unwrap();
        builder
            .bind(echo, unsafe {
                // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
                HostCallback::new(move |_: &HostCallContext<'_>, args: &[Value]| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(args[0])
                })
            })
            .unwrap();
        let called = calls.clone();
        let output = builder
            .define_function(
                FunctionDecl::new(format!("bad{index}"))
                    .returns(ty)
                    .host_policy(host_policy(0)),
            )
            .unwrap();
        builder
            .bind(output, unsafe {
                // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
                HostCallback::new(move |_: &HostCallContext<'_>, _: &[Value]| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(bad)
                })
            })
            .unwrap();
        let ty = syntax[index];
        source.push_str(&format!(
            "fn echo{index}(value: {ty}) -> {ty} {{ composites::echo{index}(value) }} fn bad{index}() -> {ty} {{ composites::bad{index}() }}\n"
        ));
        checks.push((good, bad, calls));
    }
    let provider = builder.finish().unwrap();
    let program = compile_program(&source, Some(&provider));
    provider.install(runtime).unwrap();
    // Values and host signatures share an installation, including nominal
    // collection/enum/interface evidence retained across this additive reload.
    let candidate = runtime
        .stage_reload_program(&_loaded_program, None, program)
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let owner = runtime.select_program(&_loaded_program).unwrap();
    for (index, (good, bad, calls)) in checks.into_iter().enumerate() {
        let echo = native_slot(runtime, &owner, &provider, &format!("echo{index}"));
        let output = native_slot(runtime, &owner, &provider, &format!("bad{index}"));
        assert_eq!(
            native_invocation::invoke(runtime, &owner, echo, &[good])
                .unwrap_or_else(|error| panic!("case {index}: {error:?}")),
            good
        );
        assert!(native_invocation::invoke(runtime, &owner, echo, &[bad]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(native_invocation::invoke(runtime, &owner, output, &[]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn composite_arguments_are_rooted_during_callbacks_and_reject_foreign_or_wrong_kind_values() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut builder = ModuleBuilder::new("fixtures::rooted", &Default::default()).unwrap();
    let ty = Type::tuple([Type::from_semantic(
        Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))),
        &DefinitionContext::new().unwrap().snapshot(),
    )
    .unwrap()])
    .unwrap();
    let echo = builder
        .define_function(
            FunctionDecl::new("echo")
                .parameter("value", ty.clone())
                .returns(ty)
                .host_policy(host_policy(1)),
        )
        .unwrap();
    builder
        .bind(echo, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context: &HostCallContext<'_>, args: &[Value]| {
                called.fetch_add(1, Ordering::SeqCst);
                context.runtime().collect_garbage().unwrap();
                Ok(args[0])
            })
        })
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = native_program::free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let entry = runtime.load_program("rooted-composite", program).unwrap();
    let _entry_program = entry;
    let entry = runtime.select_program(&_entry_program).unwrap();
    let id = native_slot(&runtime, &entry, &provider, "echo");
    let array = unsafe {
        runtime.alloc_array_transport(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
    }
    .unwrap();
    let value = unsafe {
        runtime
            .gc()
            .alloc_tuple_transport(vec![Value::Array(array)])
    }
    .unwrap();
    assert_eq!(
        native_invocation::invoke(&runtime, &entry, id, std::slice::from_ref(&value)).unwrap(),
        value
    );
    assert!(unsafe { runtime.gc().array_snapshot_transport(array) }.is_some());
    let mut other = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut other).unwrap();
    let other_owner = allocation_owner(&mut other);
    let foreign = unsafe {
        other.gc().alloc_tuple_transport(vec![Value::Array(
            other
                .alloc_array_transport(
                    &other_owner,
                    Ty::Builtin(BuiltinType::I32),
                    vec![Value::I32(7)],
                )
                .unwrap(),
        )])
    }
    .unwrap();
    assert!(native_invocation::invoke(&runtime, &entry, id, &[foreign]).is_err());
    assert!(native_invocation::invoke(&runtime, &entry, id, &[Value::Tuple(array)]).is_err());
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    assert_eq!(runtime.gc().allocated_objects(), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn owned_composites_cannot_hide_frame_scoped_host_borrows() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let mut builder = ModuleBuilder::new("fixtures::escape", &Default::default()).unwrap();
    let mut object = builder.define_type("Object").unwrap();
    object
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::None,
        })
        .unwrap();
    let object = object.finish().unwrap();
    let consume = builder
        .define_function(
            FunctionDecl::new("consume")
                .parameter(
                    "value",
                    Type::tuple([Type::tuple([object.apply([]).unwrap()]).unwrap()]).unwrap(),
                )
                .host_policy(host_policy(1)),
        )
        .unwrap();
    builder
        .bind(consume, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|_: &HostCallContext<'_>, _: &[Value]| {
                panic!("an owned parameter must reject nested borrows before the callback")
            })
        })
        .unwrap();
    let provider = builder.finish().unwrap();
    let program = native_program::free_function_program(&provider.to_declaration());
    provider.install(&mut runtime).unwrap();
    let entry = runtime.load_program("owned-composite", program).unwrap();
    let _entry_program = entry;
    let entry = runtime.select_program(&_entry_program).unwrap();
    let id = native_slot(&runtime, &entry, &provider, "consume");
    for unique in [false, true] {
        let scope = unsafe {
            // SAFETY: Empty inputs; this guard owns only host borrow/session leases.
            runtime.host_scope_transport(&[])
        }
        .unwrap();
        let value = if unique {
            unsafe {
                runtime.gc().alloc_host_mut_transport(
                    scope
                        .borrows()
                        .borrow_unique(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
            }
            .unwrap()
        } else {
            unsafe {
                runtime.gc().alloc_host_ref_transport(
                    scope
                        .borrows()
                        .borrow_shared(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
            }
            .unwrap()
        };
        let value = unsafe {
            runtime.gc().alloc_tuple_transport(vec![
                runtime.gc().alloc_tuple_transport(vec![value]).unwrap(),
            ])
        }
        .unwrap();
        assert_eq!(
            native_invocation::invoke(&runtime, &entry, id, std::slice::from_ref(&value))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        assert!(
            unsafe {
                runtime.alloc_array_transport(&owner, Ty::Builtin(BuiltinType::I32), vec![value])
            }
            .is_err()
        );
        drop(scope);
        assert!(native_invocation::invoke(&runtime, &entry, id, &[value]).is_err());
    }
    assert!(!runtime.is_quarantined());
}

#[test]
fn native_hash_payloads_reject_host_roots_and_frame_borrows_before_mutation() {
    let mut runtime = runtime();
    let program = compile_program(
        "use std::collections::{HashMap, HashSet, List};\nfn main() -> (HashMap<i32,i32>, HashSet<i32>) { val map: HashMap<i32,i32> = HashMap::new(); map.insert(1,1); map.clear(); val set: HashSet<i32> = HashSet::new(); set.insert(1); set.clear(); (map, set) }",
        None,
    );
    let loaded = runtime.load_program("host-storage", program).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = Vm::new(runtime);
    let fixture_root = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value;
    // SAFETY: fixture_root retains every input across the collecting host calls below.
    let Value::Tuple(values) = unsafe { fixture_root.value_transport(vm.runtime().gc()) }
        .expect("retained execution result")
    else {
        panic!("containers")
    };
    let values = unsafe { vm.runtime().gc().tuple_transport(values) }
        .unwrap()
        .to_vec();
    let [map @ Value::GcHandle(_), set @ Value::GcHandle(_)] = values.as_slice() else {
        panic!("handles")
    };
    let map = *map;
    let set = *set;
    let map_slot = loaded
        .bytecode
        .native_imports
        .iter()
        .position(|import| import.signature.params.len() == 3)
        .unwrap();
    let set_slot = loaded
        .bytecode
        .native_imports
        .iter()
        .position(|import| import.signature.params.len() == 2)
        .unwrap();
    let runtime = vm.runtime_mut();
    let catalog = kagari_stdlib::declarations::StandardDeclarations::default()
        .catalog()
        .unwrap();
    let mut builder = ModuleBuilder::new("external", &catalog).unwrap();
    let mut external = builder.define_type("External").unwrap();
    external
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadOnly,
        })
        .unwrap();
    external.finish().unwrap();
    builder.finish().unwrap().install(runtime).unwrap();
    let ty = runtime
        .types()
        .get_by_name("external::External")
        .unwrap()
        .id;
    let host = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let scope = unsafe {
        // SAFETY: Empty inputs; this guard owns only host borrow/session leases.
        runtime.host_scope_transport(&[])
    }
    .unwrap();
    let borrowed = unsafe {
        runtime
            .gc()
            .alloc_host_ref_transport(scope.borrows().borrow_shared(HostObjectId(1), ty).unwrap())
    }
    .unwrap();
    let heap = runtime.gc();
    let invalid_values = [
        unsafe { heap.alloc_host_root_transport(host) }.unwrap(),
        borrowed,
    ];
    let before = heap.stats();
    for invalid in invalid_values {
        assert!(
            native_invocation::invoke(runtime, &loaded, map_slot, &[map, Value::I32(1), invalid])
                .is_err()
        );
        assert!(
            native_invocation::invoke(runtime, &loaded, map_slot, &[map, invalid, Value::I32(1)])
                .is_err()
        );
        assert!(native_invocation::invoke(runtime, &loaded, set_slot, &[set, invalid]).is_err());
        assert_eq!(heap.stats(), before);
        let mut conversion = ConversionContext::new(runtime, &loaded).unwrap();
        let map = conversion
            .decode::<ScriptMap<i32, i32>>(
                &unsafe { runtime.gc().root_value_transport(map) }.unwrap(),
            )
            .unwrap();
        let set = conversion
            .decode::<ScriptSet<i32>>(&unsafe { runtime.gc().root_value_transport(set) }.unwrap())
            .unwrap();
        let cx = NativeContext::new(runtime, &loaded).unwrap();
        assert_eq!(map.len(&cx).unwrap(), 0);
        assert_eq!(set.len(&cx).unwrap(), 0);
    }
}
