#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    execution::roots::ExecutionRoots,
    execution_metadata::{
        application_key::{
            ApplicationArguments, ApplicationKey, InterfaceMethodIdentity, MethodIdentity,
        },
        applications::{ApplicationRecord, MethodApplication},
        managed::sources::MetadataSource,
    },
    frame::types::construction::ConstructionScope,
    module::{ExecutableModule, InstalledProgram, managed::source::RevisionSource},
    objects::interfaces::InterfaceRequest,
    value::{carriers::RootedValue, interfaces::InterfaceObject},
    value_semantics::identity_equal,
};
use kagari_abi::representation::ValueType;
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::types::PublicItem;
use kagari_gc::{access::Scope, error::GcError, object::Gc};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{callable::Signature, scalar::BuiltinType, ty::Ty};
use std::sync::Arc;

fn load_source(source: &str) -> (Runtime, InstalledProgram, ExecutableModule) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("interfaces.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap();
    let mut runtime = Runtime::default();
    let loaded_program = load_bytecode(&mut runtime, "interfaces", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    (runtime, loaded_program, loaded)
}

fn fixture() -> (
    Runtime,
    InstalledProgram,
    ExecutableModule,
    usize,
    RootedValue,
) {
    let (mut runtime, loaded_program, loaded) = load_source(include_str!(
        "../../../tests/fixtures/metadata/interface-parents.kgr"
    ));
    let table = loaded
        .bytecode
        .interface_tables
        .iter()
        .position(|table| {
            table
                .methods
                .iter()
                .any(|method| loaded.definition_name(method.method) == Some("second"))
        })
        .unwrap();
    let value = runtime
        .make_interface(&loaded, table, &RootedValue::from(7_i32))
        .unwrap();
    (runtime, loaded_program, loaded, table, value)
}

#[test]
fn boxed_scalar_does_not_acquire_object_identity() {
    let (mut runtime, _program, _, _, value) = fixture();
    assert_eq!(
        identity_equal(&mut runtime, &value, &value)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ScriptTrap
    );
}

#[test]
fn rejected_parent_publication_keeps_the_cache_empty_until_valid_upcast() {
    let (mut runtime, _loaded_program, loaded, table, value) = fixture();
    let roots = runtime.gc_stats().root_registrations;
    let (source, target, parent_table) = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let value = object(cx, &value)?;
            cx.read(value, |value| {
                Ok((
                    value.description().interface_type().clone(),
                    value.description().parents[0].interface.clone(),
                    value.description().parents[0].binding.table,
                ))
            })
        })
        .unwrap();
    let wrong = runtime
        .make_interface(&loaded, table, &RootedValue::from(8_i32))
        .unwrap();
    let wrong_parent = runtime.upcast_interface(&wrong, &source, &target).unwrap();
    let (foreign, _program, _, _, foreign_value) = fixture();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let owner = object(cx, &value)?;
            assert!(matches!(
                object(cx, &foreign_value),
                Err(GcError::ForeignHeap)
            ));
            assert!(InterfaceObject::parent(cx, owner, 0)?.is_none());
            let wrong = object(cx, &wrong_parent)?;
            assert_eq!(
                InterfaceObject::publish_parent(cx, owner, 0, wrong),
                Err(GcError::Representation)
            );
            assert!(InterfaceObject::parent(cx, owner, 0)?.is_none());
            assert_eq!(
                InterfaceObject::publish_parent(cx, owner, usize::MAX, wrong),
                Err(GcError::Representation)
            );
            assert_eq!(
                InterfaceObject::publish_parent(cx, owner, 0, owner),
                Err(GcError::Representation)
            );
            assert!(InterfaceObject::parent(cx, owner, 0)?.is_none());
            Ok(())
        })
        .unwrap();
    let parent = runtime
        .make_interface(&loaded, parent_table, &RootedValue::from(7_i32))
        .unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let owner = object(cx, &value)?;
            let parent = object(cx, &parent)?;
            InterfaceObject::publish_parent(cx, owner, 0, parent)?;
            assert_eq!(
                InterfaceObject::publish_parent(cx, owner, 0, parent),
                Err(GcError::Representation)
            );
            assert!(
                InterfaceObject::parent(cx, owner, 0)?
                    .unwrap()
                    .ptr_eq(parent)
            );
            Ok(())
        })
        .unwrap();
    let reused = runtime.upcast_interface(&value, &source, &target).unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            assert!(object(cx, &reused)?.ptr_eq(object(cx, &parent)?));
            Ok(())
        })
        .unwrap();
    drop(reused);
    drop(parent);
    drop(wrong_parent);
    drop(wrong);
    runtime.collect_garbage().unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let owner = object(cx, &value)?;
            let parent = InterfaceObject::parent(cx, owner, 0)?.unwrap();
            assert_eq!(
                InterfaceObject::data(cx, parent)?.scalar_payload(),
                Some((ValueType::I32, 7))
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    drop(value);
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots - 1);
    drop(foreign_value);
    drop(foreign);
}

#[test]
fn application_publication_checks_actual_sources_without_retaining_receivers() {
    let (mut runtime, program, loaded, table, value) = fixture();
    let description = runtime.interface_description(&value).unwrap();
    let weak = Arc::downgrade(&description);
    let key = ApplicationKey::new(
        MethodIdentity::Interface(Arc::new(InterfaceMethodIdentity {
            owner: loaded.key(),
            table,
            slot: 0,
            interface: description.interface_expression.clone(),
            arguments: description.receiver_table.arguments.clone(),
            environment: None,
            result_adapter: None,
        })),
        &ApplicationArguments::new(&loaded, vec![], ConstructionScope::new(&loaded)).unwrap(),
        None,
        &Default::default(),
    );
    drop(description);
    let application = || {
        ApplicationRecord::Semantic(MethodApplication {
            signature: Signature {
                params: vec![],
                result: Ty::Tuple(vec![]),
            },
            scoped_signature: None,
            environment: None,
            result_adapter: None,
        })
    };
    let retired = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let stale = runtime
        .alloc_application(retired, application(), &[])
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, retired)
        .unwrap();
    runtime.collect_garbage().unwrap();
    let mut foreign = Runtime::default();
    let foreign_source = foreign.memory.enter_execution(&foreign.resources).unwrap();
    let foreign_application = foreign
        .alloc_application(foreign_source, application(), &[])
        .unwrap();
    for invalid in [&stale, &foreign_application] {
        let error = runtime
            .publish_method_application(
                &loaded,
                key.clone(),
                invalid,
                &[MetadataSource::AdmittedApplication(invalid)],
            )
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    }
    assert_eq!(
        foreign
            .publish_method_application(
                &loaded,
                key.clone(),
                &foreign_application,
                &[MetadataSource::AdmittedApplication(&foreign_application)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    foreign
        .memory
        .retire_execution(&foreign.resources, foreign_source)
        .unwrap();
    assert!(runtime.modules.method_application(&loaded, &key).is_none());
    let baseline = runtime.gc_stats().objects;
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let prepared = runtime
        .alloc_application(source, application(), &[])
        .unwrap();
    let unused = runtime
        .alloc_application(source, application(), &[])
        .unwrap();
    let protected = [
        MetadataSource::AdmittedApplication(&prepared),
        MetadataSource::AdmittedApplication(&unused),
    ];
    runtime
        .publish_method_application(&loaded, key.clone(), &prepared, &protected)
        .unwrap();
    assert_eq!(
        runtime
            .publish_method_application(&loaded, key.clone(), &unused, &protected)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.modules.method_application(&loaded, &key),
        Some(prepared.id())
    );
    let root = runtime.retain_application(&prepared).unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().objects,
        baseline + 1,
        "only the explicitly rooted application survives"
    );
    assert_eq!(
        runtime.application_record(&prepared).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .application_record_in(&[MetadataSource::HostApplication(&root)], prepared.id())
        .unwrap();
    drop(value);
    runtime.collect_garbage().unwrap();
    assert!(
        weak.upgrade().is_none(),
        "pure application facts and the optional cache do not retain a receiver description"
    );
    runtime
        .application_record_in(&[MetadataSource::HostApplication(&root)], prepared.id())
        .unwrap();
    drop(root);
    runtime.collect_garbage().unwrap();
    let source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    assert!(
        runtime
            .copy_application_in(source, prepared.id(), &[])
            .unwrap()
            .is_none()
    );
    let next = runtime
        .alloc_application(source, application(), &[])
        .unwrap();
    runtime
        .publish_method_application(
            &loaded,
            key.clone(),
            &next,
            &[MetadataSource::AdmittedApplication(&next)],
        )
        .unwrap();
    let staged = runtime
        .stage_reload_verified_program(&program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    runtime.collect_garbage().unwrap();
    assert!(runtime.modules().loaded(loaded.key()).is_none());
    assert_eq!(
        runtime
            .publish_method_application(
                &loaded,
                key,
                &next,
                &[MetadataSource::AdmittedApplication(&next)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, source)
        .unwrap();
    assert!(!runtime.is_quarantined());
    assert!(!foreign.is_quarantined());
}

#[test]
fn receiver_dictionary_publication_keeps_actual_edges_until_last_interface_root() {
    let (mut runtime, _program, _loaded, _table, value) = fixture();
    let description = runtime.interface_description(&value).unwrap();
    let parent = runtime
        .upcast_interface(
            &value,
            description.interface_type(),
            &description.parents[0].interface,
        )
        .unwrap();
    let other_description = runtime.interface_description(&parent).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let execution = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let dictionary = runtime
        .alloc_operation_group(execution, vec![], &[])
        .unwrap();
    let record = dictionary.record().clone();
    runtime
        .memory
        .scope(&runtime.resources, |cx, source| {
            ExecutionRoots::with(source, cx, |slots| {
                let owner = object(slots.scope(), &value)?;
                let dictionary = dictionary.resolve(slots)?;
                assert!(InterfaceObject::receiver_operations(slots.scope(), owner).is_none());
                assert_eq!(
                    InterfaceObject::publish_receiver_operations(
                        slots.scope(),
                        owner,
                        &other_description,
                        dictionary
                    ),
                    Err(GcError::Representation)
                );
                assert!(InterfaceObject::receiver_operations(slots.scope(), owner).is_none());
                InterfaceObject::publish_receiver_operations(
                    slots.scope(),
                    owner,
                    &description,
                    dictionary,
                )?;
                assert_eq!(
                    InterfaceObject::publish_receiver_operations(
                        slots.scope(),
                        owner,
                        &description,
                        dictionary
                    ),
                    Err(GcError::Representation)
                );
                Ok(())
            })
        })
        .unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, execution)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime
            .operation_group_record(&dictionary)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    // The interface's fixed metadata graph admits the same actual dictionary;
    // keeping its pure Arc record does not keep a managed source alive.
    let next = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let copied = runtime
        .operation_group_in(
            next,
            record.id(),
            &[MetadataSource::RootedInterface(&value)],
        )
        .unwrap();
    assert!(Arc::ptr_eq(copied.record(), &record));
    let count = runtime.gc_stats().objects;
    assert!(
        runtime
            .interface_receiver_operations(
                next,
                &description,
                0,
                &[MetadataSource::RootedInterface(&value)]
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(runtime.gc_stats().objects, count);
    runtime
        .memory
        .retire_execution(&runtime.resources, next)
        .unwrap();
    drop(value);
    runtime.collect_garbage().unwrap();
    let destination = runtime.memory.enter_execution(&runtime.resources).unwrap();
    assert_eq!(
        runtime
            .operation_group_in(
                destination,
                record.id(),
                &[MetadataSource::RootedInterface(&parent)]
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    runtime
        .memory
        .retire_execution(&runtime.resources, destination)
        .unwrap();
    assert!(!runtime.is_quarantined());
}

#[test]
fn parent_cache_and_prepared_method_keep_views_until_last_root() {
    let (mut runtime, program, loaded, _table, value) = fixture();
    runtime.collect_garbage().unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations - 1;
    let description = runtime.interface_description(&value).unwrap();
    let source = description.interface_type().clone();
    let target = description.parents[0].interface.clone();
    let weak_child = Arc::downgrade(&description);
    drop(description);
    let parent = runtime.upcast_interface(&value, &source, &target).unwrap();
    let weak_parent = Arc::downgrade(&runtime.interface_description(&parent).unwrap());
    let count = runtime.gc_stats().objects;
    for _ in 0..8 {
        let next = runtime.upcast_interface(&value, &source, &target).unwrap();
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                assert!(object(cx, &parent)?.ptr_eq(object(cx, &next)?));
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(runtime.gc_stats().objects, count);
    let inherited = runtime.interface_description(&parent).unwrap().methods[0]
        .as_ref()
        .unwrap()
        .method;
    for _ in 0..8 {
        assert_eq!(
            runtime
                .invoke_interface_method(&value, &inherited, &[])
                .unwrap()
                .as_i32(),
            Some(7)
        );
        let next = runtime.upcast_interface(&value, &source, &target).unwrap();
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                assert!(object(cx, &parent)?.ptr_eq(object(cx, &next)?));
                Ok(())
            })
            .unwrap();
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, count);

    let method = runtime
        .resolve_interface_method_slot(&value, &target, 0, &[])
        .unwrap();
    assert_eq!(method.receiver(&mut runtime).unwrap().as_i32(), Some(7));
    assert_eq!(
        method.parameter_types(&mut runtime).unwrap(),
        vec![Ty::Builtin(BuiltinType::I32)]
    );
    let admitted = method.admit(&mut runtime).unwrap();
    let computation = admitted.invocation.computation().unwrap().id();
    let retained = runtime
        .application_record_in(&admitted.sources().unwrap(), computation)
        .unwrap();
    drop((value, parent));
    runtime.collect_garbage().unwrap();
    assert!(weak_child.upgrade().is_none());
    assert!(weak_parent.upgrade().is_some());
    let candidate = runtime
        .stage_reload_verified_program(&program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(candidate).unwrap();
    let latest = runtime.select_program(&program).unwrap();
    assert_eq!(
        method.implementation(&mut runtime).unwrap().key(),
        latest.key()
    );
    assert_eq!(
        admitted.implementation(&mut runtime).unwrap().key(),
        loaded.key()
    );
    assert_eq!(admitted.receiver(&mut runtime).unwrap().as_i32(), Some(7));
    let mut foreign = Runtime::default();
    assert_eq!(
        method.receiver(&mut foreign).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(!foreign.is_quarantined());
    drop(admitted);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime
            .application_record_in(&method.sources().unwrap(), computation)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        method.return_type(&mut runtime).unwrap(),
        Ty::Builtin(BuiltinType::I32)
    );
    // The detached computation facts are no longer execution authority.
    assert!(matches!(retained.as_ref(), ApplicationRecord::Admitted(_)));
    // Detached immutable facts can keep the description Arc readable without
    // retaining the managed parent or authorizing the retired computation.
    drop((method, retained));
    runtime.collect_garbage().unwrap();
    assert!(weak_parent.upgrade().is_none());
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
    assert!(!runtime.is_quarantined());
}

fn object<'gc>(cx: &Scope<'gc>, root: &RootedValue) -> Result<Gc<'gc, InterfaceObject>, GcError> {
    let value = root.get(cx)?;
    cx.downcast(value.object().ok_or(GcError::Representation)?)
}

#[test]
fn shared_descriptions_keep_receiver_views_separate_and_retire_after_last_root() {
    let (mut runtime, _loaded_program, loaded, table, first) = fixture();
    let roots = runtime.gc_stats().root_registrations;
    let second = runtime
        .make_interface(&loaded, table, &RootedValue::from(23_i32))
        .unwrap();
    let (description, source, target) = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let first = object(cx, &first)?;
            let second = object(cx, &second)?;
            assert!(!first.ptr_eq(second));
            cx.read(first, |first| {
                cx.read(second, |second| {
                    assert!(Arc::ptr_eq(first.description(), second.description()));
                    Ok((
                        Arc::downgrade(first.description()),
                        first.description().interface_type().clone(),
                        first.description().parents[0].interface.clone(),
                    ))
                })
            })
        })
        .unwrap();
    let first_parent = runtime.upcast_interface(&first, &source, &target).unwrap();
    let second_parent = runtime.upcast_interface(&second, &source, &target).unwrap();
    let repeated = runtime.upcast_interface(&first, &source, &target).unwrap();
    let parent_description = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let left = object(cx, &first_parent)?;
            let right = object(cx, &second_parent)?;
            let again = object(cx, &repeated)?;
            assert!(!left.ptr_eq(right));
            assert!(
                left.ptr_eq(again),
                "cache reuse preserves this receiver's existing parent object"
            );
            assert_eq!(
                InterfaceObject::data(cx, left)?.scalar_payload(),
                Some((ValueType::I32, 7))
            );
            assert_eq!(
                InterfaceObject::data(cx, right)?.scalar_payload(),
                Some((ValueType::I32, 23))
            );
            cx.read(left, |left| {
                cx.read(right, |right| {
                    assert!(Arc::ptr_eq(left.description(), right.description()));
                    Ok(Arc::downgrade(left.description()))
                })
            })
        })
        .unwrap();
    drop(repeated);
    drop(first_parent);
    drop(second_parent);
    drop(first);
    runtime.collect_garbage().unwrap();
    assert!(description.upgrade().is_some());
    assert!(parent_description.upgrade().is_some());
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let second = object(cx, &second)?;
            let parent = InterfaceObject::parent(cx, second, 0)?
                .expect("second receiver owns its cached parent");
            assert_eq!(
                InterfaceObject::data(cx, parent)?.scalar_payload(),
                Some((ValueType::I32, 23))
            );
            Ok(())
        })
        .unwrap();
    drop(second);
    runtime.collect_garbage().unwrap();
    assert!(description.upgrade().is_none());
    assert!(parent_description.upgrade().is_none());
    assert_eq!(runtime.gc_stats().root_registrations, roots - 1);
}

#[test]
fn interface_descriptions_do_not_merge_reloaded_implementation_generations() {
    let (mut runtime, loaded_program, loaded, table, old) = fixture();
    let staged = runtime
        .stage_reload_verified_program(&loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    runtime.publish_staged_reload(staged).unwrap();
    let next = runtime.select_program(&loaded_program).unwrap();
    let new = runtime
        .make_interface(&next, table, &RootedValue::from(19_i32))
        .unwrap();
    let old_description = runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let old = object(cx, &old)?;
            let new = object(cx, &new)?;
            cx.read(old, |old| {
                cx.read(new, |new| {
                    assert!(!Arc::ptr_eq(old.description(), new.description()));
                    assert_eq!(old.description().implementation.key(), loaded.key());
                    assert_eq!(new.description().implementation.key(), next.key());
                    Ok(Arc::downgrade(old.description()))
                })
            })
        })
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert!(old_description.upgrade().is_some());
    drop(old);
    runtime.collect_garbage().unwrap();
    assert!(old_description.upgrade().is_none());
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let new = object(cx, &new)?;
            assert_eq!(
                InterfaceObject::data(cx, new)?.scalar_payload(),
                Some((ValueType::I32, 19))
            );
            Ok(())
        })
        .unwrap();
    drop(new);
}

#[test]
fn cached_generic_description_requires_actual_environment_backing() {
    let (mut runtime, _program, owner) = load_source(include_str!(
        "../../../tests/fixtures/metadata/interface-descriptions.kgr"
    ));
    let table = owner
        .bytecode
        .interface_tables
        .iter()
        .position(|linked| {
            owner.bytecode.public_items.iter().any(|item| match item {
                PublicItem::InterfaceTable(table) => {
                    table.declaration == linked.declaration && !table.generic_params.is_empty()
                }
                _ => false,
            })
        })
        .expect("fixture has a checked shared generic interface table");
    let arguments = runtime
        .resolve_type_arguments(&owner, &[Ty::Builtin(BuiltinType::I32)])
        .unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().objects;
    let first_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let revision = MetadataSource::Revision {
        owner: &owner,
        source: RevisionSource::Current,
    };
    let roots = runtime.gc_stats().root_registrations;
    let first = runtime
        .prepare_interface_description(
            first_source,
            InterfaceRequest {
                implementation: &owner,
                table,
                arguments: &arguments,
                environment: None,
                view: false,
            },
            &[revision],
        )
        .unwrap();
    let first_environment = first
        .environment
        .as_ref()
        .expect("shared table has actual binders");
    runtime.validate_environment(first_environment).unwrap();
    runtime
        .memory
        .retire_execution(&runtime.resources, first_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().objects,
        baseline,
        "live cached facts do not root managed binders"
    );
    assert_eq!(
        runtime
            .validate_environment(first_environment)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let next_source = runtime.memory.enter_execution(&runtime.resources).unwrap();
    let next = runtime
        .prepare_interface_description(
            next_source,
            InterfaceRequest {
                implementation: &owner,
                table,
                arguments: &arguments,
                environment: None,
                view: false,
            },
            &[revision],
        )
        .unwrap();
    assert!(
        !Arc::ptr_eq(&first.description, &next.description),
        "pure cached facts cannot recover retired managed binders"
    );
    let environment = next.environment.as_ref().unwrap();
    assert_ne!(first_environment.facts().id, environment.facts().id);
    assert_eq!(
        first.description.concrete_type,
        next.description.concrete_type
    );
    runtime.validate_environment(environment).unwrap();
    let objects = runtime.gc_stats().objects;
    let reused = runtime
        .prepare_interface_description(
            next_source,
            InterfaceRequest {
                implementation: &owner,
                table,
                arguments: &arguments,
                environment: None,
                view: false,
            },
            &[revision, MetadataSource::AdmittedEnvironment(environment)],
        )
        .unwrap();
    assert!(Arc::ptr_eq(&next.description, &reused.description));
    assert_eq!(
        runtime.gc_stats().objects,
        objects,
        "a genuinely protected environment reuses its node"
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    runtime
        .memory
        .retire_execution(&runtime.resources, next_source)
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline);
    assert_eq!(
        runtime
            .validate_environment(reused.environment.as_ref().unwrap())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.gc_stats().root_registrations, roots);
}

#[test]
fn invalid_interface_inspection_preserves_declaration_and_slot_error_kinds() {
    let (mut runtime, _program, _loaded, _table, value) = fixture();
    let description = runtime.interface_description(&value).unwrap();
    let target = description.interface_type().clone();
    let method = description.methods.iter().flatten().next().unwrap().method;
    let error = runtime
        .resolve_interface_method(&RootedValue::from(7i32), &method)
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ModuleValidation);
    let error = runtime
        .resolve_interface_method_slot(&value, &target, usize::MAX, &[])
        .err()
        .unwrap();
    assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
    runtime.collect_garbage().unwrap();
    // A genuine root survives collection; the removed naked-ID API's stale
    // wrapper witness is replaced by current foreign-root admission.
    runtime
        .resolve_interface_method_slot(&value, &target, 0, &[])
        .unwrap();
    let mut foreign = Runtime::default();
    assert_eq!(
        foreign
            .resolve_interface_method(&value, &method)
            .err()
            .unwrap()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(!foreign.is_quarantined());
}
