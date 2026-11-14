use super::*;
use crate::{analysis::AnalysisSnapshot, tests::test_analysis};
use kagari_common::span::Span;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn user_files(
    snapshot: &AnalysisSnapshot,
    origin: FileId,
) -> impl Iterator<Item = &Arc<crate::analysis::FileAnalysis>> {
    snapshot
        .files
        .values()
        .filter(move |file| file.source().origin_id() == origin)
}

#[test]
fn inline_views_share_the_parse_and_rebase_retained_queries() {
    let text = "// 中文😀\r\nmod a { //! Child docs\r\npub fn value(x: i32) -> i32 { x } pub fn call() -> i32 { value(1) } mod b { fn inner() {} } }";
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("inline.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let first_declarations = first.declaration_snapshot();
    let physical = first_declarations.file(root).unwrap();
    let child = user_files(&first, root)
        .find(|file| {
            file.source()
                .module_identity()
                .path
                .last()
                .is_some_and(|name| name == "a")
        })
        .unwrap();
    let child_id = child.source().id();
    assert_eq!(child.syntax().module_documentation(), "Child docs");
    assert_eq!(user_files(&first, root).count(), 3);
    for file in user_files(&first, root) {
        let parsed = &first_declarations.file(file.source().id()).unwrap().parsed;
        assert!(Arc::ptr_eq(&parsed.physical, &physical.parsed.physical));
        assert_eq!(
            file.source().text().as_ptr(),
            physical.source().text().as_ptr()
        );
    }
    let isolated =
        crate::analyze_source(child.source(), kagari_stdlib::catalog::shared().unwrap()).unwrap();
    assert_eq!(isolated.facts().lowered.module.functions.len(), 2);
    assert!(isolated.into_codegen().is_ok());
    assert_eq!(
        crate::lower::lower_module(child.source())
            .module
            .functions
            .len(),
        2
    );
    let invalid =
        SourceFile::inline_module(child.source(), "invalid", FileId::fresh(), Span::new(1, 2));
    assert!(matches!(
        db.declarations(
            SourceSnapshot::single_file(Arc::new(invalid)),
            &Default::default()
        ),
        Err(AnalysisError::InvalidSourceRange(_))
    ));
    let repeated = prepare(
        &db,
        &sources.snapshot(),
        Some(first_declarations),
        &Default::default(),
    )
    .unwrap();
    for (id, prepared) in &repeated {
        let previous = first_declarations.file(*id).unwrap();
        assert!(Arc::ptr_eq(
            &prepared.parsed.physical,
            &physical.parsed.physical
        ));
        assert!(Arc::ptr_eq(&prepared.lowered, &previous.declared.lowered));
    }
    drop(repeated);
    assert!(Arc::ptr_eq(
        &db.source_parse_cache
            .borrow()
            .get(sources.snapshot().file(root).unwrap(), db.parse_limits)
            .unwrap(),
        &physical.parsed.physical,
    ));
    let reference = text.rfind("value(1)").unwrap();
    let target = first.definition_at(root, reference).unwrap().location;
    let changed = format!("// inserted\r\n{}", text.replace("{ x }", "{ x + 2 }"));
    sources
        .set("inline.kgr", changed.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let next = second.file(child_id).unwrap();
    assert!(next.signatures_reused());
    // The nested module is a foreign namespace whose revision changed too.
    // Existing namespace validation conservatively rechecks both bodies.
    assert_eq!(next.result().facts().typed.checked_bodies, 2);
    assert_eq!(next.result().facts().typed.reused_bodies, 0);
    assert_ne!(
        child.result().facts().lowered.module.body.arena(),
        next.result().facts().lowered.module.body.arena()
    );
    assert_eq!(
        second
            .definition_at(root, changed.rfind("value(1)").unwrap())
            .unwrap()
            .location
            .range
            .start,
        target.range.start + "// inserted\r\n".len()
    );
    let old_parse = &first_declarations.file(child_id).unwrap().parsed.physical;
    let new_parse = &second
        .declaration_snapshot()
        .file(child_id)
        .unwrap()
        .parsed
        .physical;
    assert!(!Arc::ptr_eq(old_parse, new_parse));
    drop(db);
    assert_eq!(
        first.definition_at(root, reference).unwrap().location,
        target
    );
    assert_eq!(child.source().text(), text);
    assert!(second.check_program(child_id, &Default::default()).is_ok());
}

#[test]
fn inline_recovery_diagnostics_have_one_owner_and_block_child_codegen() {
    for text in [
        "mod a { pub fn f() -> i32 { 1 + } }",
        "mod a { pub fn f() {}",
        "mod a { mod b { pub fn f() {}",
    ] {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("broken.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let declarations = snapshot.declaration_snapshot();
        let mut expected = declarations
            .file(root)
            .unwrap()
            .parsed
            .physical()
            .diagnostics()
            .to_vec();
        assert!(!expected.is_empty());
        assert!(
            user_files(&snapshot, root).any(|file| file
                .result()
                .facts()
                .lowered
                .module
                .functions
                .iter()
                .any(|f| f.name == "f")),
            "{text}"
        );
        for file in user_files(&snapshot, root) {
            for diagnostic in declarations
                .file(file.source().id())
                .unwrap()
                .parsed
                .diagnostics()
            {
                if matches!(diagnostic.kind, DiagnosticKind::ExpectedBlockEnd) {
                    assert_eq!(file.source().id(), root);
                } else {
                    let range = file.source().inline_range().expect("child recovery owner");
                    let span = diagnostic.span.expect("physical recovery coordinates");
                    assert!(range.start <= span.start && span.end <= range.end);
                    let location = file.source().span(span).unwrap();
                    assert_eq!(location.file, root);
                    assert_eq!(location.revision, file.source().revision());
                }
                let index = expected
                    .iter()
                    .position(|original| original == diagnostic)
                    .expect("unique physical diagnostic occurrence");
                expected.remove(index);
            }
            assert!(file.result().clone().into_codegen().is_err(), "{text}");
            assert!(
                snapshot
                    .check_program(file.source().id(), &Default::default())
                    .is_err(),
                "{text}"
            );
        }
        assert!(expected.is_empty(), "{text}");
    }
}

#[test]
fn inline_recovery_does_not_restart_a_physical_parser_budget() {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set(
            "budget.kgr",
            "mod a { @ fn hidden() {} }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = test_analysis();
    let unrestricted = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let unrestricted_parse = unrestricted.file(root).unwrap().parsed.physical.clone();
    assert!(unrestricted.files().any(|file| {
        file.declared
            .lowered
            .module
            .functions
            .iter()
            .any(|function| function.name == "hidden")
    }));
    let mut fork = db.clone();
    db.set_parse_limits(ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    assert!(db.source_parse_cache.borrow().files.is_empty());
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let limited_parse = snapshot
        .declaration_snapshot()
        .file(root)
        .unwrap()
        .parsed
        .physical
        .clone();
    assert!(!Arc::ptr_eq(&unrestricted_parse, &limited_parse));
    assert!(Arc::ptr_eq(
        &fork
            .declarations(sources.snapshot(), &Default::default())
            .unwrap()
            .file(root)
            .unwrap()
            .parsed
            .physical,
        &unrestricted_parse,
    ));
    assert!(snapshot.check_program(root, &Default::default()).is_err());
    for file in user_files(&snapshot, root) {
        assert!(
            !file
                .result()
                .facts()
                .lowered
                .module
                .functions
                .iter()
                .any(|f| f.name == "hidden")
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
    db.set_parse_limits(Default::default());
    let restored = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let restored_parse = &restored.file(root).unwrap().parsed.physical;
    assert!(!Arc::ptr_eq(restored_parse, &limited_parse));
    assert!(!Arc::ptr_eq(restored_parse, &unrestricted_parse));
    assert!(restored.files().any(|file| {
        file.declared
            .lowered
            .module
            .functions
            .iter()
            .any(|function| function.name == "hidden")
    }));
}

#[test]
fn parse_cache_outlives_lowerings_and_old_requests_cannot_replace_newer_parses() {
    let mut sources = SourceDatabase::default();
    let a = sources
        .set("a.kgr", "mod child {}".into(), SourceLayer::Base)
        .unwrap();
    let b = sources
        .set("b.kgr", String::new(), SourceLayer::Base)
        .unwrap();
    let original = sources.snapshot();
    let mut db = AnalysisDatabase::default();
    let first = db
        .declarations(original.clone(), &Default::default())
        .unwrap();
    let child = first
        .files()
        .find(|file| file.source().origin_id() == a && file.source().id() != a)
        .unwrap()
        .source()
        .id();
    let old_a = Arc::downgrade(&first.file(a).unwrap().parsed.physical);
    let unchanged_b = Arc::downgrade(&first.file(b).unwrap().parsed.physical);
    let declarations = first.files.values().map(Arc::downgrade).collect::<Vec<_>>();
    let lowerings = first
        .files()
        .map(|file| Arc::downgrade(&file.declared.lowered))
        .collect::<Vec<_>>();
    drop(first);
    // A real environment change retires all semantic cache owners, but does not
    // change source parser inputs. The parse cache must keep no lowering alive.
    db.set_native_modules(Vec::new());
    assert!(declarations.iter().all(|owner| owner.upgrade().is_none()));
    assert!(lowerings.iter().all(|owner| owner.upgrade().is_none()));
    assert!(old_a.upgrade().is_some());
    assert!(unchanged_b.upgrade().is_some());

    let manifest = prepare_syntax(&db, &original, &Default::default()).unwrap();
    assert_eq!(manifest.len(), 3);
    assert!(Arc::ptr_eq(&manifest[&a].source, original.file(a).unwrap()));
    assert_eq!(manifest[&child].source.origin_id(), a);
    assert_eq!(
        manifest[&child].parsed.syntax().content_range(),
        manifest[&child].source.inline_range(),
    );
    assert!(Arc::ptr_eq(
        &old_a.upgrade().unwrap(),
        &manifest[&a].parsed.physical,
    ));
    assert!(Arc::ptr_eq(
        &old_a.upgrade().unwrap(),
        &manifest[&child].parsed.physical,
    ));
    assert!(db.declaration_cache.is_none());
    assert!(db.signature_cache.is_none());
    assert!(db.files.is_empty());
    assert!(declarations.iter().all(|owner| owner.upgrade().is_none()));
    assert!(lowerings.iter().all(|owner| owner.upgrade().is_none()));
    drop(manifest);
    sources
        .set("a.kgr", "mod changed {}".into(), SourceLayer::Overlay)
        .unwrap();
    let current_sources = sources.snapshot();
    let current = db
        .declarations(current_sources.clone(), &Default::default())
        .unwrap();
    let current_a = Arc::downgrade(&current.file(a).unwrap().parsed.physical);
    assert!(old_a.upgrade().is_none());
    assert!(Arc::ptr_eq(
        &unchanged_b.upgrade().unwrap(),
        &current.file(b).unwrap().parsed.physical,
    ));
    drop(current);

    let stale = db.declarations(original, &Default::default()).unwrap();
    assert_eq!(stale.file(a).unwrap().source().text(), "mod child {}");
    let stale_parse = Arc::downgrade(&stale.file(a).unwrap().parsed.physical);
    drop(stale);
    assert!(stale_parse.upgrade().is_none());
    // Syntax preparation needs no declaration or HIR owner. An older empty
    // input must not prune the independently cached current parses either.
    prepare_syntax(&db, &SourceSnapshot::default(), &Default::default()).unwrap();
    let prepared = prepare_syntax(&db, &current_sources, &Default::default()).unwrap();
    assert!(Arc::ptr_eq(
        &current_a.upgrade().unwrap(),
        &prepared[&a].parsed.physical,
    ));
    assert!(Arc::ptr_eq(
        &unchanged_b.upgrade().unwrap(),
        &prepared[&b].parsed.physical,
    ));
    // The returned syntax manifest remains usable without either a semantic
    // owner or the database/cache that produced it.
    db.set_native_modules(Vec::new());
    assert!(current_a.upgrade().is_some());
    assert!(unchanged_b.upgrade().is_some());
    let empty = sources
        .replace_captured(Vec::new(), Default::default(), &Default::default())
        .unwrap();
    prepare_syntax(&db, &empty, &Default::default()).unwrap();
    assert!(db.source_parse_cache.borrow().files.is_empty());
    drop(db);
    assert_eq!(prepared[&a].source.text(), "mod changed {}");
    assert_eq!(prepared[&a].parsed.syntax().items().count(), 1);
    assert_eq!(prepared[&b].parsed.syntax().items().count(), 0);
    assert!(current_a.upgrade().is_some());
    assert!(unchanged_b.upgrade().is_some());
    drop(prepared);
    assert!(current_a.upgrade().is_none());
    assert!(unchanged_b.upgrade().is_none());
}

#[test]
fn cancelled_parse_publication_discards_the_staged_batch() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("cancel.kgr", String::new(), SourceLayer::Base)
        .unwrap();
    let db = AnalysisDatabase::default();
    let prepared = prepare_syntax(&db, &sources.snapshot(), &Default::default()).unwrap();
    let retained = Arc::downgrade(&prepared[&id].parsed.physical);
    drop(prepared);
    sources
        .set("cancel.kgr", "mod staged {}".into(), SourceLayer::Overlay)
        .unwrap();
    let next = sources.snapshot();
    let source = next.file(id).unwrap();
    let parsed = Arc::new(parse(source, ParseOptions::default(), &Default::default()).unwrap());
    let abandoned = Arc::downgrade(&parsed);
    let staged = BTreeMap::from([(
        id,
        CachedParse {
            source: source.clone(),
            parsed,
        },
    )]);
    let cancel = CancellationToken::default();
    // Cancel after parsing has completed, at the actual cache commit boundary.
    cancel.cancel();
    assert_eq!(
        db.source_parse_cache.borrow_mut().publish(
            next.revision(),
            db.parse_limits,
            staged,
            &cancel,
        ),
        Err(Cancelled),
    );
    assert!(abandoned.upgrade().is_none());
    assert!(retained.upgrade().is_some());
    assert!(matches!(
        prepare_syntax(&db, &next, &cancel),
        Err(AnalysisError::Cancelled),
    ));
    assert!(matches!(
        prepare(&db, &next, None, &cancel),
        Err(AnalysisError::Cancelled),
    ));
    assert!(retained.upgrade().is_some());
    let completed = prepare_syntax(&db, &next, &Default::default()).unwrap();
    assert!(retained.upgrade().is_none());
    assert_eq!(completed.len(), 2);
}

#[test]
fn parse_cache_requires_exact_source_provenance_not_just_id_and_revision() {
    let original = Arc::new(SourceFile::new("generated.kgr", "@"));
    let db = AnalysisDatabase::default();
    let first = prepare_syntax(
        &db,
        &SourceSnapshot::single_file(original.clone()),
        &Default::default(),
    )
    .unwrap();
    let mut mapped = original.as_ref().clone();
    let authored = Arc::new(SourceFile::new("authored.kgr", "@"));
    let range = Span::new(0, 1);
    mapped.add_copy(range, authored.clone(), range).unwrap();
    assert_eq!(mapped.id(), original.id());
    assert_eq!(mapped.revision(), original.revision());
    let mapped = Arc::new(mapped);
    let sources = SourceSnapshot::single_file(mapped);
    let second = prepare_syntax(&db, &sources, &Default::default()).unwrap();
    let old = &first[&original.id()].parsed.physical;
    let new = &second[&original.id()].parsed.physical;
    assert!(!Arc::ptr_eq(old, new));
    let span = new.diagnostics()[0].span.unwrap();
    assert_eq!(new.source().span(span), authored.span(span));
    assert_ne!(new.source().span(span), old.source().span(span));
    let repeated = prepare_syntax(&db, &sources, &Default::default()).unwrap();
    assert!(Arc::ptr_eq(new, &repeated[&original.id()].parsed.physical));
}
