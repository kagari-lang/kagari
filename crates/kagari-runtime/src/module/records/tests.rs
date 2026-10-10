use super::*;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    module::{LinkedModule, ModuleEpochRetention, ProgramDescriptor, VerifiedProgram},
    native::{
        binding::{BindingEntry, Codec, NativeBinding},
        builder::ModuleBuilder,
        catalog::DeclarationCatalog,
        declarations::FunctionDecl,
        types::Type,
    },
    value::Value,
};
use kagari_bytecode::{
    instruction::{EnumId, StructId},
    program::BytecodeProgram,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};
use std::{slice, thread};

fn fixture() -> (Runtime, LoadedModule, BytecodeProgram) {
    let mut builder = ModuleBuilder::new("test::probe", &DeclarationCatalog::default());
    let function = builder
        .define_function(FunctionDecl::new("read").returns(Type::i32()))
        .unwrap();
    builder
        .bind_with(
            function,
            NativeBinding::new(vec![], Codec::Scalar(Type::i32().abi().clone()), |_| {
                Ok(Value::I32(3))
            }),
        )
        .unwrap();
    let provider = builder.finish().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(vec![Arc::new(provider.to_declaration().unwrap())]);
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "records.kgr",
            r#"
        use test::probe::read;
        struct Holder<T> { val value: T }
        enum Wrapped<T> { Some(T), None }
        trait Build {
            fn make<T>(self, value: T) -> Holder<T> { Holder { value: value } }
            fn wrap<T>(self, value: T) -> Wrapped<T> { Wrapped::Some(value) }
        }
        impl Build for i32 {}
        fn main() -> i32 { val b: Build = 0; val h = b.make(7); val w = b.wrap(h.value); read() }
    "#
            .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    provider.install(&mut runtime).unwrap();
    let loaded = runtime.load_program("records", code.clone()).unwrap();
    (runtime, loaded, code)
}

fn generic_structure(loaded: &LoadedModule) -> StructId {
    StructId::new(
        loaded
            .bytecode
            .structures
            .iter()
            .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
            .unwrap(),
    )
}

fn generic_enum(loaded: &LoadedModule) -> EnumId {
    EnumId::new(
        loaded
            .bytecode
            .enumerations
            .iter()
            .position(|layout| !layout.arguments.iter().all(Ty::is_concrete))
            .unwrap(),
    )
}

fn native_owner(loaded: &LoadedModule) -> LoadedModule {
    loaded
        .members()
        .find(|module| !module.bytecode.native_imports.is_empty())
        .unwrap()
}

fn detached_copy(loaded: &LoadedModule) -> LoadedModule {
    LoadedModule {
        slot: loaded.slot,
        program: Arc::new(ProgramDescriptor {
            code: loaded.program.code.clone(),
            layouts: loaded.program.layouts.clone(),
            layout_admissions: Default::default(),
            root: loaded.program.root,
            fingerprint: loaded.program.fingerprint,
            modules: loaded
                .program
                .modules
                .iter()
                .map(|module| LinkedModule {
                    id: module.id,
                    name: module.name.clone(),
                    epoch: module.epoch,
                    bytecode: module.bytecode.clone(),
                    registry_owner: module.registry_owner,
                    host_types: module.host_types.clone(),
                    host_functions: module.host_functions.clone(),
                    host_paths: module.host_paths.clone(),
                })
                .collect(),
        }),
    }
}

fn assert_layout_check(check: impl FnOnce() -> bool, expected: bool, comparisons: u64) {
    #[cfg(feature = "execution-diagnostics")]
    {
        let (actual, counts) = crate::diagnostics::measure(check);
        assert_eq!(actual, expected);
        assert_eq!(counts.layout_comparisons, comparisons);
    }
    #[cfg(not(feature = "execution-diagnostics"))]
    {
        let _ = comparisons;
        assert_eq!(check(), expected);
    }
}

#[test]
fn immutable_descriptors_can_cross_threads_without_retaining_native_links_or_caches() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LoadedModule>();
    assert_send_sync::<VerifiedProgram>();
    let (runtime, loaded, _) = fixture();
    let function = runtime
        .modules
        .native_binding(&native_owner(&loaded), NativeImportId::new(0))
        .unwrap();
    let function_probe = Arc::downgrade(&function);
    let BindingEntry::Callback(entry) = &function.binding.entry else {
        panic!("fixture installs a callback");
    };
    let entry_probe = Arc::downgrade(entry);
    drop(function);
    let id = generic_structure(&loaded);
    let args = [Ty::Builtin(BuiltinType::I32)];
    let layout = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args, None)
        .unwrap();
    let cache_probe = Arc::downgrade(layout.applied.as_ref().unwrap());
    drop(layout);
    assert!(function_probe.upgrade().is_some());
    assert!(cache_probe.upgrade().is_some());
    drop(runtime);
    assert!(function_probe.upgrade().is_none());
    assert!(entry_probe.upgrade().is_none());
    assert!(cache_probe.upgrade().is_none());
    thread::spawn(move || {
        assert!(
            !loaded
                .to_unverified(&Default::default())
                .unwrap()
                .functions
                .is_empty()
        );
        let layout = loaded.applied_struct_layout(id, &args).unwrap();
        assert_eq!(layout.layout().fields[0].ty, args[0]);
    })
    .join()
    .unwrap();
}

#[test]
fn installed_layout_applications_reuse_caches_but_old_type_facts_survive_collection() {
    let (runtime, loaded, code) = fixture();
    let args = [Ty::Builtin(BuiltinType::I32)];
    let id = generic_structure(&loaded);
    let enum_id = generic_enum(&loaded);
    let first = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args, None)
        .unwrap();
    let second = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args, None)
        .unwrap();
    assert!(Arc::ptr_eq(
        first.applied.as_ref().unwrap(),
        second.applied.as_ref().unwrap()
    ));
    let variant = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 0, None)
        .unwrap();
    let other = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 1, None)
        .unwrap();
    assert!(Arc::ptr_eq(
        variant.applied.as_ref().unwrap(),
        other.applied.as_ref().unwrap()
    ));
    assert!(
        runtime
            .modules
            .applied_enum_variant(&loaded, enum_id, &args, u32::MAX, None)
            .is_none()
    );
    let owner = native_owner(&loaded);
    let function = runtime
        .modules
        .native_binding(&owner, NativeImportId::new(0))
        .unwrap();
    let probe = Arc::downgrade(&function);
    drop(function);
    let candidate = runtime
        .stage_reload_program(&loaded, "records", code)
        .unwrap();
    let latest = runtime.publish_staged_reload(candidate).unwrap();
    let reclaimed = runtime.collect_garbage().unwrap().reclaimed_modules;
    assert!(
        loaded
            .members()
            .all(|module| reclaimed.contains(&module.key()))
    );
    assert!(probe.upgrade().is_none());
    assert!(
        runtime
            .modules
            .native_binding(&owner, NativeImportId::new(0))
            .is_none()
    );
    assert_eq!(
        runtime.validate_loaded_module(&loaded).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(runtime.validate_loaded_module(&latest).is_ok());
    let retained = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args, None)
        .unwrap();
    assert!(retained.matches(&first));
    let retained = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 0, None)
        .unwrap();
    assert!(retained.matches_layout(&variant));
}

#[test]
fn layout_scopes_preserve_provenance_across_bounded_retention_and_retirement() {
    let (runtime, loaded, code) = fixture();
    let id = generic_structure(&loaded);
    let declaration = loaded.bytecode.structures[id.index()].declaration;
    let parameter = GenericParam {
        owner: declaration,
        position: 0,
    };
    let ty = Ty::Struct(NominalTy {
        declaration,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    });
    let arguments = runtime
        .resolve_type_arguments(&loaded, slice::from_ref(&ty))
        .unwrap();
    let scope = runtime
        .prepare_layout_scope(&loaded, declaration, &arguments)
        .unwrap()
        .unwrap();
    // Independently supplied facts and different members reuse the same program scope.
    let supplied = runtime
        .resolve_type_arguments(&loaded, slice::from_ref(&ty))
        .unwrap();
    let other = runtime
        .prepare_layout_scope(&native_owner(&loaded), declaration, &supplied)
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&scope, &other));
    let first = runtime
        .modules
        .applied_struct_layout(&loaded, id, slice::from_ref(&ty), None)
        .unwrap();
    let initial_identity = first.canonical.unwrap();
    let initial_scope = scope.id_for(&loaded).unwrap();
    let layout_probe = Arc::downgrade(first.applied.as_ref().unwrap());
    drop(first);
    let first_enum = runtime
        .modules
        .applied_enum_variant(
            &loaded,
            generic_enum(&loaded),
            slice::from_ref(&ty),
            0,
            None,
        )
        .unwrap();
    let enum_probe = Arc::downgrade(first_enum.applied.as_ref().unwrap());
    drop(first_enum);
    let scope_probe = Arc::downgrade(&scope);
    drop((scope, other));
    // More distinct applications than retained slots cannot keep the first alive.
    for width in 1..=160 {
        let ty = Ty::Tuple(vec![
            ty.clone(),
            Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]),
        ]);
        let arguments = runtime
            .resolve_type_arguments(&loaded, slice::from_ref(&ty))
            .unwrap();
        runtime
            .prepare_layout_scope(&loaded, declaration, &arguments)
            .unwrap();
        runtime
            .modules
            .applied_struct_layout(&loaded, id, slice::from_ref(&ty), None)
            .unwrap();
        runtime
            .modules
            .applied_enum_variant(&loaded, generic_enum(&loaded), &[ty], 0, None)
            .unwrap();
    }
    assert!(scope_probe.upgrade().is_none());
    assert!(layout_probe.upgrade().is_none());
    assert!(enum_probe.upgrade().is_none());
    // An independent owner keeps evicted facts readable, without an executable lease.
    let retained = runtime
        .prepare_layout_scope(&loaded, declaration, &arguments)
        .unwrap()
        .unwrap();
    let rebuilt = runtime
        .modules
        .applied_struct_layout(&loaded, id, slice::from_ref(&ty), None)
        .unwrap();
    assert_ne!(rebuilt.canonical, Some(initial_identity));
    assert_ne!(retained.id_for(&loaded), Some(initial_scope));
    let candidate = runtime
        .stage_reload_program(&loaded, "records", code)
        .unwrap();
    let latest = runtime.publish_staged_reload(candidate).unwrap();
    let fresh = runtime
        .resolve_type_arguments(&latest, slice::from_ref(&ty))
        .unwrap();
    let old_scope = runtime
        .prepare_layout_scope(&latest, declaration, &arguments)
        .unwrap()
        .unwrap();
    let new_scope = runtime
        .prepare_layout_scope(&latest, declaration, &fresh)
        .unwrap()
        .unwrap();
    assert!(!Arc::ptr_eq(&old_scope, &new_scope));
    assert_ne!(arguments[0].identity(&latest), fresh[0].identity(&latest));
    let collected = runtime.collect_garbage().unwrap();
    assert!(
        loaded
            .members()
            .all(|member| collected.reclaimed_modules.contains(&member.key()))
    );
    assert!(runtime.validate_loaded_module(&loaded).is_err());
    assert_eq!(
        retained.bindings().resolve(&parameter.as_type()).unwrap(),
        ty
    );
    assert_eq!(
        old_scope
            .bindings()
            .argument(&declaration, 0)
            .unwrap()
            .identity(&latest),
        arguments[0].identity(&latest)
    );
    let detached = runtime
        .prepare_layout_scope(&loaded, declaration, &arguments)
        .unwrap()
        .unwrap();
    assert_eq!(
        detached.bindings().resolve(&parameter.as_type()).unwrap(),
        ty
    );
    let (foreign_runtime, foreign_owner, _) = fixture();
    let foreign_ty = Ty::Struct(NominalTy {
        declaration: foreign_owner.bytecode.structures[generic_structure(&foreign_owner).index()]
            .declaration,
        arguments: vec![Ty::Builtin(BuiltinType::I32)],
        associated_types: Default::default(),
    });
    let foreign = foreign_runtime
        .resolve_type_arguments(&foreign_owner, &[foreign_ty])
        .unwrap();
    assert!(
        runtime
            .prepare_layout_scope(&loaded, declaration, &foreign)
            .is_err()
    );
    assert!(
        runtime
            .prepare_layout_scope(&foreign_owner, declaration, &arguments)
            .is_err()
    );
}

#[test]
fn equivalent_member_layouts_share_prepared_identity_without_aliasing_versions() {
    let (runtime, previous, mut code) = fixture();
    let args = [Ty::Builtin(BuiltinType::I32)];
    let source = code.root.index();
    let target = (0..code.modules.len())
        .find(|index| *index != source)
        .unwrap();
    let structure = code.modules[source]
        .structures
        .iter()
        .find(|layout| !layout.arguments.iter().all(Ty::is_concrete))
        .unwrap()
        .apply(&args, &Default::default())
        .unwrap()
        .into_owned();
    let enumeration = code.modules[source]
        .enumerations
        .iter()
        .find(|layout| !layout.arguments.iter().all(Ty::is_concrete))
        .unwrap()
        .apply(&args, &Default::default())
        .unwrap()
        .into_owned();
    // Different portable members may each carry the same checked applied layout,
    // as happens with native enum results and their consuming script patterns.
    for index in [source, target] {
        if !code.modules[index].structures.contains(&structure) {
            code.modules[index].structures.push(structure.clone());
        }
        if !code.modules[index].enumerations.contains(&enumeration) {
            code.modules[index].enumerations.push(enumeration.clone());
        }
    }
    let struct_template = code.modules[source]
        .structures
        .iter()
        .find(|layout| !layout.arguments.iter().all(Ty::is_concrete))
        .unwrap()
        .clone();
    let enum_template = code.modules[source]
        .enumerations
        .iter()
        .find(|layout| !layout.arguments.iter().all(Ty::is_concrete))
        .unwrap()
        .clone();
    let linked_argument = Ty::Struct(NominalTy {
        declaration: structure.declaration.clone(),
        arguments: args.to_vec(),
        associated_types: Default::default(),
    });
    code.modules[target].structures.push(
        struct_template
            .apply(slice::from_ref(&linked_argument), &Default::default())
            .unwrap()
            .into_owned(),
    );
    code.modules[target].structures.push(struct_template);
    code.modules[target].enumerations.push(enum_template);
    let candidate = runtime
        .stage_reload_program(&previous, "records", code)
        .unwrap();
    let loaded = runtime.publish_staged_reload(candidate).unwrap();
    let other = loaded.members().nth(target).unwrap();
    let args = [Ty::Builtin(BuiltinType::I32)];
    let struct_id = generic_structure(&loaded);
    let enum_id = generic_enum(&loaded);
    let structure = runtime
        .modules
        .applied_struct_layout(&loaded, struct_id, &args, None)
        .unwrap();
    let enumeration = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 0, None)
        .unwrap();
    let other_structure = other
        .bytecode
        .structures
        .iter()
        .position(|layout| layout == structure.layout())
        .unwrap();
    let other_enum = other
        .bytecode
        .enumerations
        .iter()
        .position(|layout| layout == enumeration.layout())
        .unwrap();
    let alias = other.struct_layout(StructId::new(other_structure)).unwrap();
    let enum_alias = other.enum_variant(EnumId::new(other_enum), 0).unwrap();
    assert_ne!(loaded.slot(), other.slot());
    assert!(structure.canonical.is_some());
    assert_eq!(structure.canonical, alias.canonical);
    assert!(structure.same_instance(&alias));
    assert!(enumeration.canonical.is_some());
    assert_eq!(enumeration.canonical, enum_alias.canonical);
    assert!(enumeration.matches_layout(&enum_alias));
    assert!(!enumeration.matches_layout(&enum_alias.with_variant(1).unwrap()));
    let linked_argument = Ty::Struct(NominalTy {
        declaration: structure.layout().declaration,
        arguments: args.to_vec(),
        associated_types: Default::default(),
    });
    let linked_args = runtime
        .resolve_type_arguments(&loaded, slice::from_ref(&linked_argument))
        .unwrap();
    let linked_scope = runtime
        .prepare_layout_scope(&loaded, structure.layout().declaration, &linked_args)
        .unwrap();
    let scoped_linked = runtime
        .modules
        .applied_struct_layout(
            &loaded,
            struct_id,
            slice::from_ref(&linked_argument),
            linked_scope,
        )
        .unwrap();
    let linked_slot = other
        .bytecode
        .structures
        .iter()
        .position(|layout| layout == scoped_linked.layout())
        .unwrap();
    let linked_consumer = other.struct_layout(StructId::new(linked_slot)).unwrap();
    assert!(scoped_linked.same_instance(&linked_consumer));
    // This nested application is absent from the linked concrete layout tables.
    let nominal = Ty::Struct(NominalTy {
        declaration: structure.layout().declaration,
        arguments: vec![Ty::Builtin(BuiltinType::I64)],
        associated_types: Default::default(),
    });
    let scoped_args = runtime
        .resolve_type_arguments(&loaded, slice::from_ref(&nominal))
        .unwrap();
    let scope = runtime
        .prepare_layout_scope(&loaded, structure.layout().declaration, &scoped_args)
        .unwrap();
    let dynamic = runtime
        .modules
        .applied_struct_layout(&loaded, struct_id, slice::from_ref(&nominal), scope.clone())
        .unwrap();
    let dynamic_alias = runtime
        .modules
        .applied_struct_layout(
            &other,
            generic_structure(&other),
            slice::from_ref(&nominal),
            scope,
        )
        .unwrap();
    assert!(dynamic.canonical.is_some());
    assert!(dynamic.same_instance(&dynamic_alias));
    let unscoped = runtime
        .modules
        .applied_struct_layout(
            &other,
            generic_structure(&other),
            slice::from_ref(&nominal),
            None,
        )
        .unwrap();
    assert!(dynamic.same_instance(&unscoped));
    let enum_scope = runtime
        .prepare_layout_scope(&loaded, enumeration.layout().declaration, &scoped_args)
        .unwrap();
    let dynamic_enum = runtime
        .modules
        .applied_enum_variant(
            &loaded,
            enum_id,
            slice::from_ref(&nominal),
            0,
            enum_scope.clone(),
        )
        .unwrap();
    let dynamic_enum_alias = runtime
        .modules
        .applied_enum_variant(
            &other,
            generic_enum(&other),
            slice::from_ref(&nominal),
            0,
            enum_scope,
        )
        .unwrap();
    assert!(dynamic_enum.canonical.is_some());
    assert_eq!(dynamic_enum.canonical, dynamic_enum_alias.canonical);
    assert!(dynamic_enum.matches_layout(&dynamic_enum_alias));
    let candidate = runtime
        .stage_reload_verified_program(&loaded, "records", loaded.verified_program().clone())
        .unwrap();
    let latest = runtime.publish_staged_reload(candidate).unwrap();
    let fresh = runtime
        .modules
        .applied_struct_layout(&latest, struct_id, &args, None)
        .unwrap();
    assert!(!structure.same_instance(&fresh));
    assert_layout_check(|| structure.matches(&fresh), true, 1);
    for repeats in [2_500, 5_000] {
        assert_layout_check(|| (0..repeats).all(|_| structure.matches(&fresh)), true, 0);
    }
    let fresh_enum = runtime
        .modules
        .applied_enum_variant(&latest, enum_id, &args, 0, None)
        .unwrap();
    assert_layout_check(|| enumeration.matches_layout(&fresh_enum), true, 1);
    for repeats in [2_500, 5_000] {
        assert_layout_check(
            || (0..repeats).all(|_| enumeration.matches_layout(&fresh_enum)),
            true,
            0,
        );
    }
    // The same printed argument from a new generation cannot inherit an old proof.
    let latest_args = runtime
        .resolve_type_arguments(&latest, slice::from_ref(&nominal))
        .unwrap();
    let changed_scope = runtime
        .prepare_layout_scope(&loaded, structure.layout().declaration, &latest_args)
        .unwrap();
    let changed = runtime
        .modules
        .applied_struct_layout(&loaded, struct_id, slice::from_ref(&nominal), changed_scope)
        .unwrap();
    assert!(!dynamic.same_instance(&changed));
    assert!(dynamic.matches(&changed));
    // A scope prepared for another binder must be rejected before publication.
    let wrong_scope = runtime
        .prepare_layout_scope(&loaded, enumeration.layout().declaration, &scoped_args)
        .unwrap();
    assert!(
        runtime
            .modules
            .applied_struct_layout(&loaded, struct_id, slice::from_ref(&nominal), wrong_scope)
            .is_none()
    );
    runtime.collect_garbage().unwrap();
    assert!(runtime.validate_loaded_module(&loaded).is_err());
    // Canonical type facts do not root executable instances and remain readable.
    assert!(enumeration.matches_layout(&enum_alias));
    assert!(dynamic.same_instance(&dynamic_alias));
    assert!(dynamic_enum.matches_layout(&dynamic_enum_alias));
    assert!(structure.same_instance(&alias));
}

#[test]
fn layout_admission_is_bounded_and_does_not_retain_producer_programs() {
    let (runtime, old, code) = fixture();
    let id = generic_structure(&old);
    let latest = runtime
        .publish_staged_reload(runtime.stage_reload_program(&old, "records", code).unwrap())
        .unwrap();
    let arguments = [Ty::Builtin(BuiltinType::I32)];
    let producer = runtime
        .modules
        .applied_struct_layout(&old, id, &arguments, None)
        .unwrap();
    let consumer = runtime
        .modules
        .applied_struct_layout(&latest, id, &arguments, None)
        .unwrap();
    assert!(producer.canonical.is_some() && consumer.canonical.is_some());
    assert_layout_check(|| consumer.matches(&producer), true, 1);
    assert_layout_check(|| consumer.matches(&producer), true, 0);
    let copied_owner = detached_copy(&old);
    let copied = copied_owner.applied_struct_layout(id, &arguments).unwrap();
    assert_eq!(producer.canonical, copied.canonical);
    // Equal numeric keys in another descriptor cannot reuse the producer's proof.
    assert_layout_check(|| consumer.matches(&copied), true, 1);
    assert_layout_check(|| consumer.matches(&producer), true, 0);
    drop((copied, copied_owner));
    for width in 1..=160 {
        let arguments = [Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width])];
        let actual = runtime
            .modules
            .applied_struct_layout(&old, id, &arguments, None)
            .unwrap();
        let expected = runtime
            .modules
            .applied_struct_layout(&latest, id, &arguments, None)
            .unwrap();
        assert_layout_check(|| expected.matches(&actual), true, 1);
        assert_layout_check(|| expected.matches(&actual), true, 0);
    }
    // Eviction removes only reusable evidence, never the validity of owned facts.
    assert_layout_check(|| consumer.matches(&producer), true, 1);
    assert_layout_check(|| consumer.matches(&producer), true, 0);
    let cross_thread = (consumer.clone(), producer.clone());
    thread::spawn(move || {
        for _ in 0..64 {
            assert!(cross_thread.0.matches(&cross_thread.1));
        }
    })
    .join()
    .unwrap();
    let (_, foreign, _) = fixture();
    let foreign = foreign
        .applied_struct_layout(generic_structure(&foreign), &arguments)
        .unwrap();
    assert_layout_check(|| consumer.matches(&foreign), false, 0);
    let producer_program = Arc::downgrade(&old.program);
    drop((producer, old));
    runtime.collect_garbage().unwrap();
    assert!(producer_program.upgrade().is_none());
    // Consumer-side admission evidence has no strong edge back to the producer.
    assert!(consumer.matches(&consumer));
}

#[test]
fn link_lookup_checks_runtime_version_bounds_and_store_borrows() {
    let (runtime, loaded, _) = fixture();
    let (foreign, foreign_loaded, _) = fixture();
    let owner = native_owner(&loaded);
    let other = native_owner(&foreign_loaded);
    assert_eq!(owner.key(), other.key());
    assert!(
        runtime
            .modules
            .native_binding(&other, NativeImportId::new(0))
            .is_none()
    );
    assert!(
        foreign
            .modules
            .native_binding(&owner, NativeImportId::new(0))
            .is_none()
    );
    assert!(
        runtime
            .modules
            .native_binding(&owner, NativeImportId::new(usize::MAX))
            .is_none()
    );
    let view = runtime.modules.instance_mut(loaded.key()).unwrap();
    assert!(
        runtime
            .modules
            .native_binding(&owner, NativeImportId::new(0))
            .is_none()
    );
    // Pure layout inspection needs no mutable instance access, including cache fallback.
    assert!(
        runtime
            .modules
            .applied_struct_layout(
                &loaded,
                generic_structure(&loaded),
                &[Ty::Builtin(BuiltinType::I32)],
                None
            )
            .is_some()
    );
    drop(view);
    assert!(
        runtime
            .modules
            .native_binding(&owner, NativeImportId::new(0))
            .is_some()
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn abandoned_candidates_release_links_even_when_their_descriptors_are_retained() {
    let (runtime, loaded, code) = fixture();
    let candidate = runtime
        .stage_reload_program(&loaded, "records", code)
        .unwrap();
    let owner = native_owner(candidate.module());
    let link = runtime
        .modules
        .native_binding(&owner, NativeImportId::new(0))
        .unwrap();
    let probe = Arc::downgrade(&link);
    drop(link);
    drop(candidate);
    assert!(
        runtime
            .modules
            .native_binding(&owner, NativeImportId::new(0))
            .is_none()
    );
    assert!(probe.upgrade().is_some());
    runtime.collect_garbage().unwrap();
    assert!(probe.upgrade().is_none());
    assert!(!owner.bytecode.native_imports.is_empty());
}

#[test]
fn matching_keys_and_copied_bindings_do_not_authorize_an_uninstalled_descriptor() {
    let (runtime, loaded, _) = fixture();
    let forged = detached_copy(&loaded);
    assert_eq!(loaded.key(), forged.key());
    assert!(forged.belongs_to(runtime.host.owner()));
    let before = runtime.modules.loaded_count();
    assert_eq!(
        runtime.validate_loaded_module(&forged).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(
        runtime
            .retain_program(&forged, ModuleEpochRetention::ActiveCall)
            .is_none()
    );
    assert!(
        runtime
            .retain_module(&forged, ModuleEpochRetention::RuntimeValue)
            .is_none()
    );
    assert!(
        runtime
            .modules
            .native_binding(&native_owner(&forged), NativeImportId::new(0))
            .is_none()
    );
    let id = generic_structure(&loaded);
    let args = [Ty::Builtin(BuiltinType::I32)];
    let cached = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args, None)
        .unwrap();
    let detached = runtime
        .modules
        .applied_struct_layout(&forged, id, &args, None)
        .unwrap();
    assert!(!Arc::ptr_eq(
        cached.applied.as_ref().unwrap(),
        detached.applied.as_ref().unwrap()
    ));
    assert!(cached.matches(&detached));
    assert_eq!(runtime.modules.loaded_count(), before);
    assert!(!runtime.is_quarantined());
}
