use super::*;
use crate::{analysis::AnalysisSnapshot, tests::test_analysis};
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
    let invalid = SourceFile::inline_module(
        child.source(),
        "invalid",
        FileId::fresh(),
        kagari_common::span::Span::new(1, 2),
    );
    assert!(matches!(
        db.declarations(
            SourceSnapshot::single_file(Arc::new(invalid)),
            &Default::default()
        ),
        Err(AnalysisError::InvalidSourceRange(_))
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
    db.set_parse_limits(kagari_syntax::parser::ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    let snapshot = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
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
}
