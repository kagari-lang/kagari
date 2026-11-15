use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_hir::types::*;
use kagari_source::source_database::*;
use kagari_types::scalar::*;

use kagari_source::diagnostic::DiagnosticKind;

#[test]
fn branch_and_array_merges_recover_complementary_member_facts() {
    for (expression, array, conflict) in [
        (
            "if true { (missing, true) } else { (1, missing) }",
            false,
            false,
        ),
        (
            "match 0 { 0 => (missing, true), _ => (1, missing) }",
            false,
            false,
        ),
        ("[(missing, true), (1, missing)]", true, false),
        ("[(missing, true), (1, missing), (false, true)]", true, true),
    ] {
        let text = format!("fn bad() {{ val combined = {expression}; combined; }}");
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("merge.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(root).unwrap();
        let pair = TypeId::Tuple(vec![
            TypeId::Builtin(BuiltinType::I32),
            TypeId::Builtin(BuiltinType::Bool),
        ]);
        let expected = if array {
            TypeId::Array(Box::new(pair))
        } else {
            pair
        };
        assert_eq!(
            file.queries().type_at(text.rfind("combined").unwrap()),
            Some(expected),
            "{expression}"
        );
        assert_eq!(
            file.result().diagnostics().iter().any(|d| matches!(
                d.kind,
                DiagnosticKind::ArrayElementTypeMismatch { .. }
                    | DiagnosticKind::IfBranchTypeMismatch { .. }
                    | DiagnosticKind::MatchArmTypeMismatch { .. }
            )),
            conflict,
            "{expression}"
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn generic_type_parameters_navigate_and_rebase_without_losing_their_owner() {
    let text = "fn before() {} struct Cell<T> { val value: T } enum Packet<T> { Data(T) } fn read(x: Cell<i32>) -> i32 { x.value }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("generic.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let parameter = old_file
        .queries()
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(
        parameter.location.range.start,
        text.find("Cell<T").unwrap() + 5
    );
    let payload = old_file
        .queries()
        .definition_at(text.find("Data(T").unwrap() + 5)
        .unwrap();
    assert_ne!(payload.id, parameter.id);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("generic.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let new_parameter = file
        .queries()
        .definition_at(edit.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(new_parameter.id, parameter.id);
    assert_eq!(
        new_parameter.location.range.start,
        edit.find("Cell<T").unwrap() + 5
    );
    assert!(matches!(
        file.queries().type_at(edit.rfind("value").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    ));
}
