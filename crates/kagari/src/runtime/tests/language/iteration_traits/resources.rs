use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    test_support::cancellation as support,
};
use kagari_common::cancellation::CancellationToken;
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    native::conversion::context::ConversionContext, session::ExecutionOptions, value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};

#[test]
fn native_iterators_validate_revisions_and_survive_gc_and_failure() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "iterator-resources.kgr",
                r#"
pub fn fail()->i32{val a=Vec::from([20]);for x in a {a.push(1);}0}
pub fn nested()->i32{val a=Vec::from([20]);for x in a {for y in a {break;}a.push(1);}0}
pub fn manual()->i32{val a=Vec::from([20]);val c=a.iter();c.next();a.push(1);c.next();0}
pub fn trap()->i32{val a=Vec::from([20]);for x in a {return x/0;}0}
pub fn exhaust()->i32{val a=Vec::from([20]);for x in a {while true {}}0}
"#,
            ),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded_program =
        PreparedProgram::from_artifact(artifact.clone(), &Default::default(), &Default::default())
            .unwrap();
    let loaded = runtime
        .load_program(&loaded_program, Default::default())
        .unwrap();
    for entry in ["fail", "nested", "manual", "trap", "exhaust"] {
        let options = context.clone();
        let cancellation =
            (entry == "exhaust").then(|| support::cancel_after(runtime.runtime(), &loaded, 30));
        let error = runtime.execute(&loaded, entry, &[], &options).unwrap_err();
        assert!(
            !format!("{error:?}").contains("MissingFunction"),
            "{entry}: {error:?}"
        );
        drop(cancellation);
        assert!(!format!("{error:?}").contains("UnsupportedExecution"));
        assert!(!runtime.runtime().is_quarantined());
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry}");
    }
    let rt = runtime.runtime();
    let root = ConversionContext::new(rt, &loaded)
        .unwrap()
        .encode(vec![20i32, 22])
        .unwrap();
    let Value::GcHandle(array) = root.value(rt.gc()).unwrap() else {
        panic!("Vec")
    };
    let vector_type = ConversionContext::new(rt, &loaded)
        .unwrap()
        .type_for::<Vec<i32>>()
        .unwrap();
    let item = Ty::Builtin(BuiltinType::I32);
    let ty = Ty::Iter(Box::new(item.clone()));
    let cancellation = CancellationToken::default();
    let options = ExecutionOptions {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let session = rt.begin_execution(&loaded, options).unwrap();
    let value = rt
        .iter_operation(
            &loaded,
            &root.value(rt.gc()).unwrap(),
            vector_type.ty(),
            IterOp::New,
        )
        .unwrap();
    let iter = rt.root_value(value).unwrap();
    rt.gc().sequence_set(array, 0, Value::I32(20)).unwrap();
    cancellation.cancel();
    assert!(rt.gc_safepoint().is_err());
    drop(session);
    rt.collect_garbage().unwrap();
    for expected in [20, 22] {
        let session = rt.begin_execution(&loaded, Default::default()).unwrap();
        let Value::Enum(id) = rt
            .iter_operation(&loaded, &iter.value(rt.gc()).unwrap(), &ty, IterOp::Next)
            .unwrap()
        else {
            panic!("Option")
        };
        assert_eq!(
            rt.gc().enum_snapshot(id).unwrap().fields,
            vec![Value::I32(expected)]
        );
        drop(session);
    }
    // Structural edits commit between calls; the old cursor rejects its next read.
    rt.gc().sequence_push(array, Value::I32(1)).unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.iter_operation(&loaded, &iter.value(rt.gc()).unwrap(), &ty, IterOp::Next)
            .is_err()
    );
    drop(session);
    let mut other = engine.runtime(context);
    let other_loaded_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let other_loaded = other
        .load_program(&other_loaded_program, Default::default())
        .unwrap();
    let session = other
        .runtime()
        .begin_execution(&other_loaded, Default::default())
        .unwrap();
    assert!(
        other
            .runtime()
            .iter_operation(
                &other_loaded,
                &iter.value(rt.gc()).unwrap(),
                &ty,
                IterOp::Next
            )
            .is_err()
    );
    drop(session);
    drop(iter);
    rt.collect_garbage().unwrap();
    let session = rt.begin_execution(&loaded, Default::default()).unwrap();
    assert!(
        rt.iter_operation(&loaded, &value, &ty, IterOp::Next)
            .is_err()
    );
    drop(session);
}
