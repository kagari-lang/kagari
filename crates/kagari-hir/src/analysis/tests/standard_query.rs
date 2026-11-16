//! Source query regressions, including native implementations and protocol views.

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
                let text = snapshot.source(declaration.source_location().unwrap().file).unwrap();
                assert_eq!(
                    &text.text()[declaration.source_location().unwrap().range.start..declaration.source_location().unwrap().range.end],
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
            .declared_implementation(definitions, &interface, &receiver, &Default::default())
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
                .declared_implementation(definitions, &interface, &item_type, &Default::default())
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
            let source = snapshot.source(declaration.source_location().unwrap().file).unwrap();
            assert_eq!(
                &source.text()[declaration.source_location().unwrap().range.start..declaration.source_location().unwrap().range.end],
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
            let source = snapshot.source(definition.source_location().unwrap().file).unwrap();
            assert_eq!(
                &source.text()[definition.source_location().unwrap().range.start..definition.source_location().unwrap().range.end],
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
}

#[cfg(test)]
mod trait_tests {}

#[cfg(test)]
mod interpolation_queries {

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
