use super::*;
use kagari_common::host_interface::path::HostPathSegmentDeclaration;
use kagari_runtime::host::HostPathAdapter;
use {kagari_bytecode::artifact::KbcArtifact, kagari_embed::context::JitPolicy};

use kagari_embed::program::PreparedProgram;

#[test]
fn source_field_chains_use_offline_contracts_and_evaluate_the_root_once() {
    use kagari_common::host_interface::{path::HostPathDeclaration, type_declaration::PathAccess};
    let mut declarations = interface();
    declarations.types[0].fields[0].path_access = PathAccess::ReadOnly;
    let mut count =
        HostFieldDeclaration::new(&declarations.types[1].id, "count", HostValueType::I32);
    count.path_access = PathAccess::ReadOnly;
    let path = HostPathDeclaration {
        root: declarations.types[0].id.clone(),
        segments: vec![
            HostPathSegmentDeclaration::Field(declarations.types[0].fields[0].id.clone()),
            HostPathSegmentDeclaration::Field(count.id.clone()),
        ],
        access: PathAccess::ReadOnly,
        schema_epoch: 7,
    };
    declarations.types[1].fields.push(count);
    declarations.paths.push(path.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations.clone()).unwrap();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "field.kgr",
                "use left as api; fn main() -> i32 { api::make().related.count }",
            ),
            Default::default(),
        )
        .unwrap();
    let required = &artifact.program.modules[artifact.program.root.index()].host_interface;
    assert_eq!(required.paths, vec![path.clone()]);
    assert_eq!(required.types.len(), 2);
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(
                declarations.types[..2]
                    .iter()
                    .cloned()
                    .map(|ty| HostTypeRegistration::new(ty, "Object"))
                    .collect(),
            )
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(7))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(
                declarations.functions[0].clone(),
                move |_, _| {
                    calls.borrow_mut().push("root");
                    Ok(Value::HostRoot(root))
                },
            ))
            .unwrap();
        assert!(
            runtime
                .load_program(
                    &PreparedProgram::from_artifact(
                        artifact.clone(),
                        &Default::default(),
                        &Default::default()
                    )
                    .unwrap(),
                    Default::default()
                )
                .is_err()
        );
        assert!(trace.borrow().is_empty());
        let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
        let calls = trace.clone();
        runtime
            .runtime_mut()
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new().with_read(move |context, _| {
                    calls.borrow_mut().push("read");
                    context.runtime().collect_garbage().unwrap();
                    Ok(Value::I32(42))
                }),
            )
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let mut denied = context.clone();
        denied.capabilities.fs_read = false;
        denied.jit_policy = JitPolicy::Disabled;
        let denied_error = runtime.execute(&loaded, "main", &[], &denied).unwrap_err();
        assert_eq!(*trace.borrow(), ["root"], "{denied_error:?}");
        trace.borrow_mut().clear();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(*trace.borrow(), ["root", "read"]);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn source_host_index_paths_capture_index_before_rhs_across_backends() {
    assert_source_index_path(false);
}

#[test]
fn source_host_field_index_paths_skip_intermediate_reads() {
    assert_source_index_path(true);
}

#[test]
fn source_multi_index_virtual_and_trailing_field_use_one_host_path() {
    use kagari_common::host_interface::{
        path::{
            HostIndexSegmentDeclaration, HostPathDeclaration, HostPathSegmentDeclaration,
            HostVirtualSegmentDeclaration,
        },
        type_declaration::PathAccess,
    };
    use kagari_runtime::host::{HostError, HostPathAdapter, PreparedHostPathWrite};
    use std::cell::Cell;

    let mut player = HostTypeDeclaration::new("game.Player");
    player.ownership = HostTypeOwnership::HostRoot;
    player.path_access = PathAccess::ReadWrite;
    let mut child = HostTypeDeclaration::new("game.Child");
    let mut score = HostFieldDeclaration::new(&child.id, "score", HostValueType::I32);
    score.path_access = PathAccess::ReadWrite;
    score.writable = true;
    child.fields.push(score.clone());
    let make = HostFunctionDeclaration::new(
        "game.make",
        vec![],
        HostValueType::Opaque(player.id.clone()),
    );
    let first = HostFunctionDeclaration::new("game.first", vec![], HostValueType::I32);
    let second = HostFunctionDeclaration::new("game.second", vec![], HostValueType::I32);
    let rhs = HostFunctionDeclaration::new("game.rhs", vec![], HostValueType::I32);
    let path = HostPathDeclaration {
        root: player.id.clone(),
        segments: vec![
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 1,
                collection: HostValueType::Opaque(player.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadWrite,
            }),
            HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::Opaque(child.id.clone()),
                index: HostValueType::I32,
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadWrite,
            }),
            HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
                name: "selected".into(),
                result: HostValueType::Opaque(child.id.clone()),
                access: PathAccess::ReadWrite,
            }),
            HostPathSegmentDeclaration::Field(score.id.clone()),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    let engine = KagariEngine::default();
    engine
        .set_host_interface(HostInterface {
            paths: vec![path.clone()],
            types: vec![player.clone(), child.clone()],
            functions: vec![make.clone(), first.clone(), second.clone(), rhs.clone()],
        })
        .unwrap();

    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "multi-index.kgr",
                "use game as api; fn main() -> i32 { var player = api::make(); player[api::first()][api::second()].selected.score += api::rhs(); player[1][2].selected.score }",
            ),

            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let context = ExecutionContext {
            jit_policy: if jit {
                JitPolicy::Enabled
            } else {
                JitPolicy::Disabled
            },
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let ids = runtime
            .register_host_types(vec![
                HostTypeRegistration::new(player.clone(), "Player"),
                HostTypeRegistration::new(child.clone(), "Child"),
            ])
            .unwrap();
        let root = runtime
            .runtime_mut()
            .register_host_root(HostObjectId(1), ids[0], HostSchemaEpoch::new(0))
            .unwrap();
        let trace = Rc::new(RefCell::new(Vec::new()));
        let state = Rc::new(Cell::new(10));
        let calls = trace.clone();
        runtime
            .register_host_function(HostFunction::new(make.clone(), move |_, _| {
                calls.borrow_mut().push("make");
                Ok(Value::HostRoot(root))
            }))
            .unwrap();
        for (declaration, label, result) in [
            (first.clone(), "first", 1),
            (second.clone(), "second", 2),
            (rhs.clone(), "rhs", 3),
        ] {
            let calls = trace.clone();
            runtime
                .register_host_function(HostFunction::new(declaration, move |_, _| {
                    calls.borrow_mut().push(label);
                    Ok(Value::I32(result))
                }))
                .unwrap();
        }
        let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
        let (reads, value) = (trace.clone(), state.clone());
        let (writes, target) = (trace.clone(), state.clone());
        runtime
            .runtime_mut()
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_read(move |_, path| {
                        let values = path
                            .dynamic_args
                            .as_slice()
                            .iter()
                            .map(|arg| arg.value.clone())
                            .collect::<Vec<_>>();
                        if values != [Value::I32(2), Value::I32(1)] {
                            return Err(HostError::new("unexpected indexes"));
                        }
                        reads.borrow_mut().push("read");
                        Ok(Value::I32(value.get()))
                    })
                    .with_prepare_write(move |_, _, record| {
                        writes.borrow_mut().push("prepare");
                        let Value::I32(next) = record.new_value else {
                            return Err(HostError::new("expected i32"));
                        };
                        let (writes, target) = (writes.clone(), target.clone());
                        Ok(PreparedHostPathWrite::new(move || {
                            writes.borrow_mut().push("commit");
                            target.set(next);
                        }))
                    }),
            )
            .unwrap();
        let loaded_program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let loaded = runtime
            .load_program(&loaded_program, Default::default())
            .unwrap();
        let report = if jit {
            let mut backend = kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
            let prepared = runtime
                .prepare_native(
                    &loaded_program,
                    &loaded,
                    "main",
                    &mut backend,
                    &context.cancellation,
                )
                .unwrap();
            runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        assert_eq!(report.return_value, Value::I32(13));
        assert_eq!(state.get(), 13);
        assert_eq!(
            *trace.borrow(),
            [
                "make", "first", "second", "rhs", "read", "prepare", "commit", "read"
            ]
        );
    }
}

#[test]
fn source_host_writes_commit_after_rhs_and_preserve_completed_rhs_effects_on_failure() {
    use kagari_common::host_interface::{path::HostPathDeclaration, type_declaration::PathAccess};
    use kagari_runtime::host::{HostError, PreparedHostPathWrite};
    use std::cell::Cell;
    let mut declarations = interface();
    declarations.types[0].path_access = PathAccess::ReadWrite;
    declarations.types[0].fields[0].path_access = PathAccess::ReadWrite;
    declarations.types[0].fields[0].writable = true;
    let mut count =
        HostFieldDeclaration::new(&declarations.types[1].id, "count", HostValueType::I32);
    count.path_access = PathAccess::ReadWrite;
    count.writable = true;
    let path = HostPathDeclaration {
        root: declarations.types[0].id.clone(),
        segments: vec![
            HostPathSegmentDeclaration::Field(declarations.types[0].fields[0].id.clone()),
            HostPathSegmentDeclaration::Field(count.id.clone()),
        ],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    declarations.types[1].fields.push(count);
    declarations.paths.push(path.clone());
    let rhs = HostFunctionDeclaration::new("left.rhs", vec![], HostValueType::I32);
    declarations.functions.push(rhs.clone());
    let engine = KagariEngine::default();
    engine.set_host_interface(declarations.clone()).unwrap();

    for rebind in [false, true] {
        for (op, rhs_value, after_rhs, deleted, expected) in [
            ("=", 2, 100, false, Some(2)),
            ("+=", 2, 100, false, Some(102)),
            ("/=", 0, 100, false, None),
            ("+=", 1, i32::MAX, false, None),
            ("+=", 2, 100, true, None),
        ] {
            let text = if rebind {
                format!(
                    "use left as api; fn main() {{ var target = api::make(); target.related.count {op} if true {{ target = api::make(); api::rhs() }} else {{ 0 }}; }}"
                )
            } else {
                format!(
                    "use left as api; fn main() {{ api::make().related.count {op} api::rhs(); }}"
                )
            };
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("write.kgr", text.clone()),
                    Default::default(),
                )
                .unwrap();
            assert!(
                engine
                    .compile_to_artifact(
                        SourceFile::new("denied.kgr", text),
                        CompileOptions {
                            language_profile: LanguageProfile {
                                allow_path_mutation: false,
                                ..profile
                            }
                        },
                        Default::default()
                    )
                    .is_err()
            );
            for (encoded, jit) in [(false, false), (true, false), (true, true)] {
                let artifact = if encoded {
                    KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
                } else {
                    artifact.clone()
                };
                let context = ExecutionContext {
                    jit_policy: if jit {
                        JitPolicy::Enabled
                    } else {
                        JitPolicy::Disabled
                    },
                    ..Default::default()
                };
                let mut runtime = engine.runtime(context.clone());
                let ids = runtime
                    .register_host_types(
                        declarations.types[..2]
                            .iter()
                            .cloned()
                            .map(|ty| HostTypeRegistration::new(ty, "Object"))
                            .collect(),
                    )
                    .unwrap();
                let root = runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(7), ids[0], HostSchemaEpoch::new(0))
                    .unwrap();
                let replacement = runtime
                    .runtime_mut()
                    .register_host_root(HostObjectId(8), ids[0], HostSchemaEpoch::new(0))
                    .unwrap();
                let trace = Rc::new(RefCell::new(Vec::new()));
                let state = Rc::new(Cell::new(10));
                let removed = Rc::new(Cell::new(false));
                let calls = trace.clone();
                let root_calls = Cell::new(0);
                runtime
                    .register_host_function(HostFunction::new(
                        declarations.functions[0].clone(),
                        move |_, _| {
                            calls.borrow_mut().push("root");
                            let first = root_calls.replace(root_calls.get() + 1) == 0;
                            Ok(Value::HostRoot(if first { root } else { replacement }))
                        },
                    ))
                    .unwrap();
                let (calls, value, absent) = (trace.clone(), state.clone(), removed.clone());
                runtime
                    .register_host_function(HostFunction::new(rhs.clone(), move |_, _| {
                        calls.borrow_mut().push("rhs");
                        value.set(after_rhs);
                        absent.set(deleted);
                        Ok(Value::I32(rhs_value))
                    }))
                    .unwrap();
                let descriptor = runtime.runtime_mut().register_host_path(&path).unwrap();
                let (reads, value, absent) = (trace.clone(), state.clone(), removed.clone());
                let (writes, target) = (trace.clone(), state.clone());
                runtime
                    .runtime_mut()
                    .register_host_path_adapter(
                        descriptor,
                        HostPathAdapter::new()
                            .with_read(move |_, path| {
                                assert_eq!(path.root, root);
                                reads.borrow_mut().push("read");
                                if absent.get() {
                                    return Err(HostError::new("target removed by RHS"));
                                }
                                Ok(Value::I32(value.get()))
                            })
                            .with_prepare_write(move |context, _, record| {
                                writes.borrow_mut().push("prepare");
                                context.runtime().collect_garbage().unwrap();
                                let Value::I32(next) = record.new_value else {
                                    return Err(HostError::new("expected i32"));
                                };
                                let (writes, target) = (writes.clone(), target.clone());
                                Ok(PreparedHostPathWrite::new(move || {
                                    writes.borrow_mut().push("commit");
                                    target.set(next);
                                }))
                            }),
                    )
                    .unwrap();
                let loaded_program = PreparedProgram::from_artifact(
                    artifact,
                    &Default::default(),
                    &Default::default(),
                )
                .unwrap();
                let loaded = runtime
                    .load_program(&loaded_program, Default::default())
                    .unwrap();
                let result = if jit {
                    let mut backend =
                        kagari_codegen_cranelift::CraneliftBackend::for_host().unwrap();
                    let prepared = runtime
                        .prepare_native(
                            &loaded_program,
                            &loaded,
                            "main",
                            &mut backend,
                            &context.cancellation,
                        )
                        .unwrap();
                    runtime.execute_prepared(&loaded, "main", &[], &context, &prepared)
                } else {
                    runtime.execute(&loaded, "main", &[], &context)
                };
                if let Some(expected) = expected {
                    result.unwrap();
                    assert_eq!(state.get(), expected);
                    assert_eq!(
                        *trace.borrow(),
                        if rebind {
                            vec!["root", "root", "rhs", "read", "prepare", "commit"]
                        } else {
                            vec!["root", "rhs", "read", "prepare", "commit"]
                        }
                    );
                    let records = runtime.runtime().host_dirty_paths();
                    assert_eq!(records.len(), 1);
                    assert_eq!(records[0].old_value, Some(Value::I32(after_rhs)));
                    assert_eq!(records[0].new_value, Value::I32(expected));
                } else {
                    assert!(result.is_err());
                    assert_eq!(state.get(), after_rhs);
                    assert_eq!(
                        *trace.borrow(),
                        if rebind {
                            vec!["root", "root", "rhs", "read"]
                        } else {
                            vec!["root", "rhs", "read"]
                        }
                    );
                    assert!(runtime.runtime().host_dirty_paths().is_empty());
                }
                assert_eq!(runtime.runtime().gc().active_roots(), 0);
            }
        }
    }
}
