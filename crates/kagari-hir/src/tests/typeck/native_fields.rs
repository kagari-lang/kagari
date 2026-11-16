use super::*;
use crate::{
    analysis::{AnalysisDatabase, AnalysisSnapshot, external::ExternalModule},
    hir::ids::FieldOwner,
    resolver::resolved::ResolvedName,
    typeck::signature_reuse::reuse_signatures,
};
use kagari_common::identity::{map::DefinitionContext, scoped::DefinitionReferences};
use kagari_source::{identity::FileId, source_database::SourceSnapshot};
use std::sync::Arc;

fn analyze(text: &str) -> (AnalysisSnapshot, FileId) {
    let source = Arc::new(SourceFile::new("/exports/native_fields.kgr", text));
    let id = source.id();
    let mut database = AnalysisDatabase::new("/project").unwrap();
    database.set_external_modules(vec![ExternalModule::new(source)]);
    let snapshot = database
        .snapshot(SourceSnapshot::default(), &Default::default())
        .unwrap();
    (snapshot, id)
}

#[test]
fn native_fields_share_ownership_types_and_assignment_rules_without_storage_authority() {
    let text = include_str!("../../../tests/fixtures/typeck/native_fields.kgr");
    let (snapshot, id) = analyze(text);
    let file = snapshot.file(id).unwrap();
    let facts = file.result().facts();
    let lowered = &facts.lowered;
    assert!(
        lowered
            .module
            .native_types
            .iter()
            .all(|item| item.host.is_none())
    );
    let cell = &lowered.module.native_types[0];
    let field = &cell.fields[0];
    assert_eq!(field.id.owner(), FieldOwner::NativeType(cell.id));
    assert_eq!(field.id.slot(), 0);
    assert!(std::ptr::eq(lowered.module.field(field.id), field));
    let script = &lowered.module.structs[0].fields[0];
    assert_eq!(script.id.slot(), field.id.slot());
    assert_ne!(script.id, field.id);
    assert!(std::ptr::eq(lowered.module.field(script.id), script));
    let (foreign, foreign_id) = analyze(text);
    let foreign = &foreign.file(foreign_id).unwrap().result().facts().lowered;
    assert!(std::panic::catch_unwind(|| foreign.module.field(field.id)).is_err());
    let span = lowered.source_map.field_span(field.id);
    assert_eq!(&text[span.start..span.end], "value");

    let diagnostics = file.result().diagnostics();
    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    assert!(diagnostics.iter().any(|d| matches!(
        &d.kind, DiagnosticKind::InvalidAssignmentTarget { reason } if reason.contains("read-only") || reason.contains("`val`")
    )));
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::AssignmentTypeMismatch { .. }))
    );
    assert!(diagnostics.iter().any(|d| matches!(
        &d.kind, DiagnosticKind::UnknownName { name } if name == "absent"
    )));
    let tail = |name: &str| {
        let function = lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == name)
            .unwrap();
        assert!(facts.typed.valid_bodies.contains(&function.id));
        lowered
            .module
            .block(function.implementation.script_body().unwrap())
            .tail_expr
            .unwrap()
    };
    assert_eq!(
        facts.typed.type_table().expr_type(tail("update")),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(matches!(
        facts.typed.type_table().expr_type(tail("child")),
        Some(TypeId::NativeObject(_))
    ));
    assert!(
        facts
            .typed
            .type_table()
            .expr_field(tail("update"))
            .is_some()
    );

    let signatures = file.signatures();
    assert!(
        signatures.diagnostics().is_empty(),
        "{:?}",
        signatures.diagnostics()
    );
    let declarations = &facts.declarations;
    let catalog = &facts.aggregates;
    let owner = declarations
        .definition(ResolvedName::NativeType(cell.id))
        .unwrap();
    let signature = catalog.native_type(owner).unwrap();
    assert!(signature.host.is_none());
    assert_eq!(signature.fields.len(), 3);
    let checked_field = &signature.fields[0];
    assert_eq!(catalog.field(&checked_field.id), Some(checked_field));
    assert_eq!(
        checked_field.declaration.source_location().unwrap().range,
        span
    );
    let mapped = common::import_facts(
        catalog,
        declarations.definitions(),
        &DefinitionContext::new().unwrap(),
    );
    mapped
        .records()
        .visit_definition_ids(&mut |_| Ok(()), &Default::default())
        .unwrap();
    assert_eq!(
        mapped
            .records()
            .native_types()
            .map(|t| t.fields.len())
            .sum::<usize>(),
        4
    );

    let (changed, changed_id) = analyze(&text.replace("cell.value\n", "cell.value + 1\n"));
    let changed = &changed.file(changed_id).unwrap().result().facts().lowered;
    let reused = reuse_signatures(lowered, signatures, changed, &Default::default()).unwrap();
    let new_field = changed.module.native_types[0].fields[0].id;
    assert_ne!(field.id.arena(), new_field.arena());
    assert!(reused.facts().type_table().field_type(field.id).is_none());
    assert_eq!(
        reused.facts().type_table().field_type(new_field),
        signatures.facts().type_table().field_type(field.id)
    );
}
