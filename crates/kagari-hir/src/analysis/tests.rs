use super::*;
use crate::{tests::test_analysis, typeck::table::CallTarget, types::TypeId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

#[test]
fn full_body_checks_follow_current_signatures_without_changing_old_facts() {
    let mut sources = SourceDatabase::default();
    let text = "fn a() -> i32 { 1 } fn b() -> i32 { a() }";
    let file = sources
        .set("body.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let token = CancellationToken::default();
    let old = db.snapshot(sources.snapshot(), &token).unwrap();
    let check_call = |snapshot: &AnalysisSnapshot, return_type| {
        let facts = snapshot.file(file).unwrap().result().facts();
        let a = &facts.lowered.module.functions[0];
        let b = &facts.lowered.module.functions[1];
        let call = facts
            .lowered
            .module
            .block(b.implementation.script_body().unwrap())
            .tail_expr
            .unwrap();
        let resolution = facts.typed.type_table.call_resolution(call).unwrap();
        assert_eq!(resolution.target, CallTarget::Function(a.id));
        assert_eq!(
            resolution.signature.unwrap().return_type,
            TypeId::Builtin(return_type)
        );
    };

    sources
        .set(
            "body.kgr",
            text.replace("{ 1 }", "{ 100 + 2 }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let current = db.snapshot(sources.snapshot(), &token).unwrap();
    let fresh = test_analysis()
        .snapshot(sources.snapshot(), &token)
        .unwrap();
    for snapshot in [&current, &fresh] {
        check_call(snapshot, BuiltinType::I32);
        snapshot.check_program(file, &token).unwrap();
    }
    let current_file = current.file(file).unwrap();
    let fresh_file = fresh.file(file).unwrap();
    let current_facts = current_file.result().facts();
    let fresh_facts = fresh_file.result().facts();
    current_facts.typed.type_table.assert_same_source_facts(
        current_file.definitions(),
        &fresh_facts.typed.type_table,
        fresh_file.definitions(),
        current_facts.lowered.module.nodes.arena(),
        fresh_facts.lowered.module.nodes.arena(),
    );

    sources
        .set(
            "body.kgr",
            text.replace("i32 { 1 }", "bool { true }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let broken = db.snapshot(sources.snapshot(), &token).unwrap();
    check_call(&broken, BuiltinType::Bool);
    assert!(matches!(
        broken.file(file).unwrap().result().diagnostics(),
        [diagnostic] if matches!(&diagnostic.kind, DiagnosticKind::ReturnTypeMismatch { expected, found, .. } if expected == "i32" && found == "bool")
    ));
    check_call(&old, BuiltinType::I32);
    assert_eq!(old.file(file).unwrap().source().text(), text);
    old.check_program(file, &token).unwrap();
}

mod arena;
mod associated_const;
mod callable;
mod completion;
mod constructor;
mod gat;
mod generic_type;
mod identity;
mod member;
mod namespace;
mod owner;
mod payload;
mod prelude;
mod scoped_identity;
mod signature;
mod standard_query;
mod statics;
mod trait_catalog;
mod trait_identity;
mod trait_reference;
mod type_application;
mod type_name;
