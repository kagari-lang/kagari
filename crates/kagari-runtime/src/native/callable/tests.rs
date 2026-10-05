use super::*;
use crate::{error::RuntimeErrorKind, frame::types::EnvironmentRecord, module::LoadedModule};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_contract::ids::FunctionRef;
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

fn fixture() -> (Runtime, LoadedModule, PreparedClosure) {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "callback.kgr",
            "fn callback(captured: i32) -> i32 { captured }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir).unwrap();
    let mut runtime = Runtime::default();
    let loaded = runtime.load_program("callback", program).unwrap();
    assert_eq!(loaded.bytecode.functions.len(), 1);
    let environment = runtime
        .gc
        .alloc_environment(
            EnvironmentRecord::new(runtime.definition_context(), vec![], vec![]).unwrap(),
        )
        .unwrap();
    let value = runtime
        .make_closure(
            &loaded,
            FunctionRef::new(0),
            vec![Value::I32(7)],
            Some(environment),
        )
        .unwrap();
    (runtime, loaded, PreparedClosure { value })
}

#[test]
fn prepared_and_stored_descriptors_do_not_root_or_alias_recycled_closures() {
    let (runtime, loaded, prepared) = fixture();
    let stored = StoredCallable(Rc::new(StoredFunction {
        target: prepared.clone(),
        params: Box::new([]),
        result: Ty::Builtin(BuiltinType::I32),
    }));
    let environment_id = prepared
        .snapshot(&runtime)
        .unwrap()
        .environment
        .as_ref()
        .unwrap()
        .id;
    let Value::Closure(old) = *prepared.value() else {
        panic!("closure fixture")
    };
    let foreign = Runtime::default();
    assert_eq!(
        prepared.snapshot(&foreign).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 1);
    assert!(runtime.gc.environment(environment_id).is_none());
    assert_eq!(
        stored.0.target.snapshot(&runtime).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    let replacement = runtime
        .make_closure(&loaded, FunctionRef::new(0), vec![Value::I32(9)], None)
        .unwrap();
    let Value::Closure(next) = replacement else {
        panic!("closure fixture")
    };
    assert_eq!(old.index(), next.index());
    assert_ne!(old.generation(), next.generation());
    assert_eq!(
        prepared.snapshot(&runtime).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        runtime.resolve_closure(&replacement).unwrap().captures,
        vec![Value::I32(9)]
    );
}

#[test]
fn rooted_callback_retains_captures_but_not_storage_after_runtime_teardown() {
    let (runtime, _loaded, prepared) = fixture();
    let environment_id = prepared
        .snapshot(&runtime)
        .unwrap()
        .environment
        .as_ref()
        .unwrap()
        .id;
    let runtime_owner = runtime.resources().lifetime_probe();
    let rooted = RootedCallable {
        stored: StoredCallable(Rc::new(StoredFunction {
            target: prepared.clone(),
            params: Box::new([]),
            result: Ty::Builtin(BuiltinType::I32),
        })),
        _root: runtime.root_value(prepared.value().clone()).unwrap(),
    };
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 0);
    assert!(runtime.gc.environment(environment_id).is_some());
    assert_eq!(
        rooted.stored.0.target.snapshot(&runtime).unwrap().captures,
        vec![Value::I32(7)]
    );
    drop(runtime);
    assert!(runtime_owner.upgrade().is_none());
    let foreign = Runtime::default();
    assert_eq!(
        rooted
            .stored
            .0
            .target
            .snapshot(&foreign)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert_eq!(
        prepared.snapshot(&foreign).unwrap_err().kind(),
        RuntimeErrorKind::ModuleValidation
    );
}

#[test]
fn scoped_closure_views_reject_mutation_without_panicking_or_partial_writes() {
    let (runtime, loaded, prepared) = fixture();
    let array = runtime
        .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
        .unwrap();
    let snapshot = prepared.snapshot(&runtime).unwrap();
    let before = runtime.resources().counters();
    let stats = runtime.gc.stats();
    assert_eq!(
        runtime
            .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ModuleValidation
    );
    assert!(runtime.gc.array_push(array, Value::I32(2)).is_err());
    assert!(runtime.gc.array_set(array, 0, Value::I32(3)).is_err());
    assert!(runtime.gc.array_swap(array, 0, 0).is_err());
    assert_eq!(
        runtime.gc.array_snapshot(array).unwrap(),
        vec![Value::I32(1)]
    );
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc.stats(), stats);
    assert!(!runtime.is_quarantined());
    drop(snapshot);
    runtime.gc.array_push(array, Value::I32(2)).unwrap();
    assert_eq!(
        runtime.gc.array_snapshot(array).unwrap(),
        vec![Value::I32(1), Value::I32(2)]
    );
    let snapshot = prepared.snapshot(&runtime).unwrap();
    let before = runtime.resources().counters();
    let stats = runtime.gc.stats();
    assert_eq!(
        runtime.collect_garbage().unwrap_err().kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(runtime.resources().counters(), before);
    assert_eq!(runtime.gc.stats(), stats);
    drop(snapshot);
    assert!(runtime.gc.validate_value(prepared.value()));
    assert!(runtime.gc.validate_value(&Value::Array(array)));
}
