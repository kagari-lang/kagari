use super::*;
use crate::declarations::DeclarationId;
use kagari_common::{
    identity::DefinitionKind,
    line_index::PositionEncoding,
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn members_keep_module_ownership_and_exact_declaration_locations() {
    let text = "// 中文 😀\r\nenum State { Ready, Running }\r\nstruct Point { var x: i32 }";
    let mut sources = SourceDatabase::default();
    let left = sources
        .set("left.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let right = sources
        .set("right.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let headers = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let snapshot = analyze(&mut db, &sources);
    let a = snapshot.file(left).unwrap();
    let b = snapshot.file(right).unwrap().result().facts();
    assert!(
        a.result().diagnostics().is_empty(),
        "{:?}",
        a.result().diagnostics()
    );
    let facts = a.result().facts();
    let variant = facts.lowered.module.enums[0].variants[0].id;
    let other_variant = b.lowered.module.enums[0].variants[0].id;
    assert_eq!(variant.slot(), other_variant.slot());
    assert_eq!(variant.owner(), other_variant.owner());
    assert_ne!(variant.arena(), other_variant.arena());
    let declaration = facts.declarations.variant(variant).unwrap();
    assert_ne!(
        declaration.id,
        b.declarations.variant(other_variant).unwrap().id
    );
    assert!(b.declarations.variant(variant).is_none());
    assert!(std::panic::catch_unwind(|| b.lowered.module.variant(variant)).is_err());
    assert!(std::panic::catch_unwind(|| b.lowered.source_map.variant_span(variant)).is_err());
    assert_eq!(facts.lowered.module.variant(variant).name, "Ready");
    let offset = text.find("Ready").unwrap();
    assert_eq!(declaration.location.file, left);
    assert_eq!(declaration.location.revision, a.source().revision());
    assert_eq!(
        declaration.location.range,
        kagari_common::Span::new(offset, offset + 5)
    );
    assert_eq!(a.definition_at(offset), Some(declaration));
    assert_eq!(
        headers.file(left).unwrap().member_at(offset),
        Some(declaration)
    );
    let position = a
        .source()
        .position(offset, PositionEncoding::Utf16)
        .unwrap();
    assert_eq!((position.line, position.character), (1, 13));
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("nominal variant")
    };
    assert_eq!(id.path.len(), 2);
    assert_eq!(id.path[0].kind, DefinitionKind::Enum);
    assert_eq!(id.path[0].name, "State");
    assert_eq!(id.path[1].kind, DefinitionKind::Variant);
    assert_eq!(id.path[1].name, "Ready");

    let field = facts.lowered.module.structs[0].fields[0].id;
    assert!(b.declarations.field(field).is_none());
    assert!(b.typed.type_table.field_type(field).is_none());
    assert!(std::panic::catch_unwind(|| b.lowered.module.field(field)).is_err());
    assert!(std::panic::catch_unwind(|| b.lowered.source_map.field_span(field)).is_err());
    assert_eq!(
        a.definition_at(text.find("x:").unwrap()),
        facts.declarations.field(field)
    );
}

#[test]
fn variant_reordering_preserves_nominal_identity_and_retires_raw_ids() {
    let text = "enum State { Ready, Running }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("state.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_file = old.file(file).unwrap();
    let old_facts = old_file.result().facts();
    let old_id = old_facts.lowered.module.enums[0].variants[0].id;
    let old_declaration = old_facts.declarations.variant(old_id).unwrap();
    let edited = "enum State { Running, Ready }";
    sources
        .set("state.kgr", edited.into(), SourceLayer::Overlay)
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_file = new.file(file).unwrap();
    let new_facts = new_file.result().facts();
    let new_id = new_facts.lowered.module.enums[0].variants[1].id;
    let new_declaration = new_facts.declarations.variant(new_id).unwrap();
    assert_ne!(old_id.slot(), new_id.slot());
    assert_ne!(old_id.arena(), new_id.arena());
    assert_eq!(old_declaration.id, new_declaration.id);
    assert_ne!(
        old_declaration.location.revision,
        new_declaration.location.revision
    );
    assert!(new_facts.declarations.variant(old_id).is_none());
    assert!(old_facts.declarations.variant(new_id).is_none());
    assert_eq!(
        old_file.definition_at(text.find("Ready").unwrap()),
        Some(old_declaration)
    );
    assert_eq!(
        new_file.definition_at(edited.find("Ready").unwrap()),
        Some(new_declaration)
    );
}

#[test]
fn duplicate_variants_remain_queryable_but_cannot_reach_codegen() {
    let text = "enum State { Ready, Ready } fn good(value: i32) -> i32 { value }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("duplicate.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let headers = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let header = headers.file(file).unwrap();
    let diagnostic = header
        .diagnostics()
        .iter()
        .find(|d| d.kind.code() == "KG_RESOLVE_DUPLICATE_VARIANT")
        .unwrap();
    let offset = text.rfind("Ready").unwrap();
    assert_eq!(
        diagnostic.span,
        Some(kagari_common::Span::new(offset, offset + 5))
    );
    let first = header.member_at(text.find("Ready").unwrap()).unwrap();
    let second = header.member_at(offset).unwrap();
    assert_ne!(first.id, second.id);
    for (declaration, occurrence) in [(first, 0), (second, 1)] {
        let DeclarationId::Definition(id) = &declaration.id else {
            panic!("nominal variant")
        };
        assert_eq!(id.path[1].occurrence, occurrence);
    }
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis
            .definition_at(text.rfind("value }").unwrap())
            .is_some()
    );
    assert_eq!(analysis.definition_at(offset), Some(second));
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn signature_reuse_rebases_field_keys_into_the_current_arena() {
    let text = "struct Point { var x: i32 } fn read(p: Point) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("field.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_facts = old.file(file).unwrap().result().facts();
    let old_field = old_facts.lowered.module.structs[0].fields[0].id;
    sources
        .set(
            "field.kgr",
            text.replace("{ p.x }", "{ p.x + 1 }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_file = new.file(file).unwrap();
    assert!(new_file.signatures_reused());
    let new_facts = new_file.result().facts();
    let new_field = new_facts.lowered.module.structs[0].fields[0].id;
    assert_ne!(old_field.arena(), new_field.arena());
    assert!(new_facts.typed.type_table.field_type(old_field).is_none());
    assert!(old_facts.typed.type_table.field_type(new_field).is_none());
    assert_eq!(
        new_facts.typed.type_table.field_type(new_field),
        old_facts.typed.type_table.field_type(old_field)
    );
    assert!(new_facts.typed.type_table.field_type(new_field).is_some());
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let fresh = fresh.file(file).unwrap().result().facts();
    new_facts.typed.type_table.assert_same_source_facts(
        &fresh.typed.type_table,
        new_facts.lowered.module.body.arena(),
        fresh.lowered.module.body.arena(),
    );
}
