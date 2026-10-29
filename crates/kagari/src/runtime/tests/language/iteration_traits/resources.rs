use crate::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
    test_support::cancellation as support,
};
use kagari_common::cancellation::CancellationToken;
use kagari_runtime::{
    native::{
        conversion::{IntoKagari, context::ConversionContext},
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    session::ExecutionOptions,
};
use kagari_source::source::SourceFile;
use kagari_stdlib::collections::vector::{ScriptVec, VecValues};

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
pub fn cursor(a:Vec<i32>)->Iterator<Item = i32>{a.iter()}
pub fn next(c:Iterator<Item = i32>)->Option<i32>{c.next()}
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
    let _selected_loaded = runtime.runtime().select_program(&loaded).unwrap();
    for entry in ["fail", "nested", "manual", "trap", "exhaust"] {
        let options = context.clone();
        let cancellation = (entry == "exhaust").then(|| {
            support::cancel_after(
                runtime.runtime(),
                &runtime.runtime().select_program(&loaded).unwrap(),
                30,
            )
        });
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
    let root = ConversionContext::new(rt, &_selected_loaded)
        .unwrap()
        .encode(VecValues(vec![20i32, 22]))
        .unwrap();
    let values: ScriptVec<i32> = ConversionContext::new(rt, &_selected_loaded)
        .unwrap()
        .decode(&root)
        .unwrap();
    let cancellation = CancellationToken::default();
    let options = ExecutionOptions {
        cancellation: cancellation.clone(),
        ..Default::default()
    };
    let session = rt.begin_execution(&_selected_loaded, options).unwrap();
    let iter: ScriptValue = runtime
        .execute_typed(&loaded, "cursor", (values.clone(),), &context)
        .unwrap();
    let cursor_type = iter.type_argument().clone();
    let value = unsafe {
        iter.clone()
            .into_kagari(
                &mut ConversionContext::new(rt, &_selected_loaded).unwrap(),
                &cursor_type,
            )
            .unwrap()
            .value_transport(rt)
    }
    .unwrap();
    values
        .set(
            &mut NativeContext::new(rt, &_selected_loaded).unwrap(),
            0,
            20,
        )
        .unwrap();
    cancellation.cancel();
    assert!(rt.gc_safepoint().is_err());
    drop(session);
    rt.collect_garbage().unwrap();
    for expected in [20, 22] {
        assert_eq!(
            runtime
                .execute_typed::<_, Option<i32>>(&loaded, "next", (iter.clone(),), &context)
                .unwrap(),
            Some(expected)
        );
    }
    // Structural edits commit between calls; the old cursor rejects exhaustion.
    values
        .push(&mut NativeContext::new(rt, &_selected_loaded).unwrap(), 1)
        .unwrap();
    assert!(
        runtime
            .execute_typed::<_, Option<i32>>(&loaded, "next", (iter.clone(),), &context)
            .is_err()
    );
    let mut other = engine.runtime(context.clone());
    let other_program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let other_loaded = other
        .load_program(&other_program, Default::default())
        .unwrap();
    assert!(
        other
            .execute_typed::<_, Option<i32>>(&other_loaded, "next", (iter.clone(),), &context)
            .is_err()
    );
    drop(iter);
    rt.collect_garbage().unwrap();
    // Checked-ID diagnostics confirm reclamation after the last retained cursor.
    // Safe conversion no longer accepts raw transport; its compile-fail contract
    // prevents reconstructing an owning cursor from this expired diagnostic ID.
    assert!(!unsafe { rt.gc().validate_value_transport(&value) });
}
