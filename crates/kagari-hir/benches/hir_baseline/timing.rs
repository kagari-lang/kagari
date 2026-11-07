//! Independent timing routes; this module is not compiled into the allocator-instrumented target.

use std::sync::Arc;

use kagari_hir::{
    analysis::{
        body::FunctionAnalysis, declarations::DeclarationSnapshot, eligibility::BodyEligibility,
    },
    declarations::DeclarationId,
};
use serde_json::{Map, Value, json};

use crate::{
    report::measure,
    scenarios::{
        Setup, State, edit_evidence, expected_type, lifecycle, verify_edit, verify_snapshot,
    },
    workloads::Workload,
};

pub fn suite(workload: &Workload, setup: &Setup) -> Vec<Value> {
    workload
        .scenario_names(false)
        .into_iter()
        .map(|name| match name {
            "staged" => staged(workload, setup),
            "snapshot" | "checked" | "warm" => direct(name, workload, setup),
            "body_edit" | "signature_edit" => edited(name, workload, setup),
            "selected" | "selected_warm" | "selected_signature_edit" => {
                selected(name, workload, setup)
            }
            "lifecycle" => {
                let result = lifecycle(workload, setup, |_| {});
                let mut metrics = result.metrics.to_vec();
                metrics.push((
                    "measured_sum_ns",
                    result.metrics.iter().map(|(_, ns)| ns).sum(),
                ));
                row(name, &metrics, result.evidence(workload))
            }
            _ => unreachable!("fixed route manifest"),
        })
        .collect()
}

fn row(name: &str, metrics: &[(&str, f64)], evidence: Value) -> Value {
    let metrics: Map<String, Value> = metrics
        .iter()
        .map(|(key, ns)| ((*key).into(), json!(ns)))
        .collect();
    json!({"name": name, "metrics_ns": metrics, "evidence": evidence})
}

fn verify_declarations(snapshot: &DeclarationSnapshot) {
    for file in snapshot.files() {
        assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    }
}

fn staged(workload: &Workload, setup: &Setup) -> Value {
    let mut state = State::new(workload);
    let (_, install) = measure(|| setup.install(&mut state.db));
    let (declarations, decl_ns) = measure(|| {
        state
            .db
            .declarations(state.sources.snapshot(), &Default::default())
    });
    let declarations = declarations.unwrap();
    verify_declarations(&declarations);
    let (signatures, sig_ns) = measure(|| {
        state
            .db
            .signatures(state.sources.snapshot(), &Default::default())
    });
    let signatures = signatures.unwrap();
    for declaration in declarations.files() {
        let file = signatures.file(declaration.source().id()).unwrap();
        assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    }
    let (snapshot, snapshot_ns) = measure(|| state.snapshot());
    let summary = verify_snapshot(&snapshot, workload, &state.ids, false);
    let (checked, checked_ns) =
        measure(|| snapshot.check_program(state.ids[0], &Default::default()));
    let checked = checked.unwrap();
    let checked_modules = checked.modules().len();
    let (_, drop_results) = measure(|| drop((checked, snapshot, signatures, declarations)));
    let (_, drop_database) = measure(|| drop(state.db));
    row(
        "staged",
        &[
            ("install", install),
            ("declarations", decl_ns),
            ("signatures_after_declarations", sig_ns),
            ("snapshot_after_signatures", snapshot_ns),
            ("check_program", checked_ns),
            ("drop_results", drop_results),
            ("drop_database", drop_database),
        ],
        json!({"diagnostics": 0, "summary": summary, "checked_modules": checked_modules, "route": "one database: declarations -> signatures -> snapshot -> check_program"}),
    )
}

fn direct(name: &str, workload: &Workload, setup: &Setup) -> Value {
    let mut state = State::new(workload);
    let (_, install) = measure(|| setup.install(&mut state.db));
    let (first, first_ns) = measure(|| state.snapshot());
    let summary = verify_snapshot(&first, workload, &state.ids, false);
    let mut metrics = vec![("install", install)];
    let mut evidence = json!({"diagnostics": 0, "summary": summary});
    if name == "warm" {
        metrics.push(("preparation_snapshot", first_ns));
        let (warm, query_ns) = measure(|| state.snapshot());
        metrics.push(("unchanged_snapshot", query_ns));
        assert_eq!(summary, verify_snapshot(&warm, workload, &state.ids, false));
        assert_eq!(first.revision(), warm.revision());
        evidence["unchanged_source"] = json!(true);
        evidence["user_file_results_reused"] = json!(
            state
                .ids
                .iter()
                .all(|id| Arc::ptr_eq(first.file(*id).unwrap(), warm.file(*id).unwrap()))
        );
        let (_, ns) = measure(|| drop((warm, first)));
        metrics.push(("drop_results", ns));
    } else {
        metrics.push(("snapshot", first_ns));
        if name == "checked" {
            let (checked, ns) = measure(|| first.check_program(state.ids[0], &Default::default()));
            let checked = checked.unwrap();
            metrics.push(("check_program", ns));
            evidence["checked_modules"] = json!(checked.modules().len());
            let (_, ns) = measure(|| drop((checked, first)));
            metrics.push(("drop_results", ns));
        } else {
            let (_, ns) = measure(|| drop(first));
            metrics.push(("drop_results", ns));
        }
    }
    let (_, ns) = measure(|| drop(state.db));
    metrics.push(("drop_database", ns));
    row(name, &metrics, evidence)
}

fn edited(name: &str, workload: &Workload, setup: &Setup) -> Value {
    let signature = name == "signature_edit";
    let mut state = State::new(workload);
    let (_, install) = measure(|| setup.install(&mut state.db));
    let (old, preparation) = measure(|| state.snapshot());
    let old_summary = verify_snapshot(&old, workload, &state.ids, false);
    let (_, edit) = measure(|| workload.edit(&mut state.sources, signature));
    let (new, query) = measure(|| state.snapshot());
    let new_summary = verify_snapshot(&new, workload, &state.ids, signature);
    verify_edit(&old, &new, workload, &state.ids, signature);
    let (checked, gate) = measure(|| new.check_program(state.ids[0], &Default::default()));
    let checked = checked.unwrap();
    let checked_modules = checked.modules().len();
    let (_, drop_results) = measure(|| drop((checked, old, new)));
    let (_, drop_database) = measure(|| drop(state.db));
    row(
        name,
        &[
            ("install", install),
            ("preparation_snapshot", preparation),
            ("source_edit", edit),
            ("edited_snapshot", query),
            ("check_program", gate),
            ("drop_results", drop_results),
            ("drop_database", drop_database),
        ],
        json!({"diagnostics": 0, "old": old_summary, "new": new_summary, "checked_modules": checked_modules, "edit": edit_evidence(workload, signature)}),
    )
}

fn verify_selected(result: &FunctionAnalysis, workload: &Workload, signature: bool) {
    assert_eq!(result.eligibility(), BodyEligibility::Complete);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let signatures = result.signature_snapshot();
    for declaration in signatures.declaration_snapshot().files() {
        let file = signatures.file(declaration.source().id()).unwrap();
        assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    }
    assert_eq!(
        result.type_at(workload.query_offset),
        Some(expected_type(signature))
    );
}

fn construction(result: &FunctionAnalysis) -> Value {
    json!({"checked_bodies": result.checked_bodies(), "reused_bodies": result.reused_bodies()})
}

fn selected(name: &str, workload: &Workload, setup: &Setup) -> Value {
    let mut state = State::new(workload);
    let (_, install) = measure(|| setup.install(&mut state.db));
    let (declarations, preparation) = measure(|| {
        state
            .db
            .declarations(state.sources.snapshot(), &Default::default())
    });
    let declarations = declarations.unwrap();
    verify_declarations(&declarations);
    let (owner, lookup) = measure(|| {
        let declaration = declarations
            .file(state.ids[workload.query_index])
            .unwrap()
            .declarations()
            .iter()
            .find(|declaration| declaration.name == "f0")
            .unwrap();
        let DeclarationId::Definition(owner) = declaration.id else {
            panic!("selected function identity")
        };
        owner
    });
    // Retain only the scoped identity, not an accidental declaration snapshot alias.
    drop(declarations);
    let (first, first_ns) = measure(|| {
        state
            .db
            .body(state.sources.snapshot(), &owner, &Default::default())
    });
    let first = first.unwrap().unwrap();
    verify_selected(&first, workload, false);
    let mut metrics = vec![
        ("install", install),
        ("declarations", preparation),
        ("owner_lookup", lookup),
    ];
    let mut evidence = json!({
        "diagnostics": 0, "representative_type": "i32", "selected_function": "consumer_a::f0",
        "construction_stats": {"first": construction(&first)},
        "construction_stats_scope": "result construction provenance, not work performed by the current query; shared results retain earlier statistics",
        "owner_preparation": "declarations only; no full snapshot before selected queries",
    });
    if name == "selected" {
        metrics.push(("selected_body", first_ns));
    } else {
        metrics.push(("preparation_body", first_ns));
        let signature = name == "selected_signature_edit";
        if signature {
            let (_, edit) = measure(|| workload.edit(&mut state.sources, true));
            metrics.push(("source_edit", edit));
        }
        let (second, ns) = measure(|| {
            state
                .db
                .body(state.sources.snapshot(), &owner, &Default::default())
        });
        let second = second.unwrap().unwrap();
        metrics.push((
            if signature {
                "edited_selected_body"
            } else {
                "unchanged_selected_body"
            },
            ns,
        ));
        verify_selected(&second, workload, signature);
        verify_selected(&first, workload, false);
        evidence["construction_stats"]["second"] = construction(&second);
        evidence["whole_result_reused"] = json!(Arc::ptr_eq(&first, &second));
        if signature {
            let id = state.ids[workload.edit_index];
            let old_signatures = first.signature_snapshot();
            let new_signatures = second.signature_snapshot();
            let before = old_signatures.file(id).unwrap().source();
            let after = new_signatures.file(id).unwrap().source();
            assert_eq!(before.text(), workload.inputs[workload.edit_index].text);
            assert_eq!(after.text(), workload.replacement(true));
            assert_ne!(before.revision(), after.revision());
            assert!(!Arc::ptr_eq(&first, &second));
            evidence["edit"] = edit_evidence(workload, true);
            evidence["representative_type"] = json!("i64");
        } else {
            evidence["unchanged_source"] = json!(true);
        }
    }
    // This full check happens only AFTER all selected-query measurements. It proves
    // provider and neighboring user bodies too, without making the selected path warm.
    let validation = state.snapshot();
    let summary = verify_snapshot(
        &validation,
        workload,
        &state.ids,
        name == "selected_signature_edit",
    );
    let checked = validation
        .check_program(state.ids[0], &Default::default())
        .unwrap();
    evidence["full_validation_after_measurement"] = json!(summary);
    evidence["checked_modules"] = json!(checked.modules().len());
    row(name, &metrics, evidence)
}
