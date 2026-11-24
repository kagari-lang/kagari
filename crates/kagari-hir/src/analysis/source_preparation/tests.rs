use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{
        AnalysisDatabase, AnalysisSnapshot, FileAnalysis, declarations::DeclarationSnapshot,
        external::ExternalModule,
    },
    resolver::resolved::ResolvedName,
    tests::test_analysis,
};
use kagari_common::span::Span;
use kagari_source::{
    source::SourceExtent,
    source_database::{SourceDatabase, SourceLayer},
};

fn source_preparer(db: &mut AnalysisDatabase) -> SourcePreparer<'_> {
    SourcePreparer {
        inline_ids: &mut db.inline_ids,
        limits: db.parse_limits,
    }
}

fn user_files(
    snapshot: &AnalysisSnapshot,
    origin: FileId,
) -> impl Iterator<Item = &Arc<FileAnalysis>> {
    snapshot
        .files
        .values()
        .filter(move |file| file.source().origin_id() == origin)
}

#[test]
fn inline_views_share_each_requests_parse_and_retain_original_offsets() {
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
        assert!(std::ptr::eq(
            parsed.physical().unwrap(),
            physical.parsed.physical().unwrap()
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

    let declarations = second.declaration_snapshot();
    let physical = declarations.file(root).unwrap().parsed.physical().unwrap();
    for file in user_files(&second, root) {
        let parsed = &declarations.file(file.source().id()).unwrap().parsed;
        assert!(std::ptr::eq(parsed.physical().unwrap(), physical));
    }
    assert!(declarations.file(nested_source.id()).is_some());
    drop(db);

    assert_eq!(child.source().text(), text);
    assert_eq!(next.source().text(), changed);
    for snapshot in [&first, &second] {
        snapshot
            .check_program(child_id, &Default::default())
            .expect("each retained inline closure remains valid");
    }
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
    let has_hidden = |snapshot: &DeclarationSnapshot| {
        snapshot.files().any(|file| {
            file.lowered()
                .module
                .functions
                .iter()
                .any(|function| function.name == "hidden")
        })
    };
    assert!(has_hidden(&unrestricted));
    db.set_parse_limits(ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let declarations = snapshot.declaration_snapshot();
    assert!(!has_hidden(declarations));
    assert!(
        declarations
            .file(root)
            .unwrap()
            .parsed
            .physical_diagnostics()
            .iter()
            .any(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    DiagnosticKind::CompileLimitExceeded {
                        resource: "parser diagnostics",
                        limit: 0
                    }
                )
            })
    );
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
    assert!(has_hidden(&restored));
    assert!(has_hidden(&unrestricted));
    assert!(!has_hidden(declarations));
}

#[test]
fn cancelled_source_preparation_returns_no_partial_result() {
    let source = Arc::new(SourceFile::new("cancel.kgr", "mod child {}"));
    let id = source.id();
    let sources = SourceSnapshot::single_file(source);
    let mut db = AnalysisDatabase::default();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        prepare_syntax(&mut source_preparer(&mut db), &sources, &cancel),
        Err(AnalysisError::Cancelled)
    ));
    assert!(matches!(
        prepare(source_preparer(&mut db), &sources, &cancel),
        Err(AnalysisError::Cancelled)
    ));
    let completed = prepare(source_preparer(&mut db), &sources, &Default::default()).unwrap();
    assert_eq!(completed.len(), 2);
    let child = completed
        .values()
        .find(|file| file.lowered.source.id() != id)
        .unwrap();
    assert_eq!(child.lowered.source.origin_id(), id);
}

#[test]
fn syntax_diagnostics_use_the_supplied_copy_mapping_even_for_the_same_revision() {
    let original = Arc::new(SourceFile::new("generated.kgr", "@"));
    let mut db = AnalysisDatabase::default();
    let first = prepare_syntax(
        &mut source_preparer(&mut db),
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
    let second =
        prepare_syntax(&mut source_preparer(&mut db), &sources, &Default::default()).unwrap();
    let old = first[&original.id()].parsed.physical().unwrap();
    let new = second[&original.id()].parsed.physical().unwrap();
    assert_eq!(old.diagnostics(), new.diagnostics());
    let span = new.diagnostics()[0].span.unwrap();
    assert_eq!(old.source().span(span), original.span(span));
    assert_eq!(new.source().span(span), authored.span(span));
    assert_ne!(new.source().span(span), old.source().span(span));
}

#[test]
fn external_sidecar_changes_reach_current_root_facts_only() {
    let source = Arc::new(SourceFile::new(
        "/vendor/outcome.kgr",
        "enum Outcome { Failed } mod child { enum Outcome { Failed } }",
    ));
    let id = source.id();
    let input = ExternalModule::new(source);
    let mut db = AnalysisDatabase::default();
    db.set_external_modules(vec![input.clone()]);
    let cancel = CancellationToken::default();
    let before = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
    let child = before
        .declaration_snapshot()
        .files()
        .find(|file| file.source().origin_id() == id && file.source().id() != id)
        .unwrap()
        .source()
        .id();
    let mut changed = input;
    changed
        .enum_failures
        .insert(("Outcome".into(), "Failed".into()));
    db.set_external_modules(vec![changed]);
    let after = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
    for (snapshot, expected) in [(&before, false), (&after, true)] {
        for (file, reports_failure) in [(id, expected), (child, false)] {
            let file = snapshot.file(file).unwrap();
            assert!(
                file.result().diagnostics().is_empty(),
                "{:?}",
                file.result().diagnostics()
            );
            let facts = file.result().facts();
            let enumeration = &facts.lowered.module.enums[0];
            assert_eq!(enumeration.variants[0].reports_failure, reports_failure);
            let definition = facts
                .declarations
                .definition(ResolvedName::Enum(enumeration.id))
                .unwrap();
            let checked = facts.aggregates.enumeration(definition).unwrap();
            assert_eq!(checked.variants[0].reports_failure, reports_failure);
        }
        let declarations = snapshot.declaration_snapshot();
        let root = declarations.file(id).unwrap();
        let child = declarations.file(child).unwrap();
        assert!(std::ptr::eq(
            root.parsed.physical().unwrap(),
            child.parsed.physical().unwrap()
        ));
    }
    assert_eq!(
        before.file(id).unwrap().source().revision(),
        after.file(id).unwrap().source().revision()
    );
}
