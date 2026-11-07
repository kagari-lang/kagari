//! Shared preparation, semantic assertions and explicit snapshot lifetime boundaries.

use std::sync::Arc;

use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_hir::{
    analysis::{AnalysisDatabase, AnalysisSnapshot},
    hir::expr::{ExprKind, literal::LiteralKind},
    typeck::scalar::ScalarValue,
    types::TypeId,
};
use kagari_source::{identity::FileId, source_database::SourceDatabase};
use kagari_stdlib::catalog;
use kagari_types::{
    declaration::module::ModuleDecl,
    scalar::{BuiltinType, IntegerType},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    report::measure,
    workloads::{ROOT, Workload},
};

type Providers = Vec<Arc<DefinitionMetadata<ModuleDecl>>>;

pub struct Setup {
    providers: Providers,
}

impl Setup {
    pub fn initialize() -> Self {
        Self {
            providers: catalog::shared().unwrap(),
        }
    }

    pub fn install(&self, db: &mut AnalysisDatabase) {
        db.set_native_modules(self.providers.clone());
    }
}

pub struct State {
    pub sources: SourceDatabase,
    pub ids: Vec<FileId>,
    pub db: AnalysisDatabase,
}

impl State {
    pub fn new(workload: &Workload) -> Self {
        let (sources, ids) = workload.sources();
        Self {
            sources,
            ids,
            db: AnalysisDatabase::new(ROOT).unwrap(),
        }
    }

    pub fn snapshot(&mut self) -> AnalysisSnapshot {
        self.db
            .snapshot(self.sources.snapshot(), &Default::default())
            .unwrap()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub diagnostics: usize,
    pub user_functions: usize,
    pub provider_functions: usize,
    pub logical_user_modules: usize,
    pub provider_modules: usize,
    pub representative_type: &'static str,
}

pub fn expected_type(signature: bool) -> TypeId {
    TypeId::Builtin(if signature {
        BuiltinType::I64
    } else {
        BuiltinType::I32
    })
}

pub fn verify_snapshot(
    snapshot: &AnalysisSnapshot,
    workload: &Workload,
    ids: &[FileId],
    signature: bool,
) -> Summary {
    let mut summary = Summary {
        diagnostics: 0,
        user_functions: 0,
        provider_functions: 0,
        logical_user_modules: 0,
        provider_modules: 0,
        representative_type: if signature { "i64" } else { "i32" },
    };
    for declaration in snapshot.declaration_snapshot().files() {
        let file = snapshot.file(declaration.source().id()).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let count = file.result().facts().lowered.module.functions.len();
        if ids.contains(&file.source().id()) {
            summary.user_functions += count;
            summary.logical_user_modules += 1;
        } else {
            summary.provider_functions += count;
            summary.provider_modules += 1;
        }
    }
    assert_eq!(summary.user_functions, workload.expected_functions());
    assert_eq!(summary.logical_user_modules, workload.inputs.len());
    assert!(summary.provider_functions > 0);
    assert!(summary.provider_modules > 0);
    let file = snapshot.file(ids[workload.query_index]).unwrap();
    assert_eq!(
        file.type_at(workload.query_offset),
        Some(expected_type(signature))
    );
    if workload.name == "dependency_edit" {
        // Both consumers must see the changed contract; the independent module stays i32.
        let other = snapshot.file(ids[3]).unwrap();
        let offset = workload.inputs[3].text.find("saved;").unwrap();
        assert_eq!(other.type_at(offset), Some(expected_type(signature)));
        let independent = snapshot.file(ids[4]).unwrap();
        let offset = workload.inputs[4].text.find("x + 0").unwrap();
        assert_eq!(independent.type_at(offset), Some(expected_type(false)));
    }
    summary
}

pub fn verify_edit(
    old: &AnalysisSnapshot,
    new: &AnalysisSnapshot,
    workload: &Workload,
    ids: &[FileId],
    signature: bool,
) {
    let id = ids[workload.edit_index];
    let before = old.file(id).unwrap().source();
    let after = new.file(id).unwrap().source();
    assert_eq!(before.text(), workload.inputs[workload.edit_index].text);
    assert_eq!(after.text(), workload.replacement(signature));
    assert_ne!(before.text(), after.text());
    assert_ne!(before.revision(), after.revision());
    assert_ne!(old.revision(), new.revision());
    // These queries deliberately happen after publication of the edited snapshot.
    verify_snapshot(old, workload, ids, false);
    verify_snapshot(new, workload, ids, signature);
    if !signature {
        // Inputs locate the edit; retained lowering and checked scalar facts are the oracle.
        let offset = workload.inputs[workload.edit_index]
            .text
            .bytes()
            .zip(workload.body_edit.bytes())
            .position(|(before, after)| before != after)
            .unwrap();
        let (old_literal, old_value) = if workload.name == "dependency_edit" {
            ("1", 1)
        } else {
            ("0", 0)
        };
        for (snapshot, expected_literal, expected_value) in
            [(old, old_literal, old_value), (new, "9", 9)]
        {
            let facts = snapshot.file(id).unwrap().result().facts();
            let (expr_id, expr) = facts
                .lowered
                .module
                .body
                .expressions()
                .find(|(expr_id, _)| {
                    let span = facts.lowered.source_map.expr_span(*expr_id);
                    span.start == offset && span.end == offset + 1
                })
                .expect("edited literal expression");
            let ExprKind::Literal(literal) = &expr.kind else {
                panic!("edited expression must remain a literal");
            };
            assert_eq!(literal.kind, LiteralKind::Number);
            assert_eq!(literal.text, expected_literal);
            assert_eq!(
                facts.typed.type_table().scalar_value(expr_id),
                Some(&ScalarValue::integer(expected_value, IntegerType::I32).unwrap())
            );
        }
    }
}

pub fn edit_evidence(workload: &Workload, signature: bool) -> Value {
    json!({
        "changed_source": workload.inputs[workload.edit_index].name,
        "effective_edit": true,
        "edit_kind": if signature { "signature" } else { "body" },
        "edit": if signature { "value return i32 -> i64" } else if workload.name == "dependency_edit" { "value literal 1 -> 9" } else { "first x + 0 -> x + 9" },
        "old_type": "i32",
        "new_type": if signature { "i64" } else { "i32" },
        "old_snapshot_valid": true,
    })
}

pub struct Lifecycle {
    pub metrics: [(&'static str, f64); 9],
    pub old: Summary,
    pub new: Summary,
    pub checked_modules: usize,
}

impl Lifecycle {
    pub fn evidence(&self, workload: &Workload) -> Value {
        json!({
            "diagnostics": 0,
            "old": self.old,
            "new": self.new,
            "checked_modules": self.checked_modules,
            "edit": edit_evidence(workload, false),
            "lifetime": "old/new snapshots and checked programs retained together, then all old aliases, all new aliases, analysis database released",
            "source_setup": "source database and shared catalog prepared outside lifecycle measurement; retained through database_released",
            "measurement": "measured_sum_ns sums actual operation blocks, excluding validation and reporting; not contiguous wall time",
            "residual": "extra final live bytes may include retained sources, shared catalog and process facilities; not inherently a leak",
        })
    }
}

pub fn lifecycle(
    workload: &Workload,
    setup: &Setup,
    mut checkpoint: impl FnMut(&'static str),
) -> Lifecycle {
    let mut state = State::new(workload);
    checkpoint("baseline");
    let (_, install) = measure(|| setup.install(&mut state.db));
    let (old, old_ns) = measure(|| state.snapshot());
    let (old_checked, old_gate) = measure(|| old.check_program(state.ids[0], &Default::default()));
    let old_checked = old_checked.unwrap();
    let old_summary = verify_snapshot(&old, workload, &state.ids, false);
    checkpoint("old_snapshot");
    let (_, edit) = measure(|| workload.edit(&mut state.sources, false));
    let (new, new_ns) = measure(|| state.snapshot());
    let (new_checked, new_gate) = measure(|| new.check_program(state.ids[0], &Default::default()));
    let new_checked = new_checked.unwrap();
    let new_summary = verify_snapshot(&new, workload, &state.ids, false);
    verify_edit(&old, &new, workload, &state.ids, false);
    let checked_modules = old_checked.modules().len();
    assert_eq!(checked_modules, new_checked.modules().len());
    checkpoint("both_snapshots");
    let (_, old_drop) = measure(|| drop((old_checked, old)));
    verify_snapshot(&new, workload, &state.ids, false);
    checkpoint("old_released");
    let (_, new_drop) = measure(|| drop((new_checked, new)));
    checkpoint("new_released");
    let (_, database_drop) = measure(|| drop(state.db));
    checkpoint("database_released");
    // No JSON/report allocation precedes the last raw checkpoint. Source backing
    // remains live through that checkpoint, so residual bytes need not be zero.
    Lifecycle {
        metrics: [
            ("install", install),
            ("old_snapshot", old_ns),
            ("old_check_program", old_gate),
            ("source_edit", edit),
            ("new_snapshot", new_ns),
            ("new_check_program", new_gate),
            ("drop_old", old_drop),
            ("drop_new", new_drop),
            ("drop_database", database_drop),
        ],
        old: old_summary,
        new: new_summary,
        checked_modules,
    }
}
