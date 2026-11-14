use crate::tests::support::bytecode::load_bytecode;
use kagari_bytecode::{artifact::KbcArtifact, program::BytecodeProgram};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeError,
    execution::memory::GcHeapConfig,
    frame::types::arguments::TypeArgument,
    module::InstalledProgram,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{IntoKagari, KagariType, context::ConversionContext},
        module::NativeModule,
        objects::{Object, ObjectType},
        typed::NativeContext,
        types::Type,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

const SOURCE: &str = include_str!("../../../tests/fixtures/runtime_boundary/host_objects.kgr");

fn program(source: &str) -> DefinitionMetadata<BytecodeProgram> {
    let context = DefinitionContext::new().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(crate::catalog::shared().unwrap());
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("objects.kgr", source.into(), SourceLayer::Base)
        .unwrap();
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
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records();
    let artifact =
        KbcArtifact::from_program(program, Default::default(), &context, &Default::default())
            .unwrap();
    let bytes = artifact.to_bytes(&context.snapshot()).unwrap();
    KbcArtifact::from_bytes(&bytes)
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
        .into_verified(&Default::default(), &context, &Default::default())
        .unwrap()
        .into_bytecode()
        .into_unverified()
}

fn runtime() -> Runtime {
    let mut runtime = Runtime::new(RuntimeConfig {
        gc: GcHeapConfig {
            collection_threshold: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    runtime
}

fn fixture() -> (Runtime, InstalledProgram) {
    let mut runtime = runtime();
    let owner = load_bytecode(&mut runtime, "objects", program(SOURCE)).unwrap();
    (runtime, owner)
}

fn player(cx: &mut NativeContext<'_>, ty: &ObjectType, hp: i32) -> Object {
    let health = cx.runtime_mut().bind_field::<i32>(ty, "hp").unwrap();
    let name = cx.runtime_mut().bind_field::<String>(ty, "name").unwrap();
    let mut builder = ty.builder().unwrap();
    builder.set(cx, &health, hp).unwrap();
    builder.set(cx, &name, "Ada".to_owned()).unwrap();
    builder.build(cx).unwrap()
}

#[test]
fn prepared_fields_preserve_aliases_and_public_access_after_gc() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let ty = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let name = runtime
        .bind_field_declaration::<String>(&ty.field("name").unwrap())
        .unwrap();
    assert!(runtime.bind_field::<u32>(&ty, "hp").is_err());
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let object = player(&mut cx, &ty, 100);
    let alias = object.clone();
    for remaining in [90, 80, 70] {
        object.set(&mut cx, &hp, remaining).unwrap();
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(alias.get(&mut cx, &hp).unwrap(), remaining);
    }
    assert_eq!(object.get(&mut cx, &name).unwrap(), "Ada");
    assert!(object.set(&mut cx, &name, "Grace".into()).is_err());
    assert_eq!(object.get(&mut cx, &name).unwrap(), "Ada");
    drop(object);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(alias.get(&mut cx, &hp).unwrap(), 70);
    drop(alias);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );

    let hidden = cx
        .runtime_mut()
        .bind_type(&_owner_program.root(), "Hidden", &[])
        .unwrap();
    assert!(
        cx.runtime_mut()
            .bind_field::<i32>(&hidden, "secret")
            .is_err()
    );
    assert!(
        cx.runtime_mut()
            .bind_field::<i32>(&hidden, "internal")
            .is_err()
    );
    assert!(
        cx.runtime_mut()
            .bind_field::<i32>(&hidden, "visible")
            .is_ok()
    );
    assert!(hidden.builder().is_err());
    assert!(
        cx.runtime_mut()
            .bind_type(&_owner_program.root(), "Private", &[])
            .is_err()
    );
}

#[test]
fn object_reads_retain_children_and_failed_construction_releases_temporaries() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let player_type = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let pair_type = runtime
        .bind_type(&_owner_program.root(), "Pair", &[])
        .unwrap();
    let first = runtime.bind_field::<Object>(&pair_type, "first").unwrap();
    let second = runtime.bind_field::<Object>(&pair_type, "second").unwrap();
    let hp = runtime.bind_field::<i32>(&player_type, "hp").unwrap();
    let name = runtime.bind_field::<String>(&player_type, "name").unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let child = player(&mut cx, &player_type, 42);
    let mut incomplete = pair_type.builder().unwrap();
    incomplete.set(&mut cx, &first, child.clone()).unwrap();
    assert!(incomplete.set(&mut cx, &first, child.clone()).is_err());
    assert!(incomplete.build(&mut cx).is_err());
    // The retained player also owns its separately traced String.
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects + 2);
    let mut complete = pair_type.builder().unwrap();
    complete.set(&mut cx, &first, child.clone()).unwrap();
    complete.set(&mut cx, &second, child.clone()).unwrap();
    let pair = complete.build(&mut cx).unwrap();
    let read = pair.get(&mut cx, &first).unwrap();
    drop(child);
    drop(pair);
    // The retained player also owns its separately traced String.
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects + 2);
    assert_eq!(read.get(&mut cx, &hp).unwrap(), 42);
    assert_eq!(read.get(&mut cx, &name).unwrap(), "Ada");
    drop(read);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn fields_reject_foreign_runtimes_and_unrelated_nominal_types() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let ty = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let other = runtime
        .bind_type(&_owner_program.root(), "Other", &[])
        .unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let object = player(&mut cx, &other, 10);
    assert!(object.get(&mut cx, &hp).is_err());
    assert!(object.set(&mut cx, &hp, 20).is_err());
    let object = player(&mut cx, &ty, 10);
    let (mut foreign, foreign_owner) = fixture();
    let _foreign_owner_program = foreign_owner;
    let foreign_owner = foreign.select_program(&_foreign_owner_program).unwrap();
    assert!(foreign.bind_field::<i32>(&ty, "hp").is_err());
    let mut foreign_cx = NativeContext::new(&mut foreign, &foreign_owner).unwrap();
    assert!(object.get(&mut foreign_cx, &hp).is_err());
    assert!(object.set(&mut foreign_cx, &hp, 20).is_err());
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 10);
}

struct FailAfterGc(Arc<AtomicBool>);

impl KagariType for FailAfterGc {
    fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
        i32::kagari_type(catalog)
    }
}

impl IntoKagari for FailAfterGc {
    fn into_kagari<'call>(
        self,
        cx: &mut ConversionContext<'call>,
        _: &TypeArgument,
    ) -> NativeResult<ValueView<'call>> {
        self.0.store(true, Ordering::Relaxed);
        cx.runtime_mut().collect_garbage()?;
        Err(RuntimeError::module_validation("converter failure"))
    }
}

#[test]
fn a_failed_conversion_preserves_earlier_commits_and_never_publishes_partial_writes() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let ty = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let failing = runtime.bind_field::<FailAfterGc>(&ty, "hp").unwrap();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let object = player(&mut cx, &ty, 100);
    object.set(&mut cx, &hp, 90).unwrap();
    let attempted = Arc::new(AtomicBool::new(false));
    assert!(
        object
            .set(&mut cx, &failing, FailAfterGc(attempted.clone()))
            .is_err()
    );
    assert!(attempted.load(Ordering::Relaxed));
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 90);
}

#[test]
fn applied_fields_preserve_nominal_arguments() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let player_type = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let cell_type = runtime
        .bind_type(
            &_owner_program.root(),
            "Cell",
            &[player_type.type_argument().clone()],
        )
        .unwrap();
    let value = runtime.bind_field::<Object>(&cell_type, "value").unwrap();
    let hp = runtime.bind_field::<i32>(&player_type, "hp").unwrap();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let child = player(&mut cx, &player_type, 11);
    let mut builder = cell_type.builder().unwrap();
    builder.set(&mut cx, &value, child).unwrap();
    let cell = builder.build(&mut cx).unwrap();
    let child = cell.get(&mut cx, &value).unwrap();
    drop(cell);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(child.get(&mut cx, &hp).unwrap(), 11);
}

#[test]
fn binding_handles_pin_old_versions_but_the_cache_does_not() {
    let (mut runtime, old) = fixture();
    let _old_program = old;
    let old = runtime.select_program(&_old_program).unwrap();
    let ty = runtime
        .bind_type(&_old_program.root(), "Player", &[])
        .unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let duplicate = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    let object = player(
        &mut NativeContext::new(&mut runtime, &old).unwrap(),
        &ty,
        50,
    );
    let candidate = runtime
        .stage_reload_program(&_old_program, Some("objects".into()), program(SOURCE))
        .unwrap();
    let new = candidate.module().clone();
    runtime.publish_staged_reload(candidate).unwrap();
    let new_ty = runtime
        .bind_type(&_old_program.root(), "Player", &[])
        .unwrap();
    let new_hp = runtime.bind_field::<i32>(&new_ty, "hp").unwrap();
    let mut cx = NativeContext::new(&mut runtime, &new).unwrap();
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(object.get(&mut cx, &hp).unwrap(), 50);
    assert!(object.get(&mut cx, &new_hp).is_err());
    drop(object);
    drop(ty);
    drop(hp);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_ok());
    drop(duplicate);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(cx.runtime().validate_executable_module(&old).is_err());
    assert!(
        cx.runtime_mut()
            .bind_type(&_old_program.root(), "Player", &[])
            .is_ok()
    );
}

#[test]
fn nested_dynamic_handles_store_heap_edges_and_cycles_are_collectible() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let node_type = runtime
        .bind_type(&_owner_program.root(), "Node", &[])
        .unwrap();
    let next = runtime
        .bind_field::<Option<Object>>(&node_type, "next")
        .unwrap();
    let bag_type = runtime
        .bind_type(&_owner_program.root(), "Bag", &[])
        .unwrap();
    let items = runtime
        .bind_field::<(Option<Object>, Option<Object>)>(&bag_type, "items")
        .unwrap();
    let pair = runtime
        .bind_field::<(Object, i32)>(&bag_type, "pair")
        .unwrap();
    assert!(
        runtime
            .bind_field::<Result<Object, String>>(&node_type, "next")
            .is_err()
    );
    assert!(
        runtime
            .bind_field::<(Object, u32)>(&bag_type, "pair")
            .is_err()
    );
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
    let mut builder = node_type.builder().unwrap();
    builder.set(&mut cx, &next, None).unwrap();
    let node = builder.build(&mut cx).unwrap();
    node.set(&mut cx, &next, Some(node.clone())).unwrap();
    let mut builder = bag_type.builder().unwrap();
    builder
        .set(&mut cx, &items, (Some(node.clone()), None))
        .unwrap();
    builder.set(&mut cx, &pair, (node.clone(), 42)).unwrap();
    let bag = builder.build(&mut cx).unwrap();
    let read = bag.get(&mut cx, &items).unwrap();
    let (pair_read, number) = bag.get(&mut cx, &pair).unwrap();
    assert_eq!(number, 42);
    assert!(read.1.is_none());
    drop(node);
    drop(bag);
    cx.runtime_mut().collect_garbage().unwrap();
    assert!(pair_read.get(&mut cx, &next).unwrap().is_some());
    assert!(
        read.0
            .as_ref()
            .unwrap()
            .get(&mut cx, &next)
            .unwrap()
            .is_some()
    );
    drop(pair_read);
    drop(read);
    cx.runtime_mut().collect_garbage().unwrap();
    assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
    assert_eq!(
        cx.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
}

#[test]
fn typed_handles_and_bindings_move_with_exclusive_runtime_ownership() {
    let (mut runtime, owner) = fixture();
    let _owner_program = owner;
    let owner = runtime.select_program(&_owner_program).unwrap();
    let ty = runtime
        .bind_type(&_owner_program.root(), "Player", &[])
        .unwrap();
    let hp = runtime.bind_field::<i32>(&ty, "hp").unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let (object, values) = {
        let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
        let object = player(&mut cx, &ty, 42);
        let values = cx
            .runtime_mut()
            .alloc_array(
                &owner,
                Ty::Builtin(BuiltinType::I32),
                &[RootedValue::from(1i32), RootedValue::from(2i32)],
            )
            .unwrap();
        (object, values)
    };
    thread::spawn(move || {
        let mut cx = NativeContext::new(&mut runtime, &owner).unwrap();
        cx.runtime_mut().collect_garbage().unwrap();
        object.set(&mut cx, &hp, 43).unwrap();
        {
            let mut conversion = ConversionContext::new(cx.runtime_mut(), &owner).unwrap();
            let array = conversion.import(&values).unwrap();
            conversion
                .array_set(&array, 1usize, &ValueView::from(3i32))
                .unwrap();
            assert_eq!(
                conversion
                    .array_element(&array, 1usize)
                    .unwrap()
                    .unwrap()
                    .as_i32(),
                Some(3)
            );
        }
        assert_eq!(object.get(&mut cx, &hp).unwrap(), 43);
        drop(object);
        drop(values);
        cx.runtime_mut().collect_garbage().unwrap();
        assert_eq!(cx.runtime().gc_stats().objects, baseline.objects);
        assert_eq!(
            cx.runtime().gc_stats().root_registrations,
            baseline.root_registrations
        );
        (ty, hp)
        // The receiving thread destroys storage; immutable bindings may outlive it.
    })
    .join()
    .unwrap();
}
