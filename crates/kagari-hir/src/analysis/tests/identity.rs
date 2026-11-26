use crate::analysis::*;
use crate::{analysis::core_source::packaged_modules, tests::standard::standard_modules};
use crate::{collections::HashSet, declarations::DeclarationId, tests::test_analysis};
use kagari_common::{
    identity::{DefinitionKind, MAX_IDENTITY_PATH_SEGMENTS, ModuleIdentity, PackageId},
    span::Span,
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{declaration::names::NameNamespace, language::binding};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn oversized_source_module_identities_are_rejected_without_panicking() {
    let text = "fn run(value: i32) -> i32 { val saved = value; saved }";
    let mut source = SourceFile::new("deep.kgr", text);
    for _ in 0..MAX_IDENTITY_PATH_SEGMENTS {
        source = SourceFile::inline_module(
            &source,
            "nested",
            SourceFile::new("child.kgr", "").id(),
            Span::new(0, text.len()),
        );
    }
    assert!(
        crate::analyze_source(&source, standard_modules())
            .expect("installed declaration analysis")
            .into_codegen()
            .is_ok()
    );
    source = SourceFile::inline_module(
        &source,
        "overflow",
        SourceFile::new("child.kgr", "").id(),
        Span::new(0, text.len()),
    );
    let analyzed =
        crate::analyze_source(&source, standard_modules()).expect("installed declaration analysis");
    assert!(analyzed.diagnostics().iter().any(|diagnostic| matches!(
        diagnostic.kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "module identity path segments",
            limit: 64
        }
    )));
    assert!(analyzed.into_codegen().is_err());
    let mut database = SourceDatabase::default();
    assert!(
        database
            .bind_module("deep.kgr", source.module_identity().clone())
            .is_err()
    );
}

#[test]
fn single_source_analysis_uses_installed_declarations_without_replacing_source_identity() {
    let mut sources = SourceDatabase::default();
    let module = ModuleIdentity {
        package: PackageId("application".into()),
        path: vec!["logic".into()],
    };
    let id = sources.bind_module("source.kgr", module.clone()).unwrap();
    sources
        .set("source.kgr", "fn previous() {}".into(), SourceLayer::Base)
        .unwrap();
    sources.set("source.kgr", "use core::option::Option as Maybe; fn identity(value: Maybe<i32>) -> Maybe<i32> { value }".into(), SourceLayer::Overlay).unwrap();
    let files = sources.snapshot();
    let source = files.file(id).unwrap();
    let result =
        crate::analyze_source(source, standard_modules()).expect("installed declaration analysis");
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    assert_eq!(facts.lowered.source.id(), id);
    assert_eq!(facts.lowered.source.revision(), source.revision());
    assert_eq!(facts.lowered.source.module_identity(), &module);
    let option = facts
        .declarations
        .imported_types()
        .resolved(
            facts
                .declarations
                .resolve_name("Maybe", NameNamespace::Type)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(option.declaration.name, "Option");
    let canonical = binding::option_declaration();
    let authored = packaged_modules()
        .iter()
        .find(|input| input.module_identity() == &canonical.module)
        .unwrap();
    let start = authored.text().find("enum Option<T>").unwrap() + "enum ".len();
    let site = authored
        .span(Span::new(start, start + "Option".len()))
        .unwrap();
    assert_eq!(option.declaration.source_location(), Some(site));
    assert!(option.native_type.is_none());
    let enumeration = facts
        .aggregates
        .enumerations()
        .find(|item| item.declaration.id == option.declaration.id)
        .unwrap();
    assert_eq!(enumeration.declaration, option.declaration);
    assert_eq!(
        Some(enumeration.id),
        facts.declarations.definitions().lookup(&canonical)
    );
}

#[test]
fn semantic_diagnostic_budget_invalidates_cached_results_without_changing_old_snapshots() {
    let mut sources = SourceDatabase::default();
    let text = "fn bad() { missing_one; missing_two; } fn good() -> i32 { 7 }";
    let id = sources
        .set("diagnostic-budget.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let original = snapshot(&mut db, &sources);
    assert!(original.file(id).unwrap().result().diagnostics().len() >= 2);

    db.set_max_semantic_diagnostics(1);
    let limited = snapshot(&mut db, &sources);
    let diagnostics = limited.file(id).unwrap().result().diagnostics();
    assert_eq!(diagnostics.len(), 2);
    assert!(matches!(
        diagnostics[1].kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "semantic diagnostics",
            limit: 1
        }
    ));
    assert!(limited.check_program(id, &Default::default()).is_err());
    assert!(
        limited
            .file(id)
            .unwrap()
            .result()
            .facts()
            .declarations
            .iter()
            .any(|declaration| declaration.name == "good")
    );
    assert!(original.file(id).unwrap().result().diagnostics().len() >= 2);

    db.set_max_semantic_diagnostics(0);
    let zero = snapshot(&mut db, &sources);
    assert!(matches!(
        zero.file(id).unwrap().result().diagnostics()[0].kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "semantic diagnostics",
            limit: 0
        }
    ));
}

#[test]
fn declaration_paths_distinguish_kinds_duplicates_and_method_owners() {
    let text = "struct Same { val n: i32 } fn Same() -> i32 { 1 } fn Same() -> i32 { 2 } trait A { fn get(self) -> i32; } trait B { fn get(self) -> i32; } impl A for Same { fn get(self) -> i32 { self.n } }";
    let source = SourceFile::new("definitions.kgr", text);
    let result =
        crate::analyze_source(&source, standard_modules()).expect("installed declaration analysis");
    assert!(!result.diagnostics().is_empty());
    let facts = result.facts();
    let declarations = facts
        .declarations
        .iter()
        .filter(|decl| matches!(decl.id, DeclarationId::Definition(_)))
        .collect::<Vec<_>>();
    let ids = declarations
        .iter()
        .map(|decl| &decl.id)
        .collect::<HashSet<_>>();
    assert_eq!(declarations.len(), ids.len());
    assert_eq!(
        declarations
            .iter()
            .filter(|decl| decl.name == "get")
            .count(),
        3
    );
    let same = declarations
        .iter()
        .filter(|decl| decl.name == "Same")
        .map(|decl| {
            let DeclarationId::Definition(id) = &decl.id else {
                unreachable!()
            };
            let segment = facts
                .declarations
                .definitions()
                .resolve(*id)
                .unwrap()
                .segments()
                .next()
                .unwrap();
            (segment.kind, segment.occurrence)
        })
        .collect::<HashSet<_>>();
    assert_eq!(
        same,
        [
            (DefinitionKind::Struct, 0),
            (DefinitionKind::Function, 0),
            (DefinitionKind::Function, 1)
        ]
        .into_iter()
        .collect()
    );
}
