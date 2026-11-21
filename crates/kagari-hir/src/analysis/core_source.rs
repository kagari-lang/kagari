//! Immutable authored core resources shared by ordinary analysis and pre-install construction.

use crate::analysis::external::ExternalModule;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_core_source::MODULES;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::{Arc, LazyLock};

fn identity(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("kagari-core".into()),
        path: vec![name.into()],
    }
}

/// Returns shared, unparsed core sources with the `core` package alias and no prelude.
///
/// A fresh database already contains these modules. To prepare a seed, take a snapshot
/// with no physical sources and call `check_program` with the desired core source ID.
/// Passing them to `set_external_modules` would supply duplicate owners, not replace core.
pub fn packaged_modules() -> &'static [ExternalModule] {
    &INPUTS
}

pub(super) fn shared_modules() -> Arc<[ExternalModule]> {
    Arc::clone(&INPUTS)
}

static INPUTS: LazyLock<Arc<[ExternalModule]>> = LazyLock::new(|| {
    // All resource names are absolute virtual URIs; the DB never reads a path.
    let mut sources = SourceDatabase::new("/").expect("absolute root");
    MODULES
        .iter()
        .map(|resource| {
            sources
                .bind_module(resource.uri, identity(resource.name))
                .expect("unique packaged core module");
            let id = sources
                .set(resource.uri, resource.text.into(), SourceLayer::Base)
                .expect("valid packaged source URI");
            let mut input = ExternalModule::new(sources.snapshot().file(id).unwrap().clone());
            input.package_alias = Some("core".into());
            // Only this packaged resource supplies the reporting variant. Ordinary
            // source (including an enum named Result) keeps fresh, unmarked variants.
            if resource.name == "result" {
                input.enum_failures.insert(("Result".into(), "Err".into()));
            }
            input.dependencies = resource
                .dependencies
                .iter()
                .map(|name| identity(name))
                .collect();
            input
        })
        .collect()
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analysis::{AnalysisDatabase, error::AnalysisError},
        analyze_source,
        declarations::DeclarationId,
        tests::native_fixture::function_module,
    };
    use kagari_source::{source::SourceFile, source_database::SourceSnapshot};
    use kagari_syntax::ast::{borrow::AstNode, views::ItemView};
    use kagari_types::{scalar::BuiltinType, ty::Ty};
    use std::sync::Arc;

    #[test]
    fn packaged_core_checks_point_and_navigates_to_authored_trait() {
        let mut db = AnalysisDatabase::new("/client").unwrap();
        let cancel = Default::default();
        let core = db.snapshot(SourceSnapshot::default(), &cancel).unwrap();
        let cmp = core
            .inputs()
            .core_modules()
            .iter()
            .find(|m| m.source.module_identity() == &identity("cmp"))
            .unwrap();
        let checked = core.check_program(cmp.source.id(), &cancel).unwrap();
        assert!(core.inputs().native_modules().is_empty());
        assert!(core.inputs().external_modules().is_empty());
        assert_eq!(core.files().count(), 6);

        let point = Arc::new(SourceFile::new(
            "/client/cmp.kgr",
            include_str!("../../tests/fixtures/analysis/external/core_point.kgr"),
        ));
        analyze_source(&point, vec![])
            .unwrap()
            .into_checked()
            .unwrap();
        let extra = Arc::new(SourceFile::new(
            "/vendor/extra.kgr",
            "pub const N: i32 = 7;",
        ));
        db.set_external_modules(vec![ExternalModule::new(extra.clone())]);
        let captured = db.inputs();
        let fingerprint = db.environment_fingerprint(&cancel).unwrap();
        db.set_external_modules(vec![]);
        assert!(db.inputs().external_modules().is_empty());
        let mut replay = captured.database().unwrap();
        assert_eq!(
            replay.environment_fingerprint(&cancel).unwrap(),
            fingerprint
        );
        let snapshot = replay
            .snapshot(SourceSnapshot::single_file(point.clone()), &cancel)
            .unwrap();
        snapshot.check_program(point.id(), &cancel).unwrap();
        assert_eq!(snapshot.inputs().external_modules().len(), 1);
        assert_eq!(
            snapshot.inputs().external_modules()[0].source.id(),
            extra.id()
        );
        assert_eq!(snapshot.inputs().library_modules().len(), 7);
        assert_eq!(snapshot.physical_sources().files().count(), 1);
        for name in ["cmp", "option", "convert", "default", "ops", "result"] {
            let owner = identity(name);
            let member = checked
                .modules()
                .iter()
                .find(|m| m.lowered.source.module_identity() == &owner)
                .unwrap();
            let id = member.lowered.source.id();
            let replayed = snapshot.file(id).unwrap();
            assert!(replayed.syntax().is_some());
            assert_eq!(replayed.source().module_identity(), &owner);
            assert_eq!(
                replayed.source().name(),
                format!("kagari-core:///{name}.kgr")
            );
            assert!(
                packaged_modules()
                    .iter()
                    .any(|m| m.source.id() == id && m.source.module_identity() == &owner)
            );
        }
        let file = snapshot.file(point.id()).unwrap();
        let facts = file.result().facts();
        let module = &facts.lowered.module;
        let same = module.functions.iter().find(|f| f.name == "same").unwrap();
        let bound = &module.generic_parameter(same.generic_params[0]).bounds[0];
        let target = facts.typed.type_table().constraint(bound.ty).unwrap();
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(target.declaration))
            .unwrap();
        let location = declaration.source_location().unwrap();
        let source = snapshot.source(location.file).unwrap();
        assert_eq!(source.name(), "kagari-core:///cmp.kgr");
        assert_eq!(location.file, cmp.source.id());
        assert_eq!(
            &source.text()[location.range.start..location.range.end],
            "PartialEq"
        );
        assert!(source.text()[..location.range.start].ends_with("pub trait "));
        let syntax = snapshot.file(location.file).unwrap().syntax().unwrap();
        assert_eq!(
            syntax.module_documentation(),
            "Equality and ordering for values of the same type."
        );
        let interface = syntax
            .items()
            .find_map(|item| match item {
                ItemView::TraitDef(t) if t.name_text().as_deref() == Some("PartialEq") => Some(t),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            interface.methods().next().unwrap().documentation(),
            "Returns true if `self` and `arg1` are equal according to this implementation."
        );
    }

    #[test]
    fn packaged_core_rejects_competing_source_and_native_owners() {
        let mut db = AnalysisDatabase::default();
        let cmp = packaged_modules()
            .iter()
            .find(|m| m.source.module_identity() == &identity("cmp"))
            .unwrap();
        let cancel = Default::default();
        db.set_external_modules(vec![cmp.clone()]);
        assert!(
            matches!(db.declarations(SourceSnapshot::default(), &cancel), Err(AnalysisError::NativeApi(e)) if e.0.contains("duplicate external declaration module or file"))
        );
        db.set_external_modules(vec![]);
        assert!(
            matches!(db.declarations(SourceSnapshot::single_file(cmp.source.clone()), &cancel), Err(AnalysisError::NativeApi(e)) if e.0.contains("source conflicts with library declaration input"))
        );
        db.set_native_modules(vec![function_module(
            identity("cmp"),
            "number",
            vec![],
            Ty::Builtin(BuiltinType::I32),
        )]);
        assert!(
            matches!(db.declarations(SourceSnapshot::default(), &cancel), Err(AnalysisError::NativeApi(e)) if e.0.contains("external module conflicts with installed provider"))
        );
    }
}
