//! Distinct executable descriptor owners, with caller inputs kept outside preparation.
use super::{Phases, PreparationSource, assert_reclaimed, compare, compile, preparation, snapshot};
use crate::execution_metadata::environments::AdmittedEnvironment;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime,
    diagnostics::allocations::{self, Counts},
    execution_metadata::managed::{ProtectedMetadata, sources::MetadataSource},
    frame::types::EnvironmentRecord,
    module::managed::source::RevisionSource,
    value::carriers::RootedValue,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::BytecodeProgram,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_types::{scalar::BuiltinType, ty::Ty};

const SOURCE: &str = include_str!("../../../tests/fixtures/metadata/applications.kgr");

fn method_lifecycle(
    code: &DefinitionMetadata<BytecodeProgram>,
    count: usize,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let program = load_bytecode(&mut runtime, "memory", code.clone()).unwrap();
        let loaded = runtime.select_program(&program).unwrap();
        let (table, definition) = loaded
            .bytecode
            .interface_tables
            .iter()
            .enumerate()
            .find(|(_, table)| {
                table
                    .methods
                    .iter()
                    .any(|method| loaded.definition_name(method.method) == Some("forward"))
            })
            .unwrap();
        let slot = definition
            .methods
            .iter()
            .position(|method| loaded.definition_name(method.method) == Some("forward"))
            .unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let root = runtime
            .make_interface(&loaded, table, &RootedValue::from(0_i32))
            .unwrap();
        let interface = runtime
            .with_metadata_graph(&[MetadataSource::RootedInterface(&root)], |cx, graph, _| {
                let Some(ProtectedMetadata::Interface(value)) = graph.single_root() else {
                    panic!("actual interface fixture");
                };
                Ok(cx.read(value, |value| value.description().interface_type().clone()))
            })
            .unwrap();
        let inputs = (1..=count)
            .map(|width| {
                let ty: Ty = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]);
                runtime.resolve_type_arguments(&loaded, &[ty]).unwrap()
            })
            .collect::<Vec<_>>();
        let mut retained = Vec::with_capacity(count);
        let mut first = true;
        let (setup, cold, warm, repeated) = preparation(&mut runtime, |runtime| {
            if prepare {
                for arguments in &inputs {
                    let prepared = runtime
                        .resolve_interface_method_slot(&root, &interface, slot, arguments)
                        .unwrap();
                    if first {
                        retained.push(prepared);
                    }
                }
            }
            first = false;
        });
        let staged = runtime
            .stage_reload_program(&program, None, code.clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        drop((retained, root, inputs, interface, loaded));
        assert_reclaimed(&mut runtime, baseline);
        let retired = snapshot();
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

fn shared_lifecycle(
    code: &DefinitionMetadata<BytecodeProgram>,
    count: usize,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let program = load_bytecode(&mut runtime, "memory", code.clone()).unwrap();
        let loaded = runtime.select_program(&program).unwrap();
        let (function, module, target, contract) = loaded
            .bytecode
            .functions
            .iter()
            .find_map(|function| {
                function
                    .instructions
                    .iter()
                    .find_map(|instruction| match instruction {
                        BytecodeInstruction::Call {
                            callee:
                                CallTarget::Shared {
                                    module,
                                    target,
                                    contract,
                                },
                            ..
                        } => Some((function, *module, *target, contract)),
                        _ => None,
                    })
            })
            .unwrap();
        let body = function.metadata.semantic.generic.as_ref().unwrap();
        let owner = loaded.member(module).unwrap();
        let selected = runtime
            .select_call_route(&owner.bind_call_route(target).unwrap())
            .unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let input_source = PreparationSource::new(&mut runtime);
        let mut prepared_source = PreparationSource::new(&mut runtime);
        let inputs = (1..=count)
            .map(|width| {
                let ty: Ty = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]);
                let arguments = runtime.resolve_type_arguments(&loaded, &[ty]).unwrap();
                runtime
                    .alloc_environment(
                        input_source.execution,
                        EnvironmentRecord::new(
                            &runtime.definition_context().snapshot(),
                            body.parameters.clone(),
                            arguments,
                        )
                        .unwrap(),
                        &[MetadataSource::Revision {
                            owner: &loaded,
                            source: RevisionSource::Current,
                        }],
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let mut retained: Vec<AdmittedEnvironment> = Vec::with_capacity(count);
        let (setup, cold, warm, repeated) = preparation(&mut runtime, |runtime| {
            prepared_source.run(runtime, |runtime, execution, first| {
                if prepare {
                    for (index, environment) in inputs.iter().enumerate() {
                        let mut protected = vec![
                            MetadataSource::Revision {
                                owner: &loaded,
                                source: RevisionSource::Current,
                            },
                            MetadataSource::Environment(environment.source()),
                        ];
                        if !first {
                            protected.push(MetadataSource::Environment(retained[index].source()));
                        }
                        let prepared = runtime
                            .prepare_selected_shared_environment(
                                execution,
                                &loaded,
                                Some(environment.facts().clone()),
                                &selected,
                                contract,
                                &mut protected,
                            )
                            .unwrap();
                        if first {
                            retained.push(prepared);
                        }
                    }
                }
            });
        });
        let staged = runtime
            .stage_reload_program(&program, None, code.clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        prepared_source.retire(&mut runtime);
        input_source.retire(&mut runtime);
        drop((retained, inputs, selected, owner, loaded));
        assert_reclaimed(&mut runtime, baseline);
        let retired = snapshot();
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

fn witness_lifecycle(
    code: &DefinitionMetadata<BytecodeProgram>,
    count: usize,
    prepare: bool,
) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let program = load_bytecode(&mut runtime, "memory", code.clone()).unwrap();
        let loaded = runtime.select_program(&program).unwrap();
        let witnesses = loaded
            .bytecode
            .functions
            .iter()
            .flat_map(|function| &function.instructions)
            .find_map(|instruction| match instruction {
                BytecodeInstruction::Call {
                    callee: CallTarget::InterfaceMethod { contract, .. },
                    ..
                } if !contract.operations.is_empty() => Some(&contract.operations),
                _ => None,
            })
            .unwrap();
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let input_source = PreparationSource::new(&mut runtime);
        let mut prepared_source = PreparationSource::new(&mut runtime);
        let inputs = (0..count)
            .map(|_| {
                runtime
                    .alloc_environment(
                        input_source.execution,
                        EnvironmentRecord::new(
                            &runtime.definition_context().snapshot(),
                            vec![],
                            vec![],
                        )
                        .unwrap(),
                        &[MetadataSource::Revision {
                            owner: &loaded,
                            source: RevisionSource::Current,
                        }],
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let mut retained: Vec<Vec<MetadataSource<'_>>> = Vec::with_capacity(count);
        let (setup, cold, warm, repeated) = preparation(&mut runtime, |runtime| {
            prepared_source.run(runtime, |runtime, execution, first| {
                if prepare {
                    for (index, environment) in inputs.iter().enumerate() {
                        let mut protected = vec![
                            MetadataSource::Revision {
                                owner: &loaded,
                                source: RevisionSource::Current,
                            },
                            MetadataSource::Environment(environment.source()),
                        ];
                        if !first {
                            protected.extend_from_slice(&retained[index]);
                        }
                        runtime
                            .bind_operations_in(
                                execution,
                                &loaded,
                                Some(environment.facts().clone()),
                                witnesses,
                                &mut protected,
                            )
                            .unwrap();
                        if first {
                            retained.push(
                                protected
                                    .into_iter()
                                    .filter(|source| {
                                        matches!(source, MetadataSource::Operations(_))
                                    })
                                    .collect::<Vec<_>>(),
                            );
                        }
                    }
                }
            });
        });
        let staged = runtime
            .stage_reload_program(&program, None, code.clone())
            .unwrap();
        runtime.publish_staged_reload(staged).unwrap();
        prepared_source.retire(&mut runtime);
        input_source.retire(&mut runtime);
        drop(retained);
        drop((inputs, loaded));
        // The identical replacement owns its current closed witness. Complete
        // object/provider/root baselines detect retained old preparation graphs.
        assert_reclaimed(&mut runtime, baseline);
        let retired = snapshot();
        Phases {
            setup,
            cold,
            warm,
            repeated,
            retired,
        }
    })
}

#[test]
#[ignore = "manual descriptor allocation accounting; run release with --nocapture"]
fn executable_descriptor_retention() {
    let code = compile(SOURCE, None);
    for count in [1, 4, 160] {
        compare(&format!("method applications={count}"), |prepare| {
            method_lifecycle(&code, count, prepare)
        });
        compare(&format!("shared applications={count}"), |prepare| {
            shared_lifecycle(&code, count, prepare)
        });
        compare(&format!("witness scopes={count}"), |prepare| {
            witness_lifecycle(&code, count, prepare)
        });
    }
}
