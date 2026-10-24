//! Manual ownership/allocation probes; no elapsed-time or RSS attribution.
use super::*;
use crate::{
    diagnostics::allocations::{self, Counts},
    frame::types::arguments::TypeArgument,
    module::StructLayoutRef,
    native::installation::NativeInstallation,
};
use kagari_bytecode::instruction::StructId;
use std::slice;

#[derive(Debug)]
struct DomainResult {
    records: usize,
    symbols: usize,
    after_cycles: Counts,
    after_snapshots: Counts,
    after_installation: Counts,
}

fn domain_lifecycle(
    inputs: &[VerifiedProgram],
    retained_installation: bool,
    retain_snapshots: bool,
) -> (DomainResult, Counts) {
    allocations::measure(|| {
        let installation = retained_installation.then(|| NativeInstallation::prepare(&[]).unwrap());
        let mut snapshots = Vec::with_capacity(inputs.len());
        let mut previous_table = None;
        let mut records = 0;
        let mut symbols = 0;
        for input in inputs {
            let independent =
                (!retained_installation).then(|| NativeInstallation::prepare(&[]).unwrap());
            let owner = installation.as_ref().or(independent.as_ref()).unwrap();
            let mut runtime = owner.runtime(Default::default());
            let loaded_program = runtime
                .load_verified_program("domain", input.clone())
                .unwrap();
            let loaded = runtime.select_program(&loaded_program).unwrap();
            let graph = loaded
                .verified_program()
                .modules()
                .iter()
                .map(Arc::downgrade)
                .collect::<Vec<_>>();
            let table = runtime.definition_context().snapshot();
            if let Some(previous) = previous_table {
                assert_eq!(table.id() == previous, retained_installation);
            }
            previous_table = Some(table.id());
            records = table.len();
            symbols = table.symbol_count();
            if retain_snapshots {
                snapshots.push(table);
            } else {
                drop(table);
            }
            drop((loaded, runtime, independent));
            assert!(graph.iter().all(|weak| weak.upgrade().is_none()));
        }
        let after_cycles = allocations::current().unwrap();
        drop(snapshots);
        let after_snapshots = allocations::current().unwrap();
        drop(installation);
        let after_installation = allocations::current().unwrap();
        DomainResult {
            records,
            symbols,
            after_cycles,
            after_snapshots,
            after_installation,
        }
    })
}

#[test]
#[ignore = "manual RL03 allocation/ownership matrix; release, --nocapture"]
fn identity_domain_retention_probe() {
    for count in [1, 16, 128] {
        for distinct in [false, true] {
            let inputs = (0..count)
                .map(|index| {
                    let name = format!("Record{:03}", if distinct { index } else { 0 });
                    VerifiedProgram::new(scoped_program(&name), &DefinitionContext::new().unwrap())
                        .unwrap()
                })
                .collect::<Vec<_>>();
            for retained in [false, true] {
                for snapshots in [false, true] {
                    let (result, total) = domain_lifecycle(&inputs, retained, snapshots);
                    assert_eq!(
                        total.net_bytes, 0,
                        "all installation/snapshot owners dropped"
                    );
                    assert_eq!(result.after_installation.net_bytes, 0);
                    println!(
                        "RL03_DOMAIN count={count} distinct={distinct} retained_installation={retained} snapshots={snapshots} records={} symbols={} after_cycles={:?} after_snapshots={:?} after_installation={:?}",
                        result.records,
                        result.symbols,
                        result.after_cycles,
                        result.after_snapshots,
                        result.after_installation
                    );
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Retain {
    None,
    Layout,
    Type,
    Native,
}

enum Holder {
    None,
    Layout(StructLayoutRef),
    Type(TypeArgument),
    Native(InstalledNativeFunction),
}

#[derive(Debug)]
struct ProvenanceResult {
    modules_registered: usize,
    payloads_alive: usize,
    after_retirement: Counts,
    after_holders: Counts,
}

fn provenance_lifecycle(
    input: &DefinitionMetadata<BytecodeProgram>,
    count: usize,
    retain: Retain,
) -> (ProvenanceResult, Counts) {
    allocations::measure(|| {
        let mut runtime = runtime();
        let current_program = runtime.load_program("provenance", input.clone()).unwrap();
        let mut current = runtime.select_program(&current_program).unwrap();
        let mut holders = Vec::with_capacity(count);
        let mut graphs = Vec::with_capacity(count);
        let mut keys = Vec::with_capacity(count);
        let mut native_owners = Vec::with_capacity(count);
        for _ in 0..count {
            keys.push(current.key());
            graphs.push(
                current
                    .verified_program()
                    .modules()
                    .iter()
                    .map(Arc::downgrade)
                    .collect::<Vec<_>>(),
            );
            let holder = match retain {
                Retain::None => Holder::None,
                Retain::Layout => Holder::Layout(current.struct_layout(StructId::new(0)).unwrap()),
                Retain::Type => {
                    let nominal = Ty::Struct(NominalTy {
                        declaration: current.bytecode.structures[0].declaration,
                        arguments: vec![],
                        associated_types: Default::default(),
                    });
                    Holder::Type(
                        runtime
                            .resolve_type_arguments(&current, slice::from_ref(&nominal))
                            .unwrap()
                            .remove(0),
                    )
                }
                Retain::Native => {
                    let product = product(execute, Arc::default());
                    native_owners.push(Arc::downgrade(&product.owner));
                    // SAFETY: fixed fixture code implements this exact Unit body and ABI.
                    Holder::Native(
                        unsafe { runtime.install_native_function(&current, product) }.unwrap(),
                    )
                }
            };
            holders.push(holder);
            let candidate = runtime
                .stage_reload_program(&current_program, None, input.clone())
                .unwrap();
            runtime.publish_staged_reload(candidate).unwrap();
            current = runtime.select_program(&current_program).unwrap();
        }
        runtime.collect_garbage().unwrap();
        let modules_registered = keys
            .iter()
            .filter(|key| runtime.modules().loaded(**key).is_some())
            .count();
        let payloads_alive = graphs
            .iter()
            .flatten()
            .filter(|weak| weak.upgrade().is_some())
            .count();
        assert_eq!(
            modules_registered,
            if matches!(retain, Retain::Native) {
                count
            } else {
                0
            }
        );
        assert_eq!(
            payloads_alive,
            if matches!(retain, Retain::None) {
                0
            } else {
                count
            }
        );
        let after_retirement = allocations::current().unwrap();
        for holder in &holders {
            match holder {
                Holder::None => {}
                Holder::Layout(layout) => {
                    assert!(layout.layout().fields.is_empty());
                    assert!(runtime.validate_executable_module(layout.module()).is_err());
                }
                Holder::Type(argument) => {
                    let Ty::Struct(nominal) = argument.ty() else {
                        panic!("nominal argument")
                    };
                    assert_eq!(
                        argument
                            .definitions()
                            .resolve(nominal.declaration)
                            .unwrap()
                            .last()
                            .unwrap()
                            .name,
                        "Record"
                    );
                }
                Holder::Native(installed) => assert_eq!(
                    runtime.invoke_native_function(installed).unwrap(),
                    Value::Unit
                ),
            }
        }
        drop(holders);
        runtime.collect_garbage().unwrap();
        assert!(
            keys.iter()
                .all(|key| runtime.modules().loaded(*key).is_none())
        );
        assert!(graphs.iter().flatten().all(|weak| weak.upgrade().is_none()));
        assert!(native_owners.iter().all(|weak| weak.upgrade().is_none()));
        let after_holders = allocations::current().unwrap();
        drop((graphs, keys, native_owners, current, runtime));
        ProvenanceResult {
            modules_registered,
            payloads_alive,
            after_retirement,
            after_holders,
        }
    })
}

#[test]
#[ignore = "manual RL03 type/code ownership matrix; release, --nocapture"]
fn type_only_provenance_retention_probe() {
    let input = scoped_program("Record");
    for count in [1, 16, 128] {
        for retain in [Retain::None, Retain::Layout, Retain::Type, Retain::Native] {
            let (result, total) = provenance_lifecycle(&input, count, retain);
            assert_eq!(
                total.net_bytes, 0,
                "all runtime/type/callable owners dropped"
            );
            println!(
                "RL03_PROVENANCE count={count} retain={retain:?} registered={} payloads={} after_retirement={:?} after_holders={:?} total={total:?}",
                result.modules_registered,
                result.payloads_alive,
                result.after_retirement,
                result.after_holders
            );
        }
    }
}
