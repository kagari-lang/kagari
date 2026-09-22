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

#[test]
fn reflection_field_navigation_retains_owner_and_survives_errors_and_body_reuse() {
    let text = "struct Left { val value: i32 } struct Right { var value: bool } fn read(a: Left, b: Right) { get_field(a, \"value\"); set_field(b, \"value\", true); } fn bad(a: Left) { set_field(a, \"value\", false); }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("reflection-members.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let profile = crate::LanguageFeatureProfile {
        allow_reflection: true,
        allow_reflection_write: true,
        ..Default::default()
    };
    let first = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let file = first.file(id).unwrap();
    let uses: Vec<_> = text
        .match_indices("\"value\"")
        .map(|(offset, _)| offset + 1)
        .collect();
    let left = file.definition_at(uses[0]).expect("reflection read field");
    let right = file.definition_at(uses[1]).expect("reflection write field");
    assert_ne!(left.id, right.id);
    assert_eq!(left.location.range.start, text.find("value").unwrap());
    assert_eq!(
        right.location.range.start,
        text.find("var value").unwrap() + 4
    );
    assert_eq!(
        file.definition_at(uses[2]),
        Some(left),
        "readonly and mismatch errors retain target"
    );
    assert_eq!(
        file.type_at(uses[0]),
        Some(TypeId::Builtin(crate::types::BuiltinType::String))
    );
    assert_eq!(file.result().diagnostics().len(), 2);
    let edit = format!("// shifted 😀\r\n{text}");
    sources
        .set("reflection-members.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let updated = second.file(id).unwrap();
    assert_eq!(updated.result().facts().typed.reused_bodies, 1);
    for (offset, old) in uses.iter().zip([left, right, left]) {
        let current = updated
            .definition_at(offset + edit.len() - text.len())
            .unwrap();
        assert_eq!(current.id, old.id);
        assert_ne!(current.location.revision, old.location.revision);
        assert_eq!(
            current.location.range.start,
            old.location.range.start + edit.len() - text.len()
        );
    }
    assert_eq!(
        file.definition_at(uses[0]),
        Some(left),
        "old snapshot remains valid"
    );
    let body_edit = edit.replace("false", "true");
    sources
        .set("reflection-members.kgr", body_edit, SourceLayer::Overlay)
        .unwrap();
    let third = db
        .snapshot(sources.snapshot(), profile, &Default::default())
        .unwrap();
    let reused = third.file(id).unwrap();
    assert_eq!(reused.result().facts().typed.reused_bodies, 1);
    for (offset, old) in uses[..2].iter().zip([left, right]) {
        let current = reused
            .definition_at(offset + edit.len() - text.len())
            .unwrap();
        assert_eq!(current.id, old.id);
        assert_eq!(current.location.revision, reused.source().revision());
    }
}

#[test]
fn partial_receiver_arguments_do_not_hide_independent_missing_fields() {
    for (receiver, missing_fields) in [("Box<Missing>", 2), ("Missing", 0)] {
        let text = format!(
            "struct Box<T> {{ val known: i32, val unknown: T }} fn bad(value: {receiver}) {{ value.absent; get_field(value, \"absent\"); value.known; value.unknown; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("partial-member.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(
                sources.snapshot(),
                crate::LanguageFeatureProfile {
                    allow_reflection: true,
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
        let file = snapshot.file(id).unwrap();
        assert_eq!(file.result().diagnostics().iter().filter(|diagnostic| matches!(
            &diagnostic.kind, kagari_common::DiagnosticKind::UnknownName { name } if name == "absent"
        )).count(), missing_fields, "{receiver}: {:?}", file.result().diagnostics());
        if missing_fields != 0 {
            let known = text.rfind("value.known").unwrap() + "value.".len();
            let unknown = text.rfind("value.unknown").unwrap() + "value.".len();
            assert_eq!(
                file.type_at(known),
                Some(TypeId::Builtin(crate::types::BuiltinType::I32))
            );
            assert_eq!(file.type_at(unknown), Some(TypeId::Error));
            assert!(file.definition_at(known).is_some());
            assert!(file.definition_at(unknown).is_some());
        }
        assert_eq!(
            file.type_at(text.rfind("42").unwrap()),
            Some(TypeId::Builtin(crate::types::BuiltinType::I32))
        );
        assert!(snapshot.check_program(id, &Default::default()).is_err());
    }
}

#[test]
fn unresolved_assignment_receivers_preserve_independent_index_facts() {
    for target in [
        "missing[make().value]",
        "missing[make(true).value]",
        "missing.absent[make().value]",
        "missing[make().value].absent[make().value]",
    ] {
        let text = format!(
            "struct Item {{ val value: i32 }} fn make() -> Item {{ Item {{ value: 0 }} }} fn bad() {{ {target} = 1; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("index-recovery.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let file = snapshot.file(id).unwrap();
        assert!(file.result().clone().into_codegen().is_err());
        assert_eq!(
            file.result()
                .diagnostics()
                .iter()
                .filter(|d| matches!(
                    d.kind,
                    kagari_common::DiagnosticKind::CallArityMismatch { .. }
                ))
                .count(),
            usize::from(target.contains("true"))
        );
        for (offset, _) in text.match_indices(".value") {
            let member = offset + 1;
            assert_eq!(
                file.type_at(member),
                Some(TypeId::Builtin(crate::types::BuiltinType::I32)),
                "{target}"
            );
            assert_eq!(file.definition_at(member).unwrap().name, "value");
        }
        assert_eq!(
            file.type_at(text.rfind("42").unwrap()),
            Some(TypeId::Builtin(crate::types::BuiltinType::I32))
        );
    }
}

#[test]
fn readonly_assignments_retain_target_and_contextual_initializer_types() {
    for (setup, target, annotation) in [
        ("", "parameter", "Cell<i32>"),
        (
            "val local: Cell<i32> = Cell { value: 1 };",
            "local",
            "Cell<i32>",
        ),
        (
            "val holder = Holder { cell: Cell { value: 1 } };",
            "holder.cell",
            "Cell<i32>",
        ),
        (
            "val pair: (Cell<i32>, bool) = (Cell { value: 1 }, true);",
            "pair[0]",
            "Cell<i32>",
        ),
    ] {
        let text = format!(
            "struct Cell<T> {{ val value: i32 }} struct Holder {{ val cell: Cell<i32> }} fn bad(parameter: {annotation}) {{ {setup} {target} = Cell {{ value: 2 }}; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("readonly-recovery.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let file = snapshot.file(id).unwrap();
        assert!(file.result().clone().into_codegen().is_err());
        assert_eq!(
            file.result()
                .diagnostics()
                .iter()
                .filter(|d| matches!(
                    d.kind,
                    kagari_common::DiagnosticKind::InvalidAssignmentTarget { .. }
                ))
                .count(),
            1,
            "{target}: {:?}",
            file.result().diagnostics()
        );
        let offset = text.find("= Cell { value: 2 }").unwrap();
        assert_eq!(
            file.result().diagnostics().len(),
            1,
            "{target}: {:?}",
            file.result().diagnostics()
        );
        assert!(
            matches!(file.type_at(offset - 2), Some(TypeId::Struct(nominal)) if nominal.arguments == vec![TypeId::Builtin(crate::types::BuiltinType::I32)]),
            "target query: {target}"
        );
        assert!(
            matches!(file.type_at(offset + 2), Some(TypeId::Struct(nominal)) if nominal.arguments == vec![TypeId::Builtin(crate::types::BuiltinType::I32)]),
            "{target}: {:?}",
            file.result().diagnostics()
        );
        assert_eq!(
            file.type_at(text.rfind("42").unwrap()),
            Some(TypeId::Builtin(crate::types::BuiltinType::I32))
        );
    }
}

#[test]
fn erroneous_array_indexes_retain_element_members_without_accepting_codegen() {
    for index in ["true", "missing", ""] {
        let text = format!(
            "struct Item {{ val value: i32 }} fn bad(items: [Item]) {{ items[{index}].value; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("index-members.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
        let file = snapshot.file(id).unwrap();
        assert!(!file.result().diagnostics().is_empty(), "{index}");
        assert!(snapshot.check_program(id, &Default::default()).is_err());
        let member = text.find(".value").unwrap() + 1;
        assert_eq!(
            file.type_at(member),
            Some(TypeId::Builtin(crate::types::BuiltinType::I32)),
            "{index}"
        );
        let field = file.definition_at(member).expect("known element field");
        assert_eq!(field.location.range.start, text.find("value").unwrap());
        assert!(
            matches!(file.member_receiver_type(member), Some(TypeId::Struct(ty)) if ty.declaration.path.last().unwrap().name == "Item")
        );
        assert_eq!(
            file.type_at(text.rfind("42").unwrap()),
            Some(TypeId::Builtin(crate::types::BuiltinType::I32))
        );
        if index == "true" {
            assert_eq!(file.result().diagnostics().len(), 1);
            assert!(matches!(
                file.result().diagnostics()[0].kind,
                kagari_common::DiagnosticKind::InvalidIndexTarget { .. }
            ));
        }
    }
}

#[test]
fn invalid_write_indexes_preserve_target_context_and_projected_members() {
    for index in ["true", "missing", ""] {
        for suffix in ["", ".nested"] {
            let target = format!("items[{index}]{suffix}");
            let text = format!(
                "struct Cell<T> {{ val value: i32 }} struct Item {{ var nested: Cell<i32> }} fn bad(items: [{}]) {{ {target} = Cell {{ value: 1 }}; }} fn good() -> i32 {{ 42 }}",
                if suffix.is_empty() {
                    "Cell<i32>"
                } else {
                    "Item"
                }
            );
            let mut sources = SourceDatabase::default();
            let id = sources
                .set("write-index.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
            let file = snapshot.file(id).unwrap();
            assert!(!file.result().diagnostics().is_empty());
            assert!(snapshot.check_program(id, &Default::default()).is_err());
            let initializer = text.find("Cell { value").unwrap();
            assert!(
                matches!(file.type_at(initializer), Some(TypeId::Struct(ty)) if ty.arguments == vec![TypeId::Builtin(crate::types::BuiltinType::I32)]),
                "{target}: {:?}",
                file.result().diagnostics()
            );
            if !suffix.is_empty() {
                let member = text.find(".nested").unwrap() + 1;
                assert_eq!(
                    file.definition_at(member).expect("projected field").name,
                    "nested"
                );
                assert!(file.member_receiver_type(member).is_some());
            }
            assert_eq!(
                file.type_at(text.rfind("42").unwrap()),
                Some(TypeId::Builtin(crate::types::BuiltinType::I32))
            );
        }
    }
}
