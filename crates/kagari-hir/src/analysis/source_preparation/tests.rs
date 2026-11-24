use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{AnalysisDatabase, AnalysisSnapshot, external::ExternalModule},
    tests::test_analysis,
};
use kagari_common::span::Span;
use kagari_source::{
    source::SourceExtent,
    source_database::{SourceDatabase, SourceLayer},
};

fn source_preparer(db: &mut AnalysisDatabase) -> SourcePreparer<'_> {
    SourcePreparer {
        cache: &mut db.source_parse_cache,
        inline_ids: &mut db.inline_ids,
        limits: db.parse_limits,
    }
}

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
    let nested = user_files(&first, root)
        .find(|file| {
            file.source()
                .module_identity()
                .path
                .last()
                .is_some_and(|name| name == "b")
        })
        .unwrap();
    let nested_source = nested.source();
    let nested_body = " fn inner() {} ";
    let start = text.find(nested_body).unwrap();
    let range = Span::new(start, start + nested_body.len());
    assert_eq!(physical.source().extent(), SourceExtent::Whole);
    assert_eq!(physical.source().origin_id(), root);
    assert_eq!(
        nested_source.extent(),
        SourceExtent::Inline {
            origin: root,
            range
        }
    );
    assert_ne!(nested_source.id(), child_id);
    assert_eq!(nested_source.span(range), physical.source().span(range));

    assert_eq!(user_files(&first, root).count(), 3);
    assert_ne!(
        physical.syntax().unwrap().location(),
        first_declarations
            .file(child_id)
            .unwrap()
            .syntax()
            .unwrap()
            .location(),
    );
    for file in user_files(&first, root) {
        let parsed = &first_declarations.file(file.source().id()).unwrap().parsed;
        assert!(Arc::ptr_eq(
            parsed.shared_parse().unwrap(),
            physical.parsed.shared_parse().unwrap()
        ));
        assert_eq!(
            file.source().text().as_ptr(),
            physical.source().text().as_ptr()
        );
    }
    let isolated = crate::analyze_source(child.source(), standard_modules()).unwrap();
    assert_eq!(isolated.facts().lowered.module.functions.len(), 2);
    assert!(isolated.into_codegen().is_ok());
    assert_eq!(
        crate::lower::lower_module(child.source())
            .module
            .functions
            .len(),
        2
    );
    let invalid_id = FileId::fresh();
    let invalid = SourceFile::inline_module(child.source(), "invalid", invalid_id, Span::new(1, 2));
    assert!(matches!(
        db.declarations(
            SourceSnapshot::single_file(Arc::new(invalid)),
            &Default::default()
        ),
        Err(AnalysisError::InvalidSourceRange(id)) if id == invalid_id
    ));
    let previous = db.declaration_cache.clone();
    let repeated = prepare(
        source_preparer(&mut db),
        &sources.snapshot(),
        previous.as_ref(),
        &Default::default(),
    )
    .unwrap();
    for (id, prepared) in &repeated {
        let previous = first_declarations.file(*id).unwrap();
        assert!(Arc::ptr_eq(
            prepared.parsed.shared_parse().unwrap(),
            physical.parsed.shared_parse().unwrap()
        ));
        assert!(Arc::ptr_eq(&prepared.lowered, &previous.declared.lowered));
    }
    drop(repeated);
    assert!(Arc::ptr_eq(
        &db.source_parse_cache.files[&root].parsed,
        physical.parsed.shared_parse().unwrap(),
    ));
    let _reference = text.rfind("value(1)").unwrap();

    let changed = format!("// inserted\r\n{}", text.replace("{ x }", "{ x + 2 }"));
    sources
        .set("inline.kgr", changed.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let next = second.file(child_id).unwrap();
    let old_lowered = &child.result().facts().lowered;
    let new_lowered = &next.result().facts().lowered;
    let old_parameter = old_lowered.module.functions[0].params[0].id;
    let parameter = new_lowered.module.functions[0].params[0].id;
    assert_eq!(parameter.index(), 0);
    assert_eq!(new_lowered.module.parameter(parameter).name, "x");
    let old_span = old_lowered.module.parameter(old_parameter).binding_span();
    let span = new_lowered.module.parameter(parameter).binding_span();
    assert_eq!(old_span.start, text.find("x: i32").unwrap());
    assert_eq!(span.start, changed.find("x: i32").unwrap());
    assert_eq!(span.start - old_span.start, "// inserted\r\n".len());
    assert_eq!(&changed[span.start..span.end], "x");
    assert!(std::panic::catch_unwind(|| new_lowered.module.parameter(old_parameter)).is_err());

    assert_ne!(
        child.result().facts().lowered.module.nodes.arena(),
        next.result().facts().lowered.module.nodes.arena()
    );

    let old_parse = first_declarations
        .file(child_id)
        .unwrap()
        .parsed
        .shared_parse()
        .unwrap();
    let new_parse = second
        .declaration_snapshot()
        .file(child_id)
        .unwrap()
        .parsed
        .shared_parse()
        .unwrap();
    assert!(!Arc::ptr_eq(old_parse, new_parse));
    drop(db);

    assert_eq!(child.source().text(), text);
    second
        .check_program(child_id, &Default::default())
        .expect("retained inline closure remains valid");
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
            .unwrap()
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
    let unrestricted_parse = unrestricted
        .file(root)
        .unwrap()
        .parsed
        .shared_parse()
        .unwrap()
        .clone();
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
    assert!(db.source_parse_cache.files.is_empty());
    // An explicitly retained source context must not bypass parser-limit changes.
    let limited = prepare(
        source_preparer(&mut db),
        &sources.snapshot(),
        fork.declaration_cache.as_ref(),
        &Default::default(),
    )
    .unwrap();
    assert!(!Arc::ptr_eq(
        limited[&root].parsed.shared_parse().unwrap(),
        &unrestricted_parse,
    ));
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let limited_parse = snapshot
        .declaration_snapshot()
        .file(root)
        .unwrap()
        .parsed
        .shared_parse()
        .unwrap()
        .clone();
    assert!(!Arc::ptr_eq(&unrestricted_parse, &limited_parse));
    assert!(Arc::ptr_eq(
        fork.declarations(sources.snapshot(), &Default::default())
            .unwrap()
            .file(root)
            .unwrap()
            .parsed
            .shared_parse()
            .unwrap(),
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
    let restored_parse = restored.file(root).unwrap().parsed.shared_parse().unwrap();
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
    let old_a = Arc::downgrade(first.file(a).unwrap().parsed.shared_parse().unwrap());
    let unchanged_b = Arc::downgrade(first.file(b).unwrap().parsed.shared_parse().unwrap());
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

    let manifest = prepare_syntax(
        &mut source_preparer(&mut db),
        &original,
        |_| None,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(manifest.len(), 3);
    assert!(Arc::ptr_eq(&manifest[&a].source, original.file(a).unwrap()));
    assert_eq!(manifest[&child].source.origin_id(), a);
    assert_eq!(
        manifest[&child].parsed.syntax().unwrap().content_range(),
        manifest[&child].source.inline_range(),
    );
    assert!(Arc::ptr_eq(
        &old_a.upgrade().unwrap(),
        manifest[&a].parsed.shared_parse().unwrap(),
    ));
    assert!(Arc::ptr_eq(
        &old_a.upgrade().unwrap(),
        manifest[&child].parsed.shared_parse().unwrap(),
    ));
    assert!(db.declaration_cache.is_none());
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
    let current_a = Arc::downgrade(current.file(a).unwrap().parsed.shared_parse().unwrap());
    assert!(old_a.upgrade().is_none());
    assert!(Arc::ptr_eq(
        &unchanged_b.upgrade().unwrap(),
        current.file(b).unwrap().parsed.shared_parse().unwrap(),
    ));
    drop(current);

    let stale = db.declarations(original, &Default::default()).unwrap();
    assert_eq!(stale.file(a).unwrap().source().text(), "mod child {}");
    let stale_parse = Arc::downgrade(stale.file(a).unwrap().parsed.shared_parse().unwrap());
    drop(stale);
    assert!(stale_parse.upgrade().is_none());
    // Syntax preparation needs no declaration or HIR owner. An older empty
    // input must not prune the independently cached current parses either.
    prepare_syntax(
        &mut source_preparer(&mut db),
        &SourceSnapshot::default(),
        |_| None,
        &Default::default(),
    )
    .unwrap();
    let prepared = prepare_syntax(
        &mut source_preparer(&mut db),
        &current_sources,
        |_| None,
        &Default::default(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        &current_a.upgrade().unwrap(),
        prepared[&a].parsed.shared_parse().unwrap(),
    ));
    assert!(Arc::ptr_eq(
        &unchanged_b.upgrade().unwrap(),
        prepared[&b].parsed.shared_parse().unwrap(),
    ));
    // The returned syntax manifest remains usable without either a semantic
    // owner or the database/cache that produced it.
    db.set_native_modules(Vec::new());
    assert!(current_a.upgrade().is_some());
    assert!(unchanged_b.upgrade().is_some());
    let empty = sources
        .replace_captured(Vec::new(), Default::default(), &Default::default())
        .unwrap();
    prepare_syntax(
        &mut source_preparer(&mut db),
        &empty,
        |_| None,
        &Default::default(),
    )
    .unwrap();
    assert!(db.source_parse_cache.files.is_empty());
    drop(db);
    assert_eq!(prepared[&a].source.text(), "mod changed {}");
    assert_eq!(prepared[&a].parsed.syntax().unwrap().items().count(), 1);
    assert_eq!(prepared[&b].parsed.syntax().unwrap().items().count(), 0);
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
    let mut db = AnalysisDatabase::default();
    let prepared = prepare_syntax(
        &mut source_preparer(&mut db),
        &sources.snapshot(),
        |_| None,
        &Default::default(),
    )
    .unwrap();
    let retained = Arc::downgrade(prepared[&id].parsed.shared_parse().unwrap());
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
        PhysicalParse {
            source: source.clone(),
            limits: db.parse_limits,
            parsed,
        },
    )]);
    let cancel = CancellationToken::default();
    // Cancel after parsing has completed, at the actual cache commit boundary.
    cancel.cancel();
    assert_eq!(
        db.source_parse_cache
            .publish(next.revision(), staged, &cancel),
        Err(Cancelled),
    );
    assert!(abandoned.upgrade().is_none());
    assert!(retained.upgrade().is_some());
    assert!(matches!(
        prepare_syntax(&mut source_preparer(&mut db), &next, |_| None, &cancel),
        Err(AnalysisError::Cancelled),
    ));
    assert!(matches!(
        prepare(source_preparer(&mut db), &next, None, &cancel),
        Err(AnalysisError::Cancelled),
    ));
    assert!(retained.upgrade().is_some());
    let completed = prepare_syntax(
        &mut source_preparer(&mut db),
        &next,
        |_| None,
        &Default::default(),
    )
    .unwrap();
    assert!(retained.upgrade().is_none());
    assert_eq!(completed.len(), 2);
}

#[test]
fn parse_cache_requires_exact_source_provenance_not_just_id_and_revision() {
    let original = Arc::new(SourceFile::new("generated.kgr", "@"));
    let mut db = AnalysisDatabase::default();
    let first = prepare_syntax(
        &mut source_preparer(&mut db),
        &SourceSnapshot::single_file(original.clone()),
        |_| None,
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
    let second = prepare_syntax(
        &mut source_preparer(&mut db),
        &sources,
        |id| first.get(&id).map(|module| &module.parsed),
        &Default::default(),
    )
    .unwrap();
    let old = first[&original.id()].parsed.shared_parse().unwrap();
    let new = second[&original.id()].parsed.shared_parse().unwrap();
    assert!(!Arc::ptr_eq(old, new));
    assert_ne!(old.module().location(), new.module().location());
    let span = new.diagnostics()[0].span.unwrap();
    assert_eq!(new.source().span(span), authored.span(span));
    assert_ne!(new.source().span(span), old.source().span(span));
    let repeated = prepare_syntax(
        &mut source_preparer(&mut db),
        &sources,
        |id| first.get(&id).map(|module| &module.parsed),
        &Default::default(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        new,
        repeated[&original.id()].parsed.shared_parse().unwrap()
    ));
}

#[test]
fn changed_library_sidecar_rebuilds_hir_without_reparsing() {
    let source = Arc::new(SourceFile::new(
        "/vendor/outcome.kgr",
        "enum Outcome { Failed }",
    ));
    let id = source.id();
    let input = ExternalModule::new(source);
    let mut db = AnalysisDatabase::default();
    db.set_external_modules(vec![input.clone()]);
    let cancel = CancellationToken::default();
    let snapshot = db.declarations(SourceSnapshot::default(), &cancel).unwrap();
    let published = snapshot.file(id).unwrap();
    let previous = &published.declared.lowered;
    assert!(!previous.module.enums[0].variants[0].reports_failure);

    let unchanged = db.prepare_external(&cancel).unwrap();
    assert!(Arc::ptr_eq(previous, &unchanged[&id].1));

    let mut changed = input;
    changed
        .enum_failures
        .insert(("Outcome".into(), "Failed".into()));
    // Keep the published candidate to exercise the local reuse rule, not the public
    // setter's broad semantic invalidation.
    db.libraries.set_external(vec![changed]);
    let updated = db.prepare_external(&cancel).unwrap();
    let (parsed, lowered) = &updated[&id];
    assert!(Arc::ptr_eq(
        published.parsed.shared_parse().unwrap(),
        parsed.shared_parse().unwrap(),
    ));
    assert_ne!(previous.module.nodes.arena(), lowered.module.nodes.arena());
    assert!(lowered.module.enums[0].variants[0].reports_failure);
    assert!(!previous.module.enums[0].variants[0].reports_failure);
}
