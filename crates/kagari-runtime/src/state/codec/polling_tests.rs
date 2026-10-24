//! Deterministic cancellation at checked helper boundaries, without timing races.
use crate::{
    Runtime,
    native::{
        conversion::{FromKagari, context::ConversionContext},
        value_handle::ScriptValue,
    },
    state::{
        self,
        codec::{export, schema, walk::Walk},
        data::{SchemaVersion, SnapshotHeader},
        error::{PathComponent, SnapshotErrorKind, SnapshotStage},
        limits::SnapshotOptions,
    },
    test_support::layouts::{allocation_owner, sequence_owner},
    value::Value,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::mem::size_of;

fn header() -> SnapshotHeader {
    SnapshotHeader {
        schema: SchemaVersion::new("polling", 1).unwrap(),
        source_deployment: "source".into(),
        state_owner: "owner".into(),
        state_revision: 1,
    }
}

#[test]
fn tuples_admit_each_child_and_honor_early_and_midpass_stops() {
    let mut runtime = Runtime::default();
    let owner = allocation_owner(&mut runtime);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let expected = cx.type_for::<(i32, (i32, i32))>().unwrap();
    let root = cx.encode((1i32, (2i32, 3i32))).unwrap();
    let raw = root.value(runtime.gc()).unwrap();
    let retained = ScriptValue::from_kagari(&mut cx, &expected, &raw).unwrap();
    let baseline = runtime.gc().active_roots();
    let mut options = SnapshotOptions::default();
    options.cancellation.cancel();
    assert_eq!(
        state::export(&mut cx, &retained, header(), &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::Cancelled
    );
    options = SnapshotOptions::default();
    options.limits.max_work = 1;
    assert_eq!(
        state::export(&mut cx, &retained, header(), &options)
            .unwrap_err()
            .kind,
        SnapshotErrorKind::LimitExceeded { limit: "work" }
    );

    let options = SnapshotOptions::default();
    let mut walk = Walk::new(&cx, &options, SnapshotStage::Export);
    let mut steps = 0;
    let error = walk
        .checked(&cx, |poll| {
            cx.check_value_with_poll(&expected, &raw, &mut || {
                steps += 1;
                if steps == 4 {
                    options.cancellation.cancel();
                }
                poll()
            })
        })
        .unwrap_err();
    assert_eq!(error.kind, SnapshotErrorKind::Cancelled);
    assert_eq!(
        steps, 4,
        "tuple admission must stop inside the nested tuple"
    );

    let options = SnapshotOptions::default();
    let mut walk = Walk::new(&cx, &options, SnapshotStage::Export);
    let schema = schema::describe(&cx, &expected, header().schema, &mut walk).unwrap();
    walk.budget.reset_data_counts();
    let bad_child = runtime
        .gc()
        .alloc_tuple(vec![Value::I32(2), Value::Bool(true)])
        .unwrap();
    let bad_child = runtime.root_value(bad_child).unwrap();
    let bad = runtime
        .gc()
        .alloc_tuple(vec![Value::I32(1), bad_child.value(runtime.gc()).unwrap()])
        .unwrap();
    let bad = runtime.root_value(bad).unwrap();
    let error = export::export(
        &mut cx,
        &expected,
        &bad.value(runtime.gc()).unwrap(),
        &schema,
        &mut walk,
    )
    .unwrap_err();
    assert_eq!(error.path.to_string(), "$[1][1]");
    assert_eq!(error.kind, SnapshotErrorKind::Runtime);
    drop(bad);
    drop(bad_child);
    assert_eq!(runtime.gc().active_roots(), baseline);
}

#[test]
fn restore_builder_tuple_validation_polls_and_charges_before_retaining() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "builder-polling.kgr",
            "pub struct Data { pub val value: (i32, (i32, i32)) } \
             pub fn inspect(value: Data) -> i32 { 0 }"
                .into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut analysis = AnalysisDatabase::default();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(file, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let code = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let mut runtime = Runtime::default();
    let program = runtime.load_program("builder-polling", code).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let function = owner
        .bytecode
        .functions
        .iter()
        .find(|function| function.name == "inspect")
        .unwrap();
    let argument = runtime
        .resolve_type_arguments(&owner, &[function.metadata.semantic.params[&0].clone()])
        .unwrap()
        .remove(0);
    let layout = runtime.prepare_struct_layout(&owner, &argument).unwrap();
    let object_type = runtime.retain_object_type(layout).unwrap();
    let field = object_type.field("value").unwrap();
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx.encode((1i32, (2i32, 3i32))).unwrap();
    let raw = root.value(runtime.gc()).unwrap();
    let malformed = cx.encode((1i32, (2i32, true))).unwrap();
    let baseline = runtime.gc().active_roots();

    // The admission entry, tuple root and first scalar precede the nested
    // tuple. Stop at its first scalar, inside the setter's full validation.
    let stop = 5;
    for cancel in [true, false] {
        let mut builder = object_type.builder().unwrap();
        let mut options = SnapshotOptions::default();
        if !cancel {
            // Include the field path edge before the setter's admission pass.
            options.limits.max_work = 1 + (stop - 1) * (1 + size_of::<Value>() as u64);
        }
        let mut walk = Walk::new(&cx, &options, SnapshotStage::Restore);
        walk.push(PathComponent::Field("value".into())).unwrap();
        let mut steps = 0;
        let error = walk
            .checked(&cx, |poll| {
                builder.set_value_in_with_poll(&cx, &field, raw, &mut || {
                    steps += 1;
                    if cancel && steps == stop {
                        options.cancellation.cancel();
                    }
                    poll()
                })
            })
            .unwrap_err();
        assert_eq!(
            error.kind,
            if cancel {
                SnapshotErrorKind::Cancelled
            } else {
                SnapshotErrorKind::LimitExceeded { limit: "work" }
            }
        );
        assert_eq!(error.path.to_string(), "$.value");
        assert_eq!(steps, stop);
        assert_eq!(runtime.gc().active_roots(), baseline);
        builder.check_member(&field).unwrap();

        let options = SnapshotOptions::default();
        let mut walk = Walk::new(&cx, &options, SnapshotStage::Restore);
        let error = walk
            .checked(&cx, |poll| {
                builder.set_value_in_with_poll(
                    &cx,
                    &field,
                    malformed.value(runtime.gc()).unwrap(),
                    poll,
                )
            })
            .unwrap_err();
        assert_eq!(error.kind, SnapshotErrorKind::Runtime);
        assert_eq!(runtime.gc().active_roots(), baseline);
        walk.checked(&cx, |poll| {
            builder.set_value_in_with_poll(&cx, &field, raw, poll)
        })
        .unwrap();
        assert_eq!(runtime.gc().active_roots(), baseline + 1);
        assert!(
            walk.checked(&cx, |poll| {
                builder.set_value_in_with_poll(&cx, &field, raw, poll)
            })
            .is_err()
        );
        assert_eq!(runtime.gc().active_roots(), baseline + 1);
        let object = builder.build_in(&mut cx).unwrap();
        drop(object);
        assert_eq!(runtime.gc().active_roots(), baseline);
    }
}

#[test]
fn sequence_snapshot_polls_options_during_copy_and_root_admission() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let root = cx.encode(vec![7i32; 1000]).unwrap();
    let raw = root.value(runtime.gc()).unwrap();
    let baseline = runtime.gc().active_roots();
    // First stop is inside copying. The second is inside the separate root
    // validation pass, not a poll after the complete thousand-element helper.
    for stop in [17, 1017] {
        let options = SnapshotOptions::default();
        let mut walk = Walk::new(&cx, &options, SnapshotStage::Export);
        let mut steps = 0;
        let error = walk
            .checked(&cx, |poll| {
                cx.snapshot_sequence_with_poll(&raw, &mut || {
                    steps += 1;
                    if steps == stop {
                        options.cancellation.cancel();
                    }
                    poll()
                })
            })
            .unwrap_err();
        assert_eq!(error.kind, SnapshotErrorKind::Cancelled);
        assert_eq!(steps, stop);
        assert_eq!(runtime.gc().active_roots(), baseline);
        assert_eq!(cx.decode::<Vec<i32>>(&root).unwrap().len(), 1000);
    }
}

#[test]
fn sequence_construction_polls_validation_copy_and_native_payload_admission() {
    let mut runtime = Runtime::default();
    let owner = sequence_owner(&mut runtime);
    let mut cx = ConversionContext::new(&runtime, &owner).unwrap();
    let expected = cx.type_for::<Vec<(i32, i32)>>().unwrap();
    let child = cx.encode((1i32, 2i32)).unwrap();
    let raw = child.value(runtime.gc()).unwrap();
    let baseline = runtime.gc().active_roots();
    let objects = runtime.gc().allocated_objects();
    let count = 1000;
    // Four polls per tuple element: the buffer edge and its three type nodes.
    // The final stop reaches the independent native-payload validity pass.
    for stop in [17, 4017] {
        let options = SnapshotOptions::default();
        let mut walk = Walk::new(&cx, &options, SnapshotStage::Restore);
        let mut steps = 0;
        let error = walk
            .checked(&cx, |poll| {
                runtime.allocate_sequence_with_poll(
                    &owner,
                    &expected,
                    vec![raw; count],
                    &mut || {
                        steps += 1;
                        if steps == stop {
                            options.cancellation.cancel();
                        }
                        poll()
                    },
                )
            })
            .unwrap_err();
        assert_eq!(error.kind, SnapshotErrorKind::Cancelled);
        assert_eq!(steps, stop);
        assert_eq!(runtime.gc().active_roots(), baseline);
        assert_eq!(
            runtime.gc().allocated_objects(),
            objects,
            "no partially prepared sequence is published"
        );
    }
    let options = SnapshotOptions::default();
    let mut walk = Walk::new(&cx, &options, SnapshotStage::Restore);
    let bad = runtime
        .gc()
        .alloc_tuple(vec![Value::Bool(true), Value::I32(2)])
        .unwrap();
    let bad = runtime.root_value(bad).unwrap();
    assert!(
        walk.checked(&cx, |poll| runtime.allocate_sequence_with_poll(
            &owner,
            &expected,
            vec![bad.value(runtime.gc()).unwrap()],
            poll
        ))
        .is_err()
    );
}
