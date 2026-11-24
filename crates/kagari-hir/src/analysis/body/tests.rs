use super::*;
use crate::{
    declarations::DeclarationId, tests::test_analysis, typeck::table::CallTarget, types::TypeId,
};
use kagari_common::identity::{DefinitionKind, DefinitionPath, ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

fn owner(
    db: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    file: FileId,
    name: &str,
) -> DefinitionPath {
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let declaration = declarations
        .file(file)
        .unwrap()
        .declarations()
        .iter()
        .find(|d| d.name == name)
        .unwrap();
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("function definition");
    };
    declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .resolve(*id)
        .unwrap()
        .to_path()
}

fn query(
    db: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    owner: &DefinitionPath,
) -> Arc<FunctionAnalysis> {
    db.declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let owner = db.definitions.snapshot().lookup(owner).unwrap();
    db.body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap()
}

#[test]
fn unrelated_applied_bound_errors_stay_in_the_signature_snapshot() {
    let text = "use std::hash::{Hash};\nstruct Key<T: Hash> { val value: T } fn bad(x: Key<f32>) {} fn good() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("application.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, file, "good");
    let result = query(&mut db, &sources, &good);
    assert_eq!(result.eligibility(), BodyEligibility::Unavailable);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert_eq!(
        result
            .signature_snapshot()
            .file(file)
            .unwrap()
            .diagnostics()
            .len(),
        1
    );
}

#[test]
fn selected_body_eligibility_uses_the_current_shared_signatures() {
    let mut sources = SourceDatabase::default();
    for package in ["root", "other"] {
        sources
            .bind_module(
                &format!("mem://{package}"),
                ModuleIdentity {
                    package: PackageId(package.into()),
                    path: vec![],
                },
            )
            .unwrap();
    }
    let root_text = "fn good() -> i32 { 42 }";
    let valid_other = "fn helper() -> i32 { 2 }";
    let root = sources
        .set("mem://root", root_text.into(), SourceLayer::Base)
        .unwrap();
    let other = sources
        .set("mem://other", valid_other.into(), SourceLayer::Base)
        .unwrap();
    let original_source = sources.snapshot();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, root, "good");
    let original = query(&mut db, &sources, &good);
    assert_eq!(original.eligibility(), BodyEligibility::Complete);
    assert!(db.declaration_cache.is_some());
    let original_root = original
        .signature_snapshot()
        .declaration_snapshot()
        .file(root)
        .unwrap();

    let mut results = vec![original.clone()];
    for (text, eligibility) in [
        ("fn helper() -> Missing { 2 }", BodyEligibility::Unavailable),
        (valid_other, BodyEligibility::Complete),
    ] {
        sources
            .set("mem://other", text.into(), SourceLayer::Overlay)
            .unwrap();
        let updated = query(&mut db, &sources, &good);
        let fresh = query(&mut test_analysis(), &sources, &good);
        assert_eq!(updated.eligibility(), eligibility);
        assert_eq!(updated.eligibility(), fresh.eligibility());
        assert_eq!(updated.diagnostics(), fresh.diagnostics());

        assert_eq!(
            updated.signature_snapshot().revision(),
            sources.snapshot().revision()
        );
        let updated_other = updated.signature_snapshot().file(other).unwrap();
        assert_eq!(updated_other.source().text(), text);
        assert_eq!(
            updated_other.diagnostics(),
            fresh
                .signature_snapshot()
                .file(other)
                .unwrap()
                .diagnostics()
        );
        assert!(updated.diagnostics().is_empty());
        updated.type_table().assert_same_source_facts(
            updated.definitions(),
            fresh.type_table(),
            fresh.definitions(),
            updated.lowered().module.nodes.arena(),
            fresh.lowered().module.nodes.arena(),
        );
        let declarations = updated.signature_snapshot().declaration_snapshot();
        let current_root = declarations.file(root).unwrap();
        assert!(std::ptr::eq(
            original_root.lowered(),
            current_root.lowered()
        ));

        let old_request = db
            .body(
                original_source.clone(),
                original.owner(),
                &Default::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(old_request.eligibility(), BodyEligibility::Complete);
        assert_eq!(
            old_request.signature_snapshot().revision(),
            original_source.revision()
        );
        results.push(updated);
    }

    assert_eq!(original.eligibility(), BodyEligibility::Complete);
    assert_eq!(
        original
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        valid_other
    );
    assert_eq!(results[1].eligibility(), BodyEligibility::Unavailable);
    assert_eq!(
        results[1]
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        "fn helper() -> Missing { 2 }"
    );
    assert_eq!(results[2].eligibility(), BodyEligibility::Complete);
}

#[test]
fn single_function_query_does_not_check_or_bind_its_neighbors() {
    let mut sources = SourceDatabase::default();
    let text = "const N: i32 = 41; fn broken(x: i32) -> i32 { val wrong = missing; true } fn good(value: i32) -> i32 { val answer = value + N; answer }";
    let id = sources
        .set("single.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, id, "good");
    let result = query(&mut db, &sources, &good);
    assert_eq!(result.eligibility(), BodyEligibility::Complete);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );

    assert!(
        result
            .declarations()
            .iter()
            .all(|d| d.name != "wrong" && d.name != "x")
    );
    let local = result
        .declarations()
        .iter()
        .find(|d| d.name == "answer")
        .unwrap();
    assert!(matches!(local.id, DeclarationId::Binding(_)));
    let complete = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!complete.file(id).unwrap().result().diagnostics().is_empty());
    for (expr, _) in result.lowered().module.nodes.expressions() {
        if let Some(ty) = result.type_table().expr_type(expr) {
            assert_eq!(
                Some(ty),
                complete
                    .file(id)
                    .unwrap()
                    .result()
                    .facts()
                    .typed
                    .type_table
                    .expr_type(expr)
            );
        }
    }
}

#[test]
fn selected_body_facts_and_bindings_follow_edited_source() {
    let mut sources = SourceDatabase::default();
    let text = include_str!("../../../tests/fixtures/analysis/body_parameter_reuse.kgr");
    let id = sources
        .set("reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, id, "good");
    let first = query(&mut db, &sources, &good);
    let old_local = first
        .declarations()
        .iter()
        .find(|d| d.name == "answer")
        .unwrap()
        .clone();
    let edit = text.replace(
        "{\n    1\n}",
        "{ val shifted: (i32, [i32]) = (10, [20]); missing }",
    );
    assert_ne!(edit, text);
    sources
        .set("reuse.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = query(&mut db, &sources, &good);
    assert!(second.diagnostics().is_empty());
    assert!(second.declarations().get(&old_local.id).is_none());
    assert_eq!(first.declarations().get(&old_local.id), Some(&old_local));
    let fresh = query(&mut test_analysis(), &sources, &good);
    second.type_table().assert_same_source_facts(
        second.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        second.lowered().module.nodes.arena(),
        fresh.lowered().module.nodes.arena(),
    );
    assert_eq!(second.diagnostics(), fresh.diagnostics());
    let old_parameter = first.lowered().module.functions[1].params[0].id;
    let lowered = second.lowered();
    let parameter = lowered.module.functions[1].params[0].id;
    assert_eq!((old_parameter.index(), parameter.index()), (0, 0));
    assert_ne!(old_parameter, parameter);
    let old_binding = ResolvedName::Param(old_parameter);
    let new_binding = ResolvedName::Param(parameter);
    assert!(second.names().binding(&old_binding).is_none());
    assert!(fresh.names().binding(&new_binding).is_none());
    assert_eq!(first.names().binding(&old_binding).unwrap().name, "value");
    assert_eq!(second.names().binding(&new_binding).unwrap().name, "value");
    assert_eq!(lowered.module.parameter(parameter).name, "value");
    assert!(std::panic::catch_unwind(|| lowered.module.parameter(old_parameter)).is_err());
    let span = lowered.module.parameter(parameter).binding_span();
    assert_eq!(span.start, edit.find("value: i32").unwrap());
    assert_eq!(&edit[span.start..span.end], "value");
    let binding = second
        .declarations()
        .iter()
        .find(|item| item.name == "value")
        .unwrap();
    assert_eq!(binding.source_location().unwrap().range, span);

    sources
        .set(
            "reuse.kgr",
            edit.replace("var n: i32", "var n: bool"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = query(&mut db, &sources, &good);
    assert!(!third.diagnostics().is_empty());
    assert_eq!(third.eligibility(), BodyEligibility::Unavailable);
}

#[test]
fn selected_body_reports_constant_prerequisite_failures() {
    let mut sources = SourceDatabase::default();
    let text = "const BAD: i32 = 1 / 0; fn selected() -> i32 { 42 } fn unrelated() { missing; }";
    let id = sources
        .set("member.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let selected = owner(&mut db, &sources, id, "selected");
    let result = query(&mut db, &sources, &selected);

    assert!(matches!(
        result.diagnostics(),
        [diagnostic] if matches!(&diagnostic.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason } if const_name == "BAD" && reason == "integer division by zero")
    ));
}

#[test]
fn selected_call_uses_current_imported_signature() {
    let mut sources = SourceDatabase::default();
    for name in ["dep", "root"] {
        sources
            .bind_module(
                &format!("mem://{name}"),
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
    }
    let dependency = sources
        .set(
            "mem://dep",
            "pub fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::dep; fn value() -> i32 { dep::value() }";
    let root = sources
        .set("mem://root", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let dep_owner = owner(&mut db, &sources, dependency, "value");
    let root_owner = owner(&mut db, &sources, root, "value");
    assert_ne!(dep_owner, root_owner);
    let first = query(&mut db, &sources, &root_owner);
    let check_call = |body: &FunctionAnalysis, return_type| {
        let module = &body.lowered().module;
        let function = &module.functions[body.function().index()];
        let call = module
            .block(function.implementation.script_body().unwrap())
            .tail_expr
            .unwrap();
        let resolution = body.type_table().call_resolution(call).unwrap();
        let CallTarget::SourceFunction(target) = resolution.target else {
            panic!("expected a cross-module source call");
        };
        assert_eq!(
            body.definitions().resolve(target).unwrap().to_path(),
            dep_owner
        );
        assert_eq!(
            resolution.signature.unwrap().return_type,
            TypeId::Builtin(return_type)
        );
    };
    sources
        .set(
            "mem://dep",
            "pub fn value() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = query(&mut db, &sources, &root_owner);
    check_call(&second, BuiltinType::Bool);
    assert!(matches!(
        second.diagnostics(),
        [diagnostic] if matches!(&diagnostic.kind, DiagnosticKind::ReturnTypeMismatch { expected, found, .. } if expected == "i32" && found == "bool")
    ));
    let fresh = query(&mut test_analysis(), &sources, &root_owner);
    second.type_table().assert_same_source_facts(
        second.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        second.lowered().module.nodes.arena(),
        fresh.lowered().module.nodes.arena(),
    );
    assert_eq!(second.diagnostics(), fresh.diagnostics());
    check_call(&first, BuiltinType::I32);
    assert!(first.diagnostics().is_empty());
}

#[test]
fn selected_same_named_impl_bodies_keep_their_own_field_types() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() -> i32 { 1 } struct P { val n: i32 } struct Q { val n: bool } trait Show { fn show(self) -> i32; } impl Show for P { fn show(self) -> i32 { self.n } } impl Show for Q { fn show(self) -> i32 { if self.n { 7 } else { 8 } } }";
    let id = sources
        .set("methods.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let declarations = declarations.file(id).unwrap().declarations();
    for (method, ty) in [
        ("show(self) -> i32 { self.n", BuiltinType::I32),
        ("show(self) -> i32 { if self.n", BuiltinType::Bool),
    ] {
        let start = text.find(method).unwrap();
        let declaration = declarations
            .iter()
            .find(|declaration| {
                declaration.name == "show"
                    && declaration.source_location().unwrap().range.start == start
            })
            .unwrap();
        let DeclarationId::Definition(id) = declaration.id else {
            panic!("method definition");
        };
        let path = declarations.definitions().resolve(id).unwrap().to_path();
        assert_eq!(path.path.first().unwrap().kind, DefinitionKind::Impl);
        let result = query(&mut db, &sources, &path);
        assert_eq!(*result.owner(), id);
        assert!(
            result.diagnostics().is_empty(),
            "{:?}",
            result.diagnostics()
        );
        let table = result.type_table();
        let field_types = result
            .lowered()
            .module
            .nodes
            .expressions()
            .filter(|(expr, _)| table.expr_field(*expr).is_some())
            .map(|(expr, _)| table.expr_type(expr).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(field_types, [TypeId::Builtin(ty)]);
    }
}

#[test]
fn deleted_function_is_only_selectable_in_old_source() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "delete.kgr",
            "fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = test_analysis();
    let value = owner(&mut db, &sources, id, "value");
    let old_source = sources.snapshot();
    let old = query(&mut db, &sources, &value);
    let value = *old.owner();
    sources
        .set(
            "delete.kgr",
            "fn next() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(
        db.body(sources.snapshot(), &value, &Default::default())
            .unwrap()
            .is_none()
    );
    let token = CancellationToken::default();
    token.cancel();
    assert!(db.body(old_source.clone(), &value, &token).is_err());
    let stale = db
        .body(old_source, &value, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(stale.source().revision(), old.source().revision());
    for body in [&old, &stale] {
        assert_eq!(*body.owner(), value);
        assert_eq!(body.eligibility(), BodyEligibility::Complete);
    }
    assert!(
        db.body(sources.snapshot(), &value, &Default::default())
            .unwrap()
            .is_none()
    );
}
