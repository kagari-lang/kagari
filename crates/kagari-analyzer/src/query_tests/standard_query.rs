use crate::query_tests::{render_native, standard_modules};
use crate::{AnalyzerQueries, query_tests::test_analysis};
// Source query regressions, including native implementations and protocol views.
use kagari_hir::{
    declarations::DeclarationId, language::semantics::ProtocolSemantics, types::TypeId,
};
use kagari_types::language::Protocol;

/* stdlib test support paused (NC01-NC06).
fn foundation_vec(definitions: &DefinitionTable, item: TypeId) -> TypeId {
    TypeId::NativeObject(NominalType {
        declaration: definitions
            .lookup(&library::type_id("Vec", DefinitionKind::AssociatedType))
            .unwrap(),
        arguments: vec![item],
        associated_types: Default::default(),
    })
} */

#[cfg(test)]
mod tests {
    use super::*;

    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn incomplete_members_keep_standard_candidates_across_snapshots() {
        let mut sources = SourceDatabase::default();
        let text = "fn main() { val text=[1]; text. }";
        let file = sources
            .set("main.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let old = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let offset = text.find("text. ").unwrap() + 5;
        let candidates = old.file(file).unwrap().queries().method_completions(offset);
        assert!(candidates.iter().any(|item| item.name == "len"));
        sources
            .set(
                "main.kgr",
                "fn main() { missing; }".into(),
                SourceLayer::Overlay,
            )
            .unwrap();
        let _new = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        assert_eq!(
            old.file(file).unwrap().queries().method_completions(offset),
            candidates
        );
    }
}

#[cfg(test)]
mod trait_tests {
    use super::*;

    use kagari_hir::analysis::AnalysisDatabase;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    fn source_database() -> AnalysisDatabase {
        let providers = standard_modules()
            .into_iter()
            .chain([crate::query_tests::native_fixture::module()])
            .collect::<Vec<_>>();
        let views = providers
            .iter()
            .map(|provider| render_native(provider, &providers))
            .collect();
        let mut database = test_analysis();
        database.set_native_sources(providers, views).unwrap();
        database
    }

    #[test]
    fn registered_defaults_navigate_to_source_and_expose_checked_signatures() {
        let text = "use demo::bindings::NativeRead; struct Reader {} impl NativeRead for Reader {} fn main() { val value = Reader {}; value.read(); value.fixed(); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("defaults.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = source_database()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        for name in ["read", "fixed"] {
            let offset = text.find(&format!(".{name}(")).unwrap() + 1;
            let api = snapshot.queries().documentation_at(file, offset).unwrap();
            assert_eq!(api.declaration.name, name);
            assert_eq!(api.documentation, format!("Native {name} contract."));
            assert_eq!(
                snapshot
                    .source(api.declaration.source_location().unwrap().file)
                    .unwrap()
                    .name(),
                render_native(
                    &crate::query_tests::native_fixture::module(),
                    &standard_modules()
                )
                .uri
            );
            let signature = analysis.queries().call_signature_at(offset).unwrap();
            assert_eq!(signature.declaration, api.declaration.id.clone());
            assert!(signature.parameters.is_empty());
            assert_eq!(
                signature.result,
                TypeId::Builtin(kagari_types::scalar::BuiltinType::I32)
            );
        }
    }

    #[test]
    fn standard_trait_members_keep_source_identity_without_user_shadowing() {
        let mut sources = SourceDatabase::default();
        let text = "use std::cmp::Eq as Equal; fn identity<T: Equal>(a:T,b:T)->bool { a.eq(b) } fn get(value: i32)->i32 { value } fn main() { get(7); }";
        let file = sources
            .set("untitled:traits.kgr", text.into(), SourceLayer::Overlay)
            .unwrap();
        let mut db = source_database();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        let queries = snapshot.queries();
        for (reference, name, docs) in [
            ("Equal>", "Eq", "Marks equality as reflexive"),
            ("eq(b)", "eq", "Returns true if `self` and `arg1` are equal"),
        ] {
            let offset = text.find(reference).unwrap();
            let target = queries.definition_at(file, offset).unwrap();
            let item = queries.documentation_at(file, offset).unwrap();
            assert_eq!(item.declaration.id, target.id);
            assert!(item.documentation.starts_with(docs), "{item:?}");
            let location = target.source_location().unwrap();
            let source = snapshot.source(location.file).unwrap();
            assert_eq!(source.name(), "kagari-core:///cmp.kgr");
            let range = source.local_range(location).unwrap();
            assert_eq!(&source.text()[range.start..range.end], name);
        }
        let user = snapshot
            .queries()
            .documentation_at(file, text.find("get(7)").unwrap())
            .unwrap();
        assert_eq!(user.declaration.name, "get");
        assert_eq!(user.declaration.source_location().unwrap().file, file);
        assert!(user.documentation.is_empty());
        let authoring_iterator = analysis.to_unverified(&Default::default()).unwrap();
        let definitions = authoring_iterator.definitions();
        let iterator = authoring_iterator
            .records()
            .facts()
            .aggregates
            .trait_(&Protocol::Iterator.nominal(definitions).unwrap().declaration)
            .unwrap();
        let member = iterator.associated_types.keys().next().unwrap();
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(*member))
            .unwrap();
        assert_eq!(declaration.name, "Item");
        assert_eq!(
            snapshot
                .source(declaration.source_location().unwrap().file)
                .unwrap()
                .name(),
            "kagari-core:///iter.kgr"
        );
    }
}

#[cfg(test)]
mod interpolation_queries {
    use super::*;

    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn hole_bindings_retain_original_locations_and_survive_snapshot_rebasing() {
        let text =
            "fn render(value:i32)->String { \"中文 😀\r\n{value}\" }\r\nfn broken() { missing; }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("queries.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let old = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let offset = text.find("{value}").unwrap() + 1;
        let original = old
            .file(file)
            .unwrap()
            .queries()
            .definition_at(offset)
            .unwrap()
            .clone();
        assert_eq!(original.name, "value");
        assert_eq!(
            old.file(file).unwrap().queries().type_at(offset),
            Some(TypeId::Builtin(kagari_types::scalar::BuiltinType::I32))
        );
        let prefix = "// shifted 😀\r\n";
        sources
            .set(
                "queries.kgr",
                format!("{prefix}{text}"),
                SourceLayer::Overlay,
            )
            .unwrap();
        let new = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let moved = new
            .file(file)
            .unwrap()
            .queries()
            .definition_at(offset + prefix.len())
            .unwrap();
        assert_eq!(
            moved.source_location().unwrap().range.start,
            original.source_location().unwrap().range.start + prefix.len()
        );
        assert_eq!(
            old.file(file).unwrap().queries().definition_at(offset),
            Some(&original)
        );
    }
}
