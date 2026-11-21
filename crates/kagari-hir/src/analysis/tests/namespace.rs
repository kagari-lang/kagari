use crate::analysis::*;
use crate::tests::standard::standard_modules;
use crate::tests::{native, native_fixture};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{declaration::names::NameNamespace, scalar::BuiltinType, ty::Ty};

fn setup(text: &str) -> (SourceDatabase, AnalysisDatabase, FileId) {
    let mut sources = SourceDatabase::default();
    sources
        .bind_module(
            "library.kgr",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["library".into()],
            },
        )
        .unwrap();
    sources
        .set(
            "library.kgr",
            "pub struct Point {} pub fn number() -> i32 { 8 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    sources
        .bind_module(
            "root.kgr",
            ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["root".into()],
            },
        )
        .unwrap();
    let file = sources
        .set("root.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = native::database();
    let mut modules = standard_modules();
    modules.push(native_fixture::module());
    modules.push(native_fixture::function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        "number",
        vec![],
        Ty::Builtin(BuiltinType::I32),
    ));
    db.set_native_modules(modules);
    (sources, db, file)
}

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn imported_constants_preserve_visibility_immutability_and_invalid_owner_rejection() {
    for (provider, body) in [
        ("const CODE: i32 = 7;", "fn main() -> i32 { code }"),
        ("pub const CODE: i32 = 7;", "fn main() { code = 8; }"),
        ("pub const CODE: i32 = 1 / 0;", "fn main() -> i32 { code }"),
        ("pub const CODE: i32 = true;", "fn main() -> i32 { code }"),
    ] {
        let text = format!("use pkg::library::CODE as code; {body}");
        let (mut sources, mut db, file) = setup(&text);
        sources
            .set("library.kgr", provider.into(), SourceLayer::Overlay)
            .unwrap();
        let result = snapshot(&mut db, &sources);
        let analysis = result.file(file).unwrap();
        assert!(
            analysis.result().clone().into_codegen().is_err(),
            "{provider}: {body}"
        );
    }
}

#[test]
fn duplicate_type_imports_invalidate_all_alias_targets() {
    for imports in [
        "use pkg::library as api; use pkg::library as api;",
        "use pkg::library::Point as api; use pkg::library::Point as api;",
    ] {
        let ty = if imports.contains("::Point") {
            "api"
        } else {
            "api::Point"
        };
        let text = format!("{imports} fn bad(x: {ty}) {{ val y = {ty} {{}}; }}");
        let (sources, mut db, file) = setup(&text);
        let snapshot = snapshot(&mut db, &sources);
        let analysis = snapshot.file(file).unwrap();
        let facts = analysis.result().facts();
        assert!(
            facts
                .names
                .imports
                .directives
                .iter()
                .take(facts.lowered.module.imports.len())
                .all(|import| import.resolution.target().is_none())
        );
        assert!(
            facts
                .declarations
                .resolve_name(ty, NameNamespace::Type)
                .is_none()
        );

        assert!(analysis.result().clone().into_codegen().is_err());
    }
}
