use crate::{
    collections::{
        hash::HashPayload,
        map::{MapStorage, ScriptMap},
    },
    modules,
};
use kagari_compiler::{bytecode::lower_program_to_bytecode, source::program::lower_program_to_mir};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    native::{
        conversion::context::ConversionContext,
        module::NativeModule,
        storage::{
            NativePayload,
            access::EditPlan,
            scan::{ScanBudget, ScanStatus},
        },
        typed::NativeContext,
    },
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::Arc;

#[test]
fn indexed_hash_relocation_and_partial_preparation_preserve_scan_and_charge() {
    let modules = modules().unwrap();
    let mut analysis = AnalysisDatabase::default();
    analysis.set_native_modules(
        modules
            .iter()
            .map(|module| Arc::new(module.to_declaration()))
            .collect(),
    );
    let mut sources = SourceDatabase::default();
    let root = sources.set("hash-storage.kgr", "use std::collections::HashMap; pub fn evidence() -> HashMap<String, String> { HashMap::new() }".into(), SourceLayer::Base).unwrap();
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(&checked, &Default::default()).unwrap();
    let program = lower_program_to_bytecode(&mir, &Default::default())
        .unwrap()
        .into_unverified();
    let mut runtime = Runtime::default();
    NativeModule::install_all(&modules, &mut runtime).unwrap();
    let program = runtime.load_program("hash-storage", program).unwrap();
    let owner = runtime.select_program(&program).unwrap();
    let mut cx = NativeContext::new(&runtime, &owner).unwrap();
    let mut conversion = ConversionContext::new(&runtime, &owner).unwrap();
    let applied = conversion.type_for::<ScriptMap<String, String>>().unwrap();
    let ty = conversion.native_type::<MapStorage>(&applied).unwrap();
    let payload = MapStorage::empty(&cx, &ty).unwrap();
    let object = ty.create(&mut cx, payload, &[]).unwrap();
    let argument = conversion.type_for::<String>().unwrap();
    let value = conversion
        .encode_value(&argument, "edge".to_owned())
        .unwrap();
    // SAFETY: Checked publication precedes each slot write; this test directly
    // exercises the provider's backing/accounting owner without invoking Hash/Eq.
    unsafe {
        object
            .edit(
                &mut cx,
                None,
                EditPlan {
                    max_logical_units: 3,
                    structural: true,
                },
                |payload, access| {
                    let table = payload.table_mut();
                    let key = access.publish(&table.key, &value)?;
                    let value = access.publish(table.value.as_ref().unwrap(), &value)?;
                    for hash in [0, 1, 0] {
                        table.reserve(hash)?;
                        access.commit_structure();
                        table.insert(hash, key, value);
                    }
                    Ok(())
                },
            )
            .unwrap();
        let mut continuation = object
            .read(&cx, None, |payload, _| {
                let mut state = payload.begin_scan();
                let mut budget = ScanBudget::new(1);
                assert_eq!(
                    payload.scan(&mut state, &mut budget, &mut |_| {}),
                    ScanStatus::Pending
                );
                Ok(state)
            })
            .unwrap();
        object
            .edit(
                &mut cx,
                None,
                EditPlan {
                    max_logical_units: 3,
                    structural: true,
                },
                |payload, access| {
                    access.commit_structure();
                    payload.table_mut().remove(0, access);
                    Ok(())
                },
            )
            .unwrap();
        object
            .read(&cx, None, |payload, _| {
                let table = payload.table();
                assert_eq!(table.buckets[&0], 0);
                assert_eq!(table.buckets[&1], 1);
                let mut remaining_edges = 0;
                loop {
                    let mut budget = ScanBudget::new(1);
                    let status = payload.scan(&mut continuation, &mut budget, &mut |_| {
                        remaining_edges += 1
                    });
                    assert_eq!(budget.spent(), 1);
                    if status == ScanStatus::Complete {
                        break;
                    }
                }
                // The relocated key is behind the captured frontier; removal's
                // republication owns that edge, without rewinding the continuation.
                assert_eq!(remaining_edges, 3);
                Ok(())
            })
            .unwrap();
        let before = runtime.gc().stats();
        let charge = object
            .read(&cx, None, |payload, _| Ok(payload.charge()))
            .unwrap();
        let error = object
            .edit(
                &mut cx,
                None,
                EditPlan {
                    max_logical_units: 2,
                    structural: false,
                },
                |payload, _| {
                    payload.table_mut().slots.try_reserve(4096).unwrap();
                    Err::<(), _>(RuntimeError::resource_limit("later hash preparation"))
                },
            )
            .unwrap_err();
        assert!(error.message().contains("later hash preparation"));
        let after = runtime.gc().stats();
        let next = object
            .read(&cx, None, |payload, _| Ok(payload.charge()))
            .unwrap();
        assert_eq!(after.current_heap_units, before.current_heap_units);
        assert!(next.retained_bytes > charge.retained_bytes);
        assert_eq!(
            after.current_retained_bytes - before.current_retained_bytes,
            (next.retained_bytes - charge.retained_bytes) as u128
        );
        object
            .read(&cx, None, |payload, _| {
                let mut state = payload.begin_scan();
                let mut edges = 0;
                let mut steps = 0;
                loop {
                    let mut budget = ScanBudget::new(1);
                    let status = payload.scan(&mut state, &mut budget, &mut |_| edges += 1);
                    assert_eq!(budget.spent(), 1);
                    steps += 1;
                    if status == ScanStatus::Complete {
                        break;
                    }
                }
                assert_eq!(edges, 4);
                assert_eq!(steps, 4, "each key and value consumes scan work");
                Ok(())
            })
            .unwrap();
        object
            .edit(
                &mut cx,
                None,
                EditPlan {
                    max_logical_units: 2,
                    structural: true,
                },
                |payload, access| {
                    access.commit_structure();
                    payload.table_mut().clear();
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            object
                .read(&cx, None, |payload, _| Ok(payload.charge().logical_units))
                .unwrap(),
            0
        );
        // Exercise every removal position while the final dense entry is a
        // head, tail, neighbor, singleton, or member of a different chain.
        for removed in 0..8 {
            object
                .edit(
                    &mut cx,
                    None,
                    EditPlan {
                        max_logical_units: 8,
                        structural: true,
                    },
                    |payload, access| {
                        let table = payload.table_mut();
                        table.clear();
                        let key = access.publish(&table.key, &value)?;
                        let item = access.publish(table.value.as_ref().unwrap(), &value)?;
                        for hash in [0, 0, 1, 0, 2, 1, 3, 0] {
                            table.reserve(hash)?;
                            table.insert(hash, key, item);
                        }
                        access.commit_structure();
                        table.remove(removed, access);
                        for hash in 0..4 {
                            let mut expected: Vec<_> = [0, 0, 1, 0, 2, 1, 3, 0]
                                .into_iter()
                                .enumerate()
                                .filter_map(|(index, entry_hash)| {
                                    (entry_hash == hash).then_some(index)
                                })
                                .collect();
                            if let Some(position) =
                                expected.iter().position(|&index| index == removed)
                            {
                                expected.swap_remove(position);
                            }
                            for index in &mut expected {
                                if *index == 7 {
                                    *index = removed;
                                }
                            }
                            let mut actual = Vec::new();
                            let mut position = table.buckets.get(&hash).copied();
                            while let Some(slot) = position {
                                actual.push(slot);
                                position = table.next(hash, slot);
                            }
                            assert_eq!(actual, expected, "collision callback visitation order");
                        }
                        while !table.slots.is_empty() {
                            let mut visited = vec![false; table.slots.len()];
                            for (&hash, &head) in &table.buckets {
                                let mut position = Some(head);
                                while let Some(slot) = position {
                                    assert!(
                                        !visited[slot],
                                        "collision chain must visit each live entry once"
                                    );
                                    visited[slot] = true;
                                    assert_eq!(table.slots[slot].hash, hash);
                                    assert_eq!(table.slots[table.slots[slot].next].previous, slot);
                                    position = table.next(hash, slot);
                                }
                            }
                            assert!(visited.into_iter().all(|visited| visited));
                            table.remove(table.slots.len() / 2, access);
                        }
                        assert!(table.buckets.is_empty());
                        Ok(())
                    },
                )
                .unwrap();
        }
    }
}
