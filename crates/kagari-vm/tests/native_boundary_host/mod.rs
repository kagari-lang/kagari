use super::compile_program;
use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata, table::DefinitionTable,
};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{HostFunction, HostObjectId, HostSchemaEpoch},
    metadata::TypeId,
    module::{LoadedModule, entry::ProgramEntry},
    native::{builder::ModuleBuilder, module::NativeModule},
    value::Value,
};
use kagari_stdlib::{identity, namespaces};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        host::{
            HostPassingStyle, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        module::ModuleDecl,
    },
    host_interface::{
        HostFunctionDeclaration, HostParameter, type_declaration::HostTypeDeclaration,
        value_type::HostValueType as Type,
    },
    language::binding,
    scalar::BuiltinType,
    ty::Ty,
};
use kagari_vm::vm::Vm;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig::default());
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn allocation_owner(runtime: &mut Runtime) -> LoadedModule {
    runtime
        .load_program(
            "allocation-owner",
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                BytecodeProgram {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap()
}

fn echo(
    name: &str,
    ty: Type,
    definitions: &DefinitionTable,
) -> DefinitionMetadata<HostFunctionDeclaration> {
    signature(
        name,
        vec![HostParameter {
            name: "value".into(),
            ty: ty.clone(),
            passing: HostPassingStyle::Owned,
        }],
        ty,
        definitions,
    )
}

#[test]
fn nested_arguments_and_results_obey_the_complete_host_signature() {
    let mut runtime = runtime();
    let program = compile_program(
        r#"use std::collections::{HashMap, HashSet, List};

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
    "#,
        None,
    );
    let loaded = runtime.load_program("host-composites", program).unwrap();
    let mut vm = Vm::new(runtime);
    let Value::Tuple(values) = vm
        .execute(&loaded, "fixtures")
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("fixtures")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
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
        runtime
            .make_enum_member(&loaded, &applied, member, fields)
            .unwrap()
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
            Type::Tuple(vec![Type::Array(Box::new(Type::I32)), Type::String]),
            runtime
                .gc()
                .alloc_tuple(vec![
                    *array,
                    runtime.gc().alloc_string("ok".into()).unwrap(),
                ])
                .unwrap(),
            runtime
                .gc()
                .alloc_tuple(vec![
                    *wrong_array,
                    runtime.gc().alloc_string("ok".into()).unwrap(),
                ])
                .unwrap(),
        ),
        (Type::Array(Box::new(Type::I32)), *array, *wrong_array),
        (Type::Array(Box::new(Type::I32)), *array, *vector),
        (
            Type::Vec(
                ModuleDecl::new(namespaces::type_owner("Vec"))
                    .definition(DefinitionKind::AssociatedType, "Vec", &definitions)
                    .unwrap(),
                Box::new(Type::I32),
            ),
            *vector,
            *array,
        ),
        (
            Type::List(
                definitions.lookup(&identity::trait_id("List")).unwrap(),
                Box::new(Type::I32),
            ),
            *list,
            *vector,
        ),
        (
            Type::Map {
                access: CollectionAccess::Mutable,
                key: Box::new(Type::String),
                value: Box::new(Type::Array(Box::new(Type::I32))),
            },
            *map,
            *wrong_map,
        ),
        (
            Type::Set(Box::new(Type::String), CollectionAccess::Mutable),
            *set,
            *wrong_set,
        ),
        (
            Type::Option(
                definitions.lookup(&binding::option_declaration()).unwrap(),
                Box::new(Type::I32),
            ),
            some(Value::I32(7)),
            some(Value::Bool(true)),
        ),
        (
            Type::Option(
                definitions.lookup(&binding::option_declaration()).unwrap(),
                Box::new(Type::I32),
            ),
            enumeration(
                binding::option(&definitions, Ty::Builtin(BuiltinType::I32)).unwrap(),
                "None",
                vec![],
            ),
            result("Ok", Value::I32(7)),
        ),
        (
            Type::Option(
                definitions.lookup(&binding::option_declaration()).unwrap(),
                Box::new(Type::I32),
            ),
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
            Type::Result {
                declaration: definitions.lookup(&binding::result_declaration()).unwrap(),
                ok: Box::new(Type::I32),
                error: Box::new(Type::String),
            },
            result("Ok", Value::I32(7)),
            result("Err", Value::I32(7)),
        ),
        (
            Type::Result {
                declaration: definitions.lookup(&binding::result_declaration()).unwrap(),
                ok: Box::new(Type::I32),
                error: Box::new(Type::String),
            },
            result("Err", runtime.gc().alloc_string("error".into()).unwrap()),
            some(Value::I32(7)),
        ),
    ];
    for (index, (ty, good, bad)) in cases.into_iter().enumerate() {
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let id = runtime
            .register_host_function(HostFunction::new(
                echo(&format!("host.echo{index}"), ty.clone(), &definitions),
                move |_, args| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(args[0])
                },
            ))
            .unwrap();
        assert_eq!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&good))
                .unwrap(),
            good
        );
        assert!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&bad))
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let called = calls.clone();
        let output = runtime
            .register_host_function(HostFunction::new(
                signature(format!("host.bad{index}"), vec![], ty, &definitions),
                move |_, _| {
                    called.fetch_add(1, Ordering::SeqCst);
                    Ok(bad)
                },
            ))
            .unwrap();
        assert!(runtime.invoke_bound_host(output, &[]).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn composite_arguments_are_rooted_during_callbacks_and_reject_foreign_or_stale_handles() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let id = runtime
        .register_host_function(HostFunction::new(
            echo(
                "host.echo",
                Type::Tuple(vec![Type::Array(Box::new(Type::I32))]),
                &runtime.host().definitions(),
            ),
            move |context, args| {
                called.fetch_add(1, Ordering::SeqCst);
                context.runtime().collect_garbage().unwrap();
                Ok(args[0])
            },
        ))
        .unwrap();
    let array = runtime
        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
        .unwrap();
    let value = runtime.gc().alloc_tuple(vec![Value::Array(array)]).unwrap();
    assert_eq!(
        runtime
            .invoke_bound_host(id, std::slice::from_ref(&value))
            .unwrap(),
        value
    );
    assert!(runtime.gc().array_snapshot(array).is_some());
    let mut other = Runtime::default();
    NativeModule::install_all(&kagari_stdlib::modules().unwrap(), &mut other).unwrap();
    let other_owner = allocation_owner(&mut other);
    let foreign = other
        .gc()
        .alloc_tuple(vec![Value::Array(
            other
                .alloc_array(
                    &other_owner,
                    Ty::Builtin(BuiltinType::I32),
                    vec![Value::I32(7)],
                )
                .unwrap(),
        )])
        .unwrap();
    assert!(runtime.invoke_bound_host(id, &[foreign]).is_err());
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().array_snapshot(array).is_none());
    assert!(runtime.invoke_bound_host(id, &[value]).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn owned_composites_cannot_hide_frame_scoped_host_borrows() {
    let mut runtime = runtime();
    let owner = allocation_owner(&mut runtime);
    let context = DefinitionContext::new().unwrap();
    let object = HostTypeDeclaration::new(&context, "host.Object")
        .unwrap()
        .id;
    let definitions = context.snapshot();
    let id = runtime
        .register_host_function(HostFunction::new(
            signature(
                "host.consume",
                vec![HostParameter {
                    name: "value".into(),
                    ty: Type::Tuple(vec![Type::Tuple(vec![Type::Opaque(object)])]),
                    passing: HostPassingStyle::Owned,
                }],
                Type::Unit,
                &definitions,
            ),
            |_, _| panic!("an owned parameter must reject nested borrows before the callback"),
        ))
        .unwrap();
    for unique in [false, true] {
        let scope = runtime.host_scope(&[]).unwrap();
        let value = if unique {
            runtime
                .gc()
                .alloc_host_mut(
                    scope
                        .borrows()
                        .borrow_unique(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
                .unwrap()
        } else {
            runtime
                .gc()
                .alloc_host_ref(
                    scope
                        .borrows()
                        .borrow_shared(HostObjectId(1), TypeId::new(0))
                        .unwrap(),
                )
                .unwrap()
        };
        let value = runtime
            .gc()
            .alloc_tuple(vec![runtime.gc().alloc_tuple(vec![value]).unwrap()])
            .unwrap();
        assert_eq!(
            runtime
                .invoke_bound_host(id, std::slice::from_ref(&value))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::HostBorrowEscape
        );
        assert!(
            runtime
                .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![value])
                .is_err()
        );
        drop(scope);
        assert!(runtime.invoke_bound_host(id, &[value]).is_err());
    }
    assert!(!runtime.is_quarantined());
}

#[test]
fn native_hash_payloads_reject_host_roots_and_frame_borrows_before_mutation() {
    let mut runtime = runtime();
    let program = compile_program(
        "use std::collections::{HashMap, HashSet, List};\nfn main() -> (HashMap<i32,i32>, HashSet<i32>) { (HashMap::new(), HashSet::new()) }",
        None,
    );
    let loaded = runtime.load_program("host-storage", program).unwrap();
    let mut vm = Vm::new(runtime);
    let Value::Tuple(values) = vm
        .execute(&loaded, ProgramEntry::Main)
        .unwrap()
        .return_value
        .value(vm.runtime().gc())
        .expect("retained execution result")
    else {
        panic!("containers")
    };
    let values = vm.runtime().gc().tuple(values).unwrap().to_vec();
    let [Value::Map(map), Value::Set(set)] = values.as_slice() else {
        panic!("handles")
    };
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
    let scope = runtime.host_scope(&[]).unwrap();
    let borrowed = runtime
        .gc()
        .alloc_host_ref(scope.borrows().borrow_shared(HostObjectId(1), ty).unwrap())
        .unwrap();
    let heap = runtime.gc();
    let invalid_values = [heap.alloc_host_root(host).unwrap(), borrowed];
    let before = heap.stats();
    for invalid in invalid_values {
        assert!(heap.map_insert(*map, Value::I32(1), invalid).is_err());
        assert!(heap.map_insert(*map, invalid, Value::I32(1)).is_err());
        assert!(heap.set_insert(*set, invalid).is_err());
        assert_eq!(heap.stats(), before);
        assert_eq!(heap.map_len(*map), Some(0));
        assert_eq!(heap.set_len(*set), Some(0));
    }
}

fn signature(
    name: impl Into<String>,
    parameters: Vec<HostParameter>,
    result: Type,
    definitions: &DefinitionTable,
) -> DefinitionMetadata<HostFunctionDeclaration> {
    let context = DefinitionContext::new().unwrap();
    let parameters =
        DefinitionMetadata::checked(definitions.clone(), parameters, &Default::default())
            .unwrap()
            .import_into(&context, &Default::default())
            .unwrap()
            .into_records();
    let result = DefinitionMetadata::checked(definitions.clone(), result, &Default::default())
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let declaration = HostFunctionDeclaration::new(&context, name, parameters, result).unwrap();
    DefinitionMetadata::checked(context.snapshot(), declaration, &Default::default()).unwrap()
}
