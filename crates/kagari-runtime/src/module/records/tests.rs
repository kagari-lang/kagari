use super::*;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    module::{LinkedModule, ModuleEpochRetention, ProgramDescriptor, VerifiedProgram},
    native::{
        binding::{Codec, NativeBinding},
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
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::thread;

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
    let function_probe = Rc::downgrade(&function);
    let entry_probe = Rc::downgrade(&function.binding.entry);
    drop(function);
    let id = generic_structure(&loaded);
    let args = [Ty::Builtin(BuiltinType::I32)];
    let layout = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args)
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
        .applied_struct_layout(&loaded, id, &args)
        .unwrap();
    let second = runtime
        .modules
        .applied_struct_layout(&loaded, id, &args)
        .unwrap();
    assert!(Arc::ptr_eq(
        first.applied.as_ref().unwrap(),
        second.applied.as_ref().unwrap()
    ));
    let variant = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 0)
        .unwrap();
    let other = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 1)
        .unwrap();
    assert!(Arc::ptr_eq(
        variant.applied.as_ref().unwrap(),
        other.applied.as_ref().unwrap()
    ));
    assert!(
        runtime
            .modules
            .applied_enum_variant(&loaded, enum_id, &args, u32::MAX)
            .is_none()
    );
    let owner = native_owner(&loaded);
    let function = runtime
        .modules
        .native_binding(&owner, NativeImportId::new(0))
        .unwrap();
    let probe = Rc::downgrade(&function);
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
        .applied_struct_layout(&loaded, id, &args)
        .unwrap();
    assert!(retained.matches(&first));
    let retained = runtime
        .modules
        .applied_enum_variant(&loaded, enum_id, &args, 0)
        .unwrap();
    assert!(retained.matches_layout(&variant));
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
                &[Ty::Builtin(BuiltinType::I32)]
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
    let probe = Rc::downgrade(&link);
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
    let forged = LoadedModule {
        slot: loaded.slot,
        program: Arc::new(ProgramDescriptor {
            code: loaded.program.code.clone(),
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
    };
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
        .applied_struct_layout(&loaded, id, &args)
        .unwrap();
    let detached = runtime
        .modules
        .applied_struct_layout(&forged, id, &args)
        .unwrap();
    assert!(!Arc::ptr_eq(
        cached.applied.as_ref().unwrap(),
        detached.applied.as_ref().unwrap()
    ));
    assert!(cached.matches(&detached));
    assert_eq!(runtime.modules.loaded_count(), before);
    assert!(!runtime.is_quarantined());
}
