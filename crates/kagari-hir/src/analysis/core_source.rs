//! Immutable authored core resources shared by ordinary analysis and pre-install construction.

use crate::analysis::external::ExternalModule;
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_core_source::MODULES;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use std::sync::{Arc, LazyLock};

fn identity(name: &str) -> ModuleIdentity {
    let resource = MODULES
        .iter()
        .find(|m| m.name == name)
        .expect("packaged module");
    ModuleIdentity {
        package: PackageId(format!("kagari-{}", resource.package_alias)),
        path: vec![name.into()],
    }
}

/// Returns shared, unparsed core sources with their public package aliases and no prelude.
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
            input.package_alias = Some(resource.package_alias.into());
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
        analysis::{AnalysisDatabase, error::AnalysisError, external::foundation::change_export},
        analyze_source,
        declarations::DeclarationId,
        imports::{ResolvedTarget, SourceItem},
        program::ProgramCheckError,
        resolver::resolved::ResolvedName,
        tests::{common::definition, native_fixture::function_module},
        types::TypeId,
    };
    use kagari_common::identity::DefinitionKind;
    use kagari_source::{
        diagnostic::DiagnosticKind, source::SourceFile, source_database::SourceSnapshot,
    };
    use kagari_syntax::ast::{borrow::AstNode, views::ItemView};
    use kagari_types::{language::role::LangRole, scalar::BuiltinType, ty::Ty};
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn core_only_client_imports_the_authored_traits_selected_by_language_roles() {
        let client = Arc::new(SourceFile::new(
            "/client/imports.kgr",
            "use core::cmp::{PartialEq, Eq, PartialOrd, Ord};
             use core::iter::{Iterator, Iterable, FromIterator, Sum, Product};
             use core::ops::{Add, Sub, Mul, Div, Rem, BitAnd, BitOr, BitXor, Shl, Shr,
                             Neg, Not, Index, Fn, Try, FromResidual, RangeBounds};
             use core::convert::{From, Into, TryFrom, TryInto};
             use core::fmt::{Debug, Display};
             use core::hash::Hash;
             use core::str::FromStr;
             use core::default::Default;",
        ));
        let cancel = Default::default();
        let snapshot = AnalysisDatabase::default()
            .snapshot(SourceSnapshot::single_file(client.clone()), &cancel)
            .unwrap();
        snapshot.check_program(client.id(), &cancel).unwrap();
        let root = snapshot.file(client.id()).unwrap().result().facts();
        let mut imported = BTreeMap::new();
        for import in &root.names.imports.directives {
            let [_, module, name] = import.segments.as_slice() else {
                panic!("unexpected import: {:?}", import.segments);
            };
            let Some(ResolvedTarget::Source(target)) = import.resolution.target() else {
                panic!("{name} must resolve to source");
            };
            let SourceItem::Trait(id) = target.item else {
                panic!("{name} must be a trait");
            };
            let owner = snapshot.file(target.unit.file).unwrap().result().facts();
            assert_eq!(
                owner.lowered.source.name(),
                format!("kagari-core:///{module}.kgr")
            );
            assert!(
                target
                    .unit
                    .matches(&owner.lowered, owner.declarations.definitions())
            );
            assert_eq!(owner.lowered.source.module_identity(), &identity(module));
            assert_eq!(&owner.lowered.module.traits[id.index()].name, name);
            let definition = owner
                .declarations
                .definition(ResolvedName::Trait(id))
                .unwrap();
            assert!(imported.insert(name.as_str(), *definition).is_none());
        }
        assert_eq!(imported.len(), 35);
        for role in LangRole::ALL {
            let protocol = role.protocol();
            let selected = root.aggregates.language_trait(protocol).unwrap();
            assert_eq!(selected.declaration, imported[protocol.name()], "{role:?}");
        }
    }

    #[test]
    fn core_only_propagation_selects_the_custom_carrier_and_residual() {
        // The runtime fixture normally gets these names from the stdlib prelude.
        let client = SourceFile::new(
            "/client/custom.kgr",
            concat!(
                "use core::option::{Option, Some, None}; use alloc::string::String;\n",
                include_str!(
                    "../../../kagari/tests/fixtures/language/try_protocols/ordinary/source_struct_carriers_use_selected_methods_once.kgr"
                )
            ),
        );
        let checked = analyze_source(&client, vec![])
            .unwrap()
            .into_checked()
            .unwrap();
        let propagation = checked
            .lowered
            .module
            .nodes
            .expressions()
            .find_map(|(id, _)| checked.typed.type_table().propagation(id))
            .expect("operand(state)? must record both selected calls");
        for (call, expected) in [
            (&propagation.branch, "Carrier"),
            (&propagation.from_residual, "Residual"),
        ] {
            let [TypeId::Struct(actual)] = call.signature.as_ref().unwrap().params.as_slice()
            else {
                panic!("expected the local {expected} struct");
            };
            assert_eq!(
                actual.declaration,
                definition(
                    checked.declarations.definitions(),
                    &checked.lowered,
                    DefinitionKind::Struct,
                    expected
                )
            );
        }
    }

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
        assert_eq!(core.files().count(), MODULES.len());

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
        assert_eq!(snapshot.inputs().library_modules().len(), MODULES.len() + 1);
        assert_eq!(snapshot.physical_sources().files().count(), 1);
        for name in [
            "cmp", "option", "convert", "default", "ops", "result", "fmt", "hash", "iter", "str",
            "string",
        ] {
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
                format!("{}:///{name}.kgr", owner.package.0)
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
    fn packaged_core_edit_invalidates_the_client_but_not_its_retained_snapshot() {
        let client = Arc::new(SourceFile::new(
            "/client/size.kgr",
            "use core::iter::Iterator; fn size<I: Iterator>(it: I) -> u64 { it.count() }",
        ));
        let root = client.id();
        let sources = SourceSnapshot::single_file(client);
        let cancel = Default::default();
        let mut db = AnalysisDatabase::default();
        let key = db.environment_fingerprint(&cancel).unwrap();
        let old = db.snapshot(sources.clone(), &cancel).unwrap();
        let facts = old.file(root).unwrap().result().facts();
        let size = definition(
            old.definitions(),
            &facts.lowered,
            DefinitionKind::Function,
            "size",
        );
        let warm = db.body(sources.clone(), &size, &cancel).unwrap().unwrap();
        assert!(warm.diagnostics().is_empty());

        let mut iter = packaged_modules()
            .iter()
            .find(|m| m.source.module_identity() == &identity("iter"))
            .unwrap()
            .clone();
        change_export(&mut iter, |text| {
            text.replace("fn count(self)", "fn remaining(self)")
        });
        db.replace_core_fixture(iter);
        assert_ne!(db.environment_fingerprint(&cancel).unwrap(), key);
        let body = db.body(sources.clone(), &size, &cancel).unwrap().unwrap();
        assert_eq!(body.reused_bodies(), 0);
        let missing_count = |kind: &DiagnosticKind| matches!(kind, DiagnosticKind::UnknownName { name } if name == "count");
        assert!(body.diagnostics().iter().any(|d| missing_count(&d.kind)));
        let current = db.snapshot(sources, &cancel).unwrap();
        let Err(ProgramCheckError::Diagnostics(errors)) = current.check_program(root, &cancel)
        else {
            panic!("expected the unchanged client to fail checking against the edited core");
        };
        assert!(
            errors
                .iter()
                .any(|d| d.file == root && missing_count(&d.diagnostic.kind)),
            "{errors:?}"
        );

        old.check_program(root, &cancel).unwrap();
        assert_eq!(
            old.inputs()
                .database()
                .unwrap()
                .environment_fingerprint(&cancel)
                .unwrap(),
            key
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
