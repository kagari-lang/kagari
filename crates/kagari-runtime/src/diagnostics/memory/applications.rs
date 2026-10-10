//! Distinct executable descriptor owners, with caller inputs kept outside preparation.
use super::{Phases, compare, compile, preparation, provider, snapshot};
use crate::{
    Runtime,
    diagnostics::allocations::{self, Counts},
    execution_metadata::MetadataRoot,
    frame::types::EnvironmentRecord,
    value::Value,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    program::BytecodeProgram,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{scalar::BuiltinType, ty::Ty};

const SOURCE: &str = r#"
    trait Read { fn read(self) -> i32; }
    impl Read for i32 { fn read(self) -> i32 { self } }
    fn relay<T>(value: T) -> T { value }
    trait Forward {
        fn forward<T>(self, value: T) -> T { relay(value) }
        fn checked<T: Read>(self, value: T) -> i32 { value.read() }
    }
    impl Forward for i32 {}
    fn main() -> i32 { val f: Forward = 0; f.checked(7); f.forward(8) }
"#;

fn method_lifecycle(code: &BytecodeProgram, count: usize, prepare: bool) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("memory", code.clone()).unwrap();
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
        let value = runtime
            .make_interface(&loaded, table, Value::I32(0))
            .unwrap();
        let root = runtime.root_value(value).unwrap();
        let Value::Interface(id) = value else {
            panic!("interface fixture")
        };
        let interface = runtime
            .gc
            .interface_snapshot(id)
            .unwrap()
            .interface_type
            .clone();
        let inputs = (1..=count)
            .map(|width| {
                let ty: Ty<DefinitionId> = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]);
                runtime.resolve_type_arguments(&loaded, &[ty]).unwrap()
            })
            .collect::<Vec<_>>();
        let (setup, cold, warm, repeated) = preparation(&runtime, || {
            if prepare {
                for arguments in &inputs {
                    drop(
                        runtime
                            .resolve_interface_method_slot(&value, &interface, slot, arguments)
                            .unwrap(),
                    );
                }
            }
        });
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop((root, inputs, interface, loaded));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().method_applications, 0);
        assert_eq!(runtime.gc.stats().environments, 0);
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

fn shared_lifecycle(code: &BytecodeProgram, count: usize, prepare: bool) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("memory", code.clone()).unwrap();
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
        let inputs = (1..=count)
            .map(|width| {
                let ty: Ty<DefinitionId> = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); width]);
                let arguments = runtime.resolve_type_arguments(&loaded, &[ty]).unwrap();
                runtime
                    .gc
                    .alloc_environment(
                        EnvironmentRecord::new(
                            runtime.definition_context(),
                            body.parameters.clone(),
                            arguments,
                        )
                        .unwrap(),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let roots = runtime
            .root_metadata(
                inputs
                    .iter()
                    .map(|environment| MetadataRoot::Environment(environment.id))
                    .collect(),
            )
            .unwrap();
        let (setup, cold, warm, repeated) = preparation(&runtime, || {
            if prepare {
                for environment in &inputs {
                    runtime
                        .prepare_shared_environment(
                            &loaded,
                            Some(environment.clone()),
                            &owner,
                            target,
                            contract,
                        )
                        .unwrap();
                }
            }
        });
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop((roots, inputs, owner, loaded));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().environments, 0);
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

fn witness_lifecycle(code: &BytecodeProgram, count: usize, prepare: bool) -> (Phases, Counts) {
    allocations::measure(|| {
        let mut runtime = Runtime::default();
        let loaded = runtime.load_program("memory", code.clone()).unwrap();
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
        let inputs = (0..count)
            .map(|_| {
                runtime
                    .gc
                    .alloc_environment(
                        EnvironmentRecord::new(runtime.definition_context(), vec![], vec![])
                            .unwrap(),
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let roots = runtime
            .root_metadata(
                inputs
                    .iter()
                    .map(|environment| MetadataRoot::Environment(environment.id))
                    .collect(),
            )
            .unwrap();
        let (setup, cold, warm, repeated) = preparation(&runtime, || {
            if prepare {
                for environment in &inputs {
                    runtime
                        .bind_operations_in(&loaded, Some(environment.clone()), witnesses)
                        .unwrap();
                }
            }
        });
        let staged = runtime
            .stage_reload_program(&loaded, "memory", code.clone())
            .unwrap();
        drop(runtime.publish_staged_reload(staged).unwrap());
        drop((roots, inputs, loaded));
        runtime.collect_garbage().unwrap();
        assert_eq!(runtime.gc.stats().operation_groups, 0);
        assert_eq!(runtime.gc.stats().environments, 0);
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
    let code = compile(SOURCE, &provider());
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
