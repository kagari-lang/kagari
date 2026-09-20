use super::*;
use crate::types::{BuiltinType, TypeId};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

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
        let mut db = AnalysisDatabase::default();
        let snapshot = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
            file.type_at(text.rfind("combined").unwrap()),
            Some(expected),
            "{expression}"
        );
        assert_eq!(
            file.result().diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArrayElementTypeMismatch { .. }
                    | kagari_common::DiagnosticKind::IfBranchTypeMismatch { .. }
                    | kagari_common::DiagnosticKind::MatchArmTypeMismatch { .. }
            )),
            conflict,
            "{expression}"
        );
        assert!(snapshot.check_program(root, &Default::default()).is_err());
    }
}

#[test]
fn recovery_members_do_not_hide_independent_argument_mismatches() {
    for (actual, mismatch) in [
        ("(missing, true)", true),
        ("(missing, 2)", false),
        ("(true, missing)", true),
        ("(1, missing)", false),
    ] {
        let source = SourceFile::new(
            "partial-conflict.kgr",
            format!("fn take(value: (i32, i32)) {{}} fn bad() {{ take({actual}); }}"),
        );
        let analysis = crate::analyze_source(&source, Default::default());
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::ArgumentTypeMismatch { .. }
            )),
            mismatch,
            "{actual}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn indexing_partial_composites_preserves_the_selected_member() {
    for (text, expected, invalid_index) in [
        (
            "fn bad() { (7, missing)[0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: [(i32, Missing)]) { value[0][0]; }",
            TypeId::Builtin(BuiltinType::I32),
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[1]; }",
            TypeId::Error,
            false,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[2]; }",
            TypeId::Error,
            true,
        ),
        (
            "fn bad(value: (i32, Missing)) { value[true]; }",
            TypeId::Error,
            true,
        ),
    ] {
        let source = SourceFile::new("index-recovery.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        let facts = analysis.facts();
        let ty = facts
            .lowered
            .module
            .body
            .expressions()
            .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::Index { .. }))
            .max_by_key(|(id, _)| {
                let span = facts.lowered.source_map.expr_span(*id);
                span.end - span.start
            })
            .and_then(|(id, _)| facts.typed.type_table.expr_type(id));
        assert_eq!(ty, Some(expected), "{text}");
        assert_eq!(
            analysis.diagnostics().iter().any(|d| matches!(
                d.kind,
                kagari_common::DiagnosticKind::InvalidIndexTarget { .. }
            )),
            invalid_index,
            "{text}"
        );
        assert!(analysis.into_codegen().is_err(), "{text}");
    }
}

#[test]
fn composite_annotations_retain_structure_without_authorizing_codegen() {
    for annotation in [
        "(i32, Missing)",
        "[Missing]",
        "Map<i32, Missing>",
        "Cell<Missing>",
    ] {
        for declaration in [
            format!("fn bad(value: {annotation}) {{}}"),
            format!("fn bad() -> {annotation} {{}}"),
            format!("struct Bad {{ val value: {annotation} }}"),
            format!("enum Bad {{ Value({annotation}) }}"),
            format!("fn bad() {{ val value: {annotation} = (); }}"),
            format!("const bad: {annotation} = ();"),
        ] {
            let text =
                format!("struct Cell<T> {{ val value: T }} {declaration} fn good() -> i32 {{ 7 }}");
            let mut sources = SourceDatabase::default();
            let root = sources
                .set("annotations.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let mut db = AnalysisDatabase::default();
            let snapshot = db
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let file = snapshot.file(root).unwrap();
            assert!(!file.result().diagnostics().is_empty(), "{declaration}");
            let ty = file.type_at(text.find(annotation).unwrap()).unwrap();
            assert!(ty.is_unresolved(), "{declaration}: {ty:?}");
            assert!(
                !matches!(ty, TypeId::Error | TypeId::Unknown),
                "{declaration}: {ty:?}"
            );
            assert_eq!(
                file.type_at(text.rfind('7').unwrap()),
                Some(TypeId::Builtin(BuiltinType::I32))
            );
            assert!(
                snapshot.check_program(root, &Default::default()).is_err(),
                "{declaration}"
            );
        }
    }
}

#[test]
fn failed_call_inference_substitutes_error_without_leaking_callee_binders() {
    for argument in ["", "missing"] {
        let text = format!(
            "struct Cell<T> {{ val value: T }} fn wrap<T>(value: T) -> Cell<T> {{ Cell {{ value: value }} }} fn bad() {{ val cell = wrap({argument}); cell.value; }} fn good() -> i32 {{ 7 }}"
        );
        let source = SourceFile::new("recovery.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(!analysis.diagnostics().is_empty());
        let facts = analysis.facts();
        let call = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Call { .. }).then_some(id)
            })
            .unwrap();
        let TypeId::Struct(result) = facts.typed.type_table.expr_type(call).unwrap() else {
            panic!("known nominal return type must survive inference failure");
        };
        assert_eq!(result.arguments, [TypeId::Error]);
        let field = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Field { .. }).then_some(id)
            })
            .unwrap();
        assert_eq!(facts.typed.type_table.expr_type(field), Some(TypeId::Error));
        assert!(facts.typed.type_table.expr_field(field).is_some());
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn partial_call_inference_preserves_known_and_caller_owned_arguments() {
    let source = SourceFile::new(
        "partial.kgr",
        "fn pair<A, B>(a: A, b: B) -> (A, B) { (a, b) } fn bad<T>(value: T) { pair(value); } fn concrete() { pair(1); }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    let facts = analysis.facts();
    let calls: Vec<_> = facts
        .lowered
        .module
        .body
        .expressions()
        .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::Call { .. }))
        .map(|(id, _)| facts.typed.type_table.expr_type(id).unwrap())
        .collect();
    assert_eq!(calls.len(), 2);
    let TypeId::Tuple(arguments) = &calls[0] else {
        panic!("tuple result");
    };
    let TypeId::Generic(parameter) = &arguments[0] else {
        panic!("caller binder");
    };
    assert_eq!(parameter.name, "T");
    assert_eq!(arguments[1], TypeId::Error);
    assert_eq!(
        calls[1],
        TypeId::Tuple(vec![TypeId::Builtin(BuiltinType::I32), TypeId::Error])
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn inference_uses_valid_members_of_partially_erroneous_arguments() {
    for source in [
        "fn identity<A, B>(value: (A, B)) -> (A, B) { value } fn bad() { identity((1, missing)); }",
        "struct Pair<A, B> { val first: A, val second: B } fn identity<A, B>(value: Pair<A, B>) -> Pair<A, B> { value } fn bad() { identity(Pair { first: 1, second: missing }); }",
        "struct Pair<A, B> { val first: A, val second: B } fn identity<A, B>(value: Pair<A, B>) -> Pair<A, B> { value } fn bad(value: Pair<i32, Missing>) { identity(value); }",
    ] {
        let source = SourceFile::new("partial-members.kgr", source);
        let analysis = crate::analyze_source(&source, Default::default());
        let facts = analysis.facts();
        let result = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(expr.kind, crate::hir::ExprKind::Call { .. })
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            })
            .unwrap();
        let arguments = match result {
            TypeId::Tuple(arguments) => arguments,
            TypeId::Struct(ty) => ty.arguments,
            _ => panic!("known composite result"),
        };
        assert_eq!(
            arguments,
            [TypeId::Builtin(BuiltinType::I32), TypeId::Error]
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn incomplete_whole_type_does_not_poison_later_inference() {
    let source = SourceFile::new(
        "later-argument.kgr",
        "fn choose<T>(first: T, second: T) -> T { second } fn bad() { choose((1, missing), (2, true)); }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    let facts = analysis.facts();
    let result = facts
        .lowered
        .module
        .body
        .expressions()
        .find_map(|(id, expr)| {
            matches!(expr.kind, crate::hir::ExprKind::Call { .. })
                .then(|| facts.typed.type_table.expr_type(id))
                .flatten()
        });
    assert_eq!(
        result,
        Some(TypeId::Tuple(vec![
            TypeId::Builtin(BuiltinType::I32),
            TypeId::Builtin(BuiltinType::Bool)
        ]))
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn aggregate_bounds_are_checked_for_annotations_constructors_and_forwarded_parameters() {
    for text in [
        "struct Key<T: HashKey> { val value: T } fn bad(x: Key<f32>) {}",
        "struct Key<T: HashKey> { val value: T } fn bad() { Key { value: 1.5 }; }",
        "enum Key<T: HashKey> { Value(T) } fn bad() { Key::Value(1.5); }",
        "struct Key<T: HashKey> { val value: T } fn bad<T>(value: T) { Key { value: value }; }",
    ] {
        let source = SourceFile::new("bounds.kgr", text);
        let analysis = crate::analyze_source(&source, Default::default());
        assert!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"),
            "{text}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "bounds.kgr",
        "struct Key<T: HashKey> { val value: T } enum Items<T: HashKey> { Values(Set<T>) } fn pass<T: HashKey>(value: T) -> Key<T> { Key { value: value } }",
    );
    let analysis = crate::analyze_source(&source, Default::default());
    assert!(
        analysis.diagnostics().is_empty(),
        "{:?}",
        analysis.diagnostics()
    );
}

#[test]
fn generic_templates_construct_and_project_distinct_instances() {
    let text = "struct Cell<T> { var value: T } enum Packet<T> { Data(T) } fn get<T>(cell: Cell<T>) -> T { cell.value } fn main() -> (i32, String, Packet<i32>) { val a = Cell { value: 7 }; val b = Cell { value: \"hello\" }; a.value = 8; (get(a), get(b), Packet::Data(a.value)) }";
    let source = SourceFile::new("generic.kgr", text);
    let result = crate::analyze_source(&source, Default::default());
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    let structure = facts.aggregates.structures().next().unwrap();
    let enumeration = facts.aggregates.enumerations().next().unwrap();
    assert_eq!(structure.generic_params.len(), 1);
    assert_eq!(enumeration.generic_params.len(), 1);
    assert_ne!(structure.generic_params[0], enumeration.generic_params[0]);
    assert_eq!(
        structure.fields[0].ty,
        TypeId::Generic(structure.generic_params[0].clone())
    );
    let instances = facts
        .lowered
        .module
        .body
        .expressions()
        .filter(|(_, expr)| matches!(expr.kind, crate::hir::ExprKind::StructInit { .. }))
        .map(|(id, _)| facts.typed.type_table.expr_type(id).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(instances.len(), 2);
    assert_ne!(instances[0], instances[1]);
    for (instance, expected) in instances
        .iter()
        .zip([BuiltinType::I32, BuiltinType::String])
    {
        let TypeId::Struct(instance) = instance else {
            panic!("struct instance");
        };
        assert_eq!(instance.arguments, [TypeId::Builtin(expected)]);
        assert_eq!(instance.declaration, structure.id);
    }
}

#[test]
fn generic_type_parameters_navigate_and_rebase_without_losing_their_owner() {
    let text = "fn before() {} struct Cell<T> { val value: T } enum Packet<T> { Data(T) } fn read(x: Cell<i32>) -> i32 { x.value }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("generic.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let parameter = old_file
        .definition_at(text.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(
        parameter.location.range.start,
        text.find("Cell<T").unwrap() + 5
    );
    let payload = old_file
        .definition_at(text.find("Data(T").unwrap() + 5)
        .unwrap();
    assert_ne!(payload.id, parameter.id);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("generic.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());
    let new_parameter = file
        .definition_at(edit.find("value: T").unwrap() + 7)
        .unwrap();
    assert_eq!(new_parameter.id, parameter.id);
    assert_eq!(
        new_parameter.location.range.start,
        edit.find("Cell<T").unwrap() + 5
    );
    assert!(matches!(
        file.type_at(edit.rfind("value").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    ));
}
