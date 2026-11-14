//! Source query regressions, including native implementations and protocol views.
use crate::{
    declarations::DeclarationId, language::semantics::ProtocolSemantics,
    native::render::declaration_source, tests::test_analysis, types::TypeId,
};
use kagari_stdlib::catalog as foundation_catalog;
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

    /* Temporarily paused: stdlib restoration (NC01-NC06).
    #[test]
    fn ranges_and_collections_expose_source_owned_api_queries() {
        for (source, receiver, expected) in [
            (
                "fn main() { val range = 0u8..3u8; range. }",
                "range. }",
                vec!["iter", "start_bound", "end_bound"],
            ),
            (
                "fn main() { val values = [0; 4]; values. }",
                "values. }",
                vec!["len", "is_empty", "iter"],
            ),
            (
                "fn main() { val values = Vec::from([0; 4]); values. }",
                "values. }",
                vec!["len", "get", "push", "clear"],
            ),
        ] {
            let mut sources = SourceDatabase::default();
            let file = sources
                .set("ranges.kgr", source.into(), SourceLayer::Base)
                .unwrap();
            let snapshot = test_analysis()
                .snapshot(sources.snapshot(), &Default::default())
                .unwrap();
            let analysis = snapshot.file(file).unwrap();
            let candidates = analysis.method_completions(
                source.find(receiver).unwrap() + receiver.find('.').unwrap() + 1,
            );
            if receiver.starts_with("values") {
                assert!(
                    !candidates
                        .iter()
                        .any(|item| matches!(item.name.as_str(), "start_bound" | "end_bound"))
                );
            }
            if source.contains("val values = [") {
                assert!(
                    !candidates
                        .iter()
                        .any(|item| matches!(item.name.as_str(), "push" | "pop" | "clear" | "get"))
                );
            }
            for name in expected {
                let matches = candidates
                    .iter()
                    .filter(|m| m.name == name)
                    .collect::<Vec<_>>();
                assert_eq!(matches.len(), 1, "{name}");
                let api = matches[0];
                let declaration = snapshot.declaration(&api.declaration).unwrap();
                let text = snapshot.source(declaration.location.file).unwrap();
                assert_eq!(
                    &text.text()[declaration.location.range.start..declaration.location.range.end],
                    name
                );
            }
        }
    }
    */

    /* Temporarily paused: stdlib restoration (NC01-NC06).
    #[test]
    fn native_iterator_implementation_exposes_checked_protocol_members() {
        let text = "fn main() { val values = [20,22].iter(); values. }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("iterator.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = test_analysis()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        let item_type = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
        let authoring_catalog = analysis.to_unverified(&Default::default()).unwrap();
        let definitions = authoring_catalog.definitions();
        let receiver = TypeId::NativeObject(NominalType {
            declaration: definitions
                .lookup(&library::type_id(
                    "CollectionCursor",
                    DefinitionKind::AssociatedType,
                ))
                .unwrap(),
            arguments: vec![item_type.clone()],
            associated_types: Default::default(),
        });
        let catalog = &authoring_catalog.records().facts().aggregates;
        let interface = Protocol::Iterator.nominal(definitions).unwrap();
        let (implementation, arguments) = catalog
            .engine_implementation(definitions, &interface, &receiver, &Default::default())
            .unwrap();
        let member = definitions
            .lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap();
        assert_eq!(
            implementation.trait_type.associated_types[&member].instantiate(&arguments),
            item_type
        );
        let impl_member = definitions
            .lookup_child(implementation.id, DefinitionKind::AssociatedType, "Item", 0)
            .unwrap();
        assert_ne!(impl_member, member);
        assert!(
            snapshot
                .declaration(&DeclarationId::Definition(impl_member))
                .is_some()
        );
        assert!(
            snapshot
                .declaration(&DeclarationId::Definition(member))
                .is_some()
        );
        assert!(
            catalog
                .engine_implementation(definitions, &interface, &item_type, &Default::default())
                .is_none()
        );

        let candidates = analysis.method_completions(text.find("values. }").unwrap() + 7);
        for name in ["next", "iter"] {
            let matches: Vec<_> = candidates.iter().filter(|m| m.name == name).collect();
            assert_eq!(matches.len(), 1, "{name}");
            let api = matches[0];
            assert_eq!(
                match &api.declaration {
                    DeclarationId::Definition(id) =>
                        definitions
                            .resolve(*id)
                            .unwrap()
                            .segments()
                            .next()
                            .unwrap()
                            .kind,
                    _ => panic!("source method"),
                },
                if name == "next" {
                    kagari_common::identity::DefinitionKind::Impl
                } else {
                    kagari_common::identity::DefinitionKind::Trait
                },
                "{name}: {:?}",
                api.declaration
            );
            let declaration = snapshot.declaration(&api.declaration).unwrap();
            let source = snapshot.source(declaration.location.file).unwrap();
            assert_eq!(
                &source.text()[declaration.location.range.start..declaration.location.range.end],
                name
            );
        }
        let method = catalog
            .trait_(&interface.declaration)
            .unwrap()
            .methods
            .iter()
            .find(|method| method.name == "next")
            .unwrap();
        let target = &implementation.methods[&method.id];
        assert!(
            snapshot
                .declaration_snapshot()
                .documentation(&DeclarationId::Definition(*target))
                .unwrap()
                .written_signature
                .contains("fn next")
        );
    }
    */

    /* Temporarily paused: stdlib restoration (NC01-NC06).
    #[test]
    fn native_calls_types_and_variants_navigate_to_source() {
        let text = "use alloc::vec::Vec::new as create; fn main() { val value: Result<i32,String> = Ok(7); val values: Vec<i32> = create(); values.len(); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("main.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = test_analysis()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        for (needle, expected) in [
            ("create()", "new"),
            ("len()", "len"),
            ("Result<i32", "Result"),
            ("Ok(7)", "Ok"),
        ] {
            let offset = text.find(needle).unwrap();
            let definition = analysis
                .definition_at(offset)
                .unwrap_or_else(|| panic!("missing {needle}"));
            assert_eq!(definition.name, expected);
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                expected
            );
            let api = snapshot.documentation_at(file, offset).unwrap();
            assert_eq!(api.declaration.id, definition.id);
            if expected == "Result" {
                assert!(!api.documentation.is_empty());
            }
            assert_eq!(snapshot.declaration(&definition.id), Some(definition));
        }
        let signature = analysis
            .call_signature_at(text.find("create()").unwrap())
            .unwrap();
        assert!(signature.parameters.is_empty());
        assert_eq!(
            signature.result,
            foundation_vec(
                analysis.definitions(),
                TypeId::Builtin(kagari_types::scalar::BuiltinType::I32)
            )
        );
        let signature = analysis
            .call_signature_at(text.find("len()").unwrap())
            .unwrap();
        assert!(signature.parameters.is_empty());
        assert_eq!(
            signature.result,
            TypeId::Builtin(kagari_types::scalar::BuiltinType::U64)
        );
    }
    */

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
        let candidates = old.file(file).unwrap().method_completions(offset);
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
            old.file(file).unwrap().method_completions(offset),
            candidates
        );
    }
}

#[cfg(test)]
mod trait_tests {
    use super::*;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn registered_defaults_navigate_to_source_and_expose_checked_signatures() {
        let text = "use demo::native::NativeRead; struct Reader {} impl NativeRead for Reader {} fn main() { val value = Reader {}; value.read(); value.fixed(); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("defaults.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = crate::tests::native::database()
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
            let api = snapshot.documentation_at(file, offset).unwrap();
            assert_eq!(api.declaration.name, name);
            assert_eq!(api.documentation, format!("Native {name} contract."));
            assert_eq!(
                snapshot
                    .source(api.declaration.location.file)
                    .unwrap()
                    .name(),
                declaration_source(
                    &crate::tests::native::module(),
                    &foundation_catalog::shared().unwrap()
                )
                .unwrap()
                .uri
            );
            let signature = analysis.call_signature_at(offset).unwrap();
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
        let text = "fn identity<T: Eq>(a:T,b:T)->bool { a.eq(b) } fn get(value: i32)->i32 { value } fn main() { get(7); }";
        let file = sources
            .set("traits.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        for (needle, name) in [("Eq>", "Eq"), ("eq(b)", "eq")] {
            let item = snapshot
                .documentation_at(file, text.find(needle).unwrap())
                .unwrap();
            assert_eq!(item.declaration.name, name);
            let declaration = item.declaration;
            assert!(
                snapshot
                    .source(declaration.location.file)
                    .unwrap()
                    .text()
                    .contains("///")
            );
        }
        let user = snapshot
            .documentation_at(file, text.find("get(7)").unwrap())
            .unwrap();
        assert_eq!(user.declaration.name, "get");
        assert_eq!(user.declaration.location.file, file);
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
        assert!(
            snapshot
                .source(declaration.location.file)
                .unwrap()
                .name()
                .ends_with("kagari-core/iter.kgr")
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
            .definition_at(offset)
            .unwrap()
            .clone();
        assert_eq!(original.name, "value");
        assert_eq!(
            old.file(file).unwrap().type_at(offset),
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
            .definition_at(offset + prefix.len())
            .unwrap();
        assert_eq!(
            moved.location.range.start,
            original.location.range.start + prefix.len()
        );
        assert_eq!(
            old.file(file).unwrap().definition_at(offset),
            Some(&original)
        );
    }

    /* Temporarily paused: stdlib restoration (NC01-NC06).
    #[test]
    fn trait_method_completion_requires_matching_associated_items() {
        for interface_implementations in [false, true] {
            for (body, available, interface_output_matches) in [
                ("[1].", false, false),
                ("[\"one\"].", true, false),
                ("[1].iter().", false, false),
                ("[\"one\"].iter().", true, false),
                // Importing a trait alone grants no methods. Explicit native impls
                // for an interface participate and retain their output constraints.
                ("val xs: List<i32> = Vec::from([1]); xs.", false, false),
                (
                    "val xs: List<String> = Vec::from([\"one\"]); xs.",
                    false,
                    true,
                ),
                (
                    "val xs: MutableList<String> = Vec::from([\"one\"]); xs.",
                    false,
                    true,
                ),
            ] {
                let text = format!(
                    "use std::collections::{{List, MutableList}}; use demo::text_items::TextItems; fn main() {{ {body} }}"
                );
                let mut sources = SourceDatabase::default();
                let file = sources
                    .set("completion.kgr", text.clone(), SourceLayer::Base)
                    .unwrap();
                let mut db = test_analysis();
                db.set_native_modules(
                    foundation_catalog::shared()
                        .unwrap()
                        .into_iter()
                        .chain([crate::tests::native::text_items_module(
                            interface_implementations,
                        )])
                        .collect(),
                );
                let snapshot = db
                    .snapshot(sources.snapshot(), &Default::default())
                    .unwrap();
                let diagnostics = snapshot.file(file).unwrap().result().diagnostics();
                assert!(
                    diagnostics.iter().all(|diagnostic| matches!(
                        diagnostic.kind,
                        kagari_source::diagnostic::DiagnosticKind::ExpectedFieldName
                    )),
                    "{text}: {diagnostics:?}"
                );
                let completions = snapshot
                    .file(file)
                    .unwrap()
                    .method_completions(text.find(". }").unwrap() + 1);
                assert_eq!(
                    completions.iter().any(|item| item.name == "text_items"),
                    available || interface_implementations && interface_output_matches,
                    "{body} with interface impls={interface_implementations}: {:?}; diagnostics={:?}",
                    completions,
                    snapshot.file(file).unwrap().result().diagnostics()
                );
            }
        }
    }
    */
}
