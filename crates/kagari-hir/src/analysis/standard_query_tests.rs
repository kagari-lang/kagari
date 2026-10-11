//! Source query regressions, including native implementations and protocol views.
use crate::{
    analysis::ownership,
    declarations::DeclarationId,
    language::semantics::ProtocolSemantics,
    native::render::declaration_source,
    tests::test_analysis,
    types::{NominalType, TypeId},
};
use kagari_stdlib::{catalog as foundation_catalog, identity as library};
use kagari_types::{collection::CollectionAccess, language::Protocol};

fn foundation_vec(item: TypeId) -> TypeId {
    let module = kagari_types::declaration::module::ModuleDecl::new(
        kagari_stdlib::namespaces::type_owner("Vec"),
    );
    TypeId::NativeObject(NominalType {
        declaration: module.definition(
            kagari_common::identity::DefinitionKind::AssociatedType,
            "Vec",
        ),
        arguments: vec![item],
        associated_types: Default::default(),
    })
}

fn foundation_interface(name: &str) -> NominalType {
    NominalType {
        declaration: library::trait_id(name),
        arguments: vec![],
        associated_types: Default::default(),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

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
        let receiver = TypeId::Iter(Box::new(item_type.clone()));
        let authoring_catalog = analysis.to_unverified(&Default::default()).unwrap();
        let catalog = &authoring_catalog.facts().aggregates;
        let interface = Protocol::Iterator.nominal();
        let (implementation, arguments) = catalog
            .engine_implementation(&interface, &receiver, &Default::default())
            .unwrap();
        let member = kagari_common::identity::associated_type_id(&interface.declaration, "Item");
        assert_eq!(
            implementation.trait_type.associated_types[&member].instantiate(&arguments),
            item_type
        );
        let impl_member = kagari_common::identity::associated_type_id(&implementation.id, "Item");
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
                .engine_implementation(&interface, &item_type, &Default::default())
                .is_none()
        );

        let candidates = analysis.method_completions(text.find("values. }").unwrap() + 7);
        for name in ["next", "iter"] {
            let matches: Vec<_> = candidates.iter().filter(|m| m.name == name).collect();
            assert_eq!(matches.len(), 1, "{name}");
            let api = matches[0];
            assert_eq!(
                match &api.declaration {
                    DeclarationId::Definition(id) => id.path[0].kind,
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
                .documentation(&DeclarationId::Definition(target.clone()))
                .unwrap()
                .written_signature
                .contains("fn next")
        );
    }

    #[test]
    fn collection_implementation_catalog_retains_constraints_and_source_members() {
        use crate::typeck::table::ConstraintTarget;
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("contracts.kgr", "fn main() {}".into(), SourceLayer::Base)
            .unwrap();
        let snapshot = test_analysis()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let authoring_catalog = snapshot
            .file(root)
            .unwrap()
            .to_unverified(&Default::default())
            .unwrap();
        let catalog = &authoring_catalog.facts().aggregates;
        let integer = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
        let string = TypeId::Builtin(kagari_types::scalar::BuiltinType::String);
        let target = TypeId::Map {
            key: Box::new(integer.clone()),
            value: Box::new(string.clone()),
            access: CollectionAccess::Mutable,
        };
        for (kind, arguments) in [
            ("Map", vec![integer.clone(), string.clone()]),
            ("MutableMap", vec![integer.clone(), string.clone()]),
            ("Iterable", vec![]),
        ] {
            let mut interface = foundation_interface(kind);
            interface.arguments = arguments;
            assert!(
                catalog
                    .engine_implementation(&interface, &target, &Default::default())
                    .is_some(),
                "{kind:?}"
            );
        }
        let mut interface = foundation_interface("Map");
        interface.arguments = vec![integer.clone(), string.clone()];
        let (collect, arguments) = catalog
            .engine_implementation(&interface, &target, &Default::default())
            .unwrap();
        let constraints = collect
            .bounds
            .iter()
            .find(|(ty, _)| ty.instantiate(&arguments) == integer)
            .unwrap()
            .1;
        let mut names = constraints
            .iter()
            .map(|constraint| {
                let ConstraintTarget::Trait(bound) = constraint else {
                    panic!("declared trait bound")
                };
                bound.declaration.path.last().unwrap().name.as_str()
            })
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["Eq", "Hash", "PartialEq"]);
        let method = collect.methods.values().next().unwrap();
        let metadata = snapshot
            .declaration_snapshot()
            .documentation(&DeclarationId::Definition(method.clone()))
            .unwrap();
        assert_eq!(
            snapshot
                .source(metadata.declaration.location.file)
                .unwrap()
                .name(),
            declaration_source(
                &foundation_catalog::shared()
                    .into_iter()
                    .find(|module| module.identity
                        == kagari_stdlib::namespaces::module("std", "collections"))
                    .unwrap(),
                &foundation_catalog::shared(),
            )
            .unwrap()
            .uri
        );
        assert!(metadata.written_signature.contains("fn "));
        assert_eq!(
            snapshot.declaration(&metadata.declaration.id),
            Some(&metadata.declaration)
        );
    }

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
            foundation_vec(TypeId::Builtin(kagari_types::scalar::BuiltinType::I32))
        );
        let signature = analysis
            .call_signature_at(text.find("len()").unwrap())
            .unwrap();
        assert!(signature.parameters.is_empty());
        assert_eq!(
            signature.result,
            TypeId::Builtin(kagari_types::scalar::BuiltinType::USize)
        );
    }

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
                    &foundation_catalog::shared()
                )
                .unwrap()
                .uri
            );
            let signature = analysis.call_signature_at(offset).unwrap();
            assert_eq!(
                signature.declaration,
                ownership::paths(
                    &api.declaration,
                    snapshot.definitions(),
                    &Default::default()
                )
                .unwrap()
                .id
            );
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
        let iterator = authoring_iterator
            .facts()
            .aggregates
            .trait_(&Protocol::Iterator.nominal().declaration)
            .unwrap();
        let member = iterator.associated_types.keys().next().unwrap();
        let declaration = snapshot
            .declaration(&DeclarationId::Definition(member.clone()))
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
            "fn render(value:i32)->String { f\"中文 😀 {value}\" }\r\nfn broken() { missing; }";
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

    #[test]
    fn trait_method_completion_requires_matching_associated_items() {
        for (body, available) in [
            ("[1].", false),
            ("[\"one\"].", true),
            ("[1].iter().", false),
            ("[\"one\"].iter().", true),
            // Erased interfaces expose their own declared/inherited methods;
            // importing another trait does not extend the interface surface.
            ("val xs: List<i32> = Vec::from([1]); xs.", false),
            ("val xs: List<String> = Vec::from([\"one\"]); xs.", false),
            (
                "val xs: MutableList<String> = Vec::from([\"one\"]); xs.",
                false,
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
                    .into_iter()
                    .chain([crate::tests::native::text_items_module()])
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
                available,
                "{body}: {:?}; diagnostics={:?}",
                completions,
                snapshot.file(file).unwrap().result().diagnostics()
            );
        }
    }
}

#[cfg(test)]
mod collection_access_tests {
    use super::{foundation_interface, *};
    use crate::resolver::resolved::ResolvedName;
    use kagari_source::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn native_collection_witnesses_match_the_declared_interface_signatures() {
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("contracts.kgr", "fn main() {}".into(), SourceLayer::Base)
            .unwrap();
        let snapshot = test_analysis()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let authoring_catalog = snapshot
            .file(root)
            .unwrap()
            .to_unverified(&Default::default())
            .unwrap();
        let catalog = &authoring_catalog.facts().aggregates;
        let integer = TypeId::Builtin(kagari_types::scalar::BuiltinType::I32);
        let receivers = [
            foundation_vec(integer.clone()),
            TypeId::Map {
                key: Box::new(integer.clone()),
                value: Box::new(integer.clone()),
                access: CollectionAccess::Mutable,
            },
            TypeId::Set(Box::new(integer), CollectionAccess::Mutable),
        ];
        let mut checked = 0;
        for receiver in receivers {
            let (kinds, arguments) = match &receiver {
                TypeId::NativeObject(nominal) => {
                    (["List", "MutableList"], nominal.arguments.clone())
                }
                TypeId::Map { key, value, .. } => (
                    ["Map", "MutableMap"],
                    vec![(**key).clone(), (**value).clone()],
                ),
                TypeId::Set(item, _) => (["Set", "MutableSet"], vec![(**item).clone()]),
                _ => unreachable!(),
            };
            for kind in kinds {
                let mut interface = foundation_interface(kind);
                interface.arguments = arguments.clone();
                let (implementation, arguments) = catalog
                    .engine_implementation(&interface, &receiver, &Default::default())
                    .unwrap();
                let contract = catalog.trait_(&interface.declaration).unwrap();
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(interface.arguments.iter().cloned())
                    .collect();
                let instantiate = |ty: &TypeId| {
                    ty.with_self(&contract.id, &receiver)
                        .instantiate(&substitution)
                };
                // Default containers explicitly override the algorithm defaults too.
                assert_eq!(implementation.methods.len(), contract.methods.len());
                for (trait_method, target) in &implementation.methods {
                    let declared = catalog.trait_method(trait_method).unwrap();
                    let declaration = snapshot
                        .declaration(&DeclarationId::Definition(target.clone()))
                        .unwrap();
                    let file = snapshot
                        .signature_snapshot()
                        .file(declaration.location.file)
                        .unwrap();
                    let authoring = file.authoring(&Default::default()).unwrap();
                    let Some(ResolvedName::Function(function)) =
                        authoring.declarations.definition_target(target)
                    else {
                        panic!("implementation method")
                    };
                    let method = authoring
                        .signatures
                        .facts()
                        .functions()
                        .iter()
                        .find(|method| method.id == function)
                        .unwrap();
                    let declared_parameters =
                        &declared.generic_params[contract.generic_params.len()..];
                    let implementation_parameters =
                        &method.generic_params[implementation.generic_params.len()..];
                    assert_eq!(implementation_parameters.len(), declared_parameters.len());
                    let mut method_arguments = arguments.clone();
                    method_arguments.extend(
                        implementation_parameters
                            .iter()
                            .cloned()
                            .zip(declared_parameters.iter().cloned().map(TypeId::Generic)),
                    );
                    assert_eq!(
                        method
                            .params
                            .iter()
                            .map(|p| p.ty.instantiate(&method_arguments))
                            .collect::<Vec<_>>(),
                        declared
                            .params
                            .iter()
                            .map(|p| instantiate(&p.ty))
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        method.return_type.instantiate(&method_arguments),
                        instantiate(&declared.return_type)
                    );
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 6);
    }

    #[test]
    fn constructors_navigate_to_distinct_documented_members() {
        let text = "use std::collections::{HashMap, HashSet, Map};\nfn main() { val a: Vec<i32> = Vec::new(); val b: Vec<i32> = Vec::new(); val c: Map<i32,i32> = HashMap::new(); val d: HashMap<i32,i32> = HashMap::new(); val e: HashSet<i32> = HashSet::new(); val f: HashSet<i32> = HashSet::new(); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("constructors.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = test_analysis()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let mut identities = std::collections::HashSet::new();
        for spelling in ["Vec::new", "HashMap::new", "HashSet::new"] {
            let offset = text.find(spelling).unwrap() + spelling.find("::").unwrap() + 2;
            let definition = file.definition_at(offset).unwrap();
            assert!(identities.insert(definition.id.clone()));
            let api = snapshot
                .documentation_at(file.source().id(), offset)
                .unwrap();
            assert!(api.written_signature.contains("fn new"));
            let source = snapshot.source(definition.location.file).unwrap();
            assert_eq!(
                &source.text()[definition.location.range.start..definition.location.range.end],
                definition.name
            );
        }
    }

    #[test]
    fn readonly_member_completion_excludes_mutators() {
        for (annotation, constructor, mutable, write) in [
            ("List<i32>", "Vec::from([1])", false, "push"),
            ("Vec<i32>", "Vec::from([1])", true, "push"),
            ("Map<i32,i32>", "HashMap::new()", false, "insert"),
            ("HashMap<i32,i32>", "HashMap::new()", true, "insert"),
            ("Set<i32>", "HashSet::new()", false, "insert"),
            ("HashSet<i32>", "HashSet::new()", true, "insert"),
        ] {
            let text = format!(
                "use std::collections::{{List, Map, Set, HashMap, HashSet}}; fn main() {{ val values: {annotation} = {constructor}; values. }}"
            );
            let mut sources = SourceDatabase::default();
            let id = sources
                .set("completion.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let snapshot = test_analysis()
                .snapshot(sources.snapshot(), &Default::default())
                .unwrap();
            let diagnostics = snapshot.file(id).unwrap().result().diagnostics();
            assert!(
                diagnostics.iter().all(|diagnostic| matches!(
                    diagnostic.kind,
                    kagari_source::diagnostic::DiagnosticKind::ExpectedFieldName
                )),
                "{text}: {diagnostics:?}"
            );
            let candidates = snapshot
                .file(id)
                .unwrap()
                .method_completions(text.find("values. }").unwrap() + 7);
            assert!(candidates.iter().any(|item| item.name == "len"));
            assert_eq!(candidates.iter().any(|item| item.name == write), mutable);
        }
    }

    #[test]
    fn registered_string_calls_navigate_to_documented_declarations() {
        let text = "use demo::native::echo; fn main() { val text = \"hello\"; echo(text); }";
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("string-api.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = crate::tests::native::database()
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let file = snapshot.file(id).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let offset = text.find("echo(text)").unwrap();
        let definition = file.definition_at(offset).unwrap();
        let api = snapshot.documentation_at(id, offset).unwrap();
        assert_eq!(api.documentation, "Registered generic echo function.");
        let source = snapshot.source(definition.location.file).unwrap();
        assert_eq!(
            &source.text()[definition.location.range.start..definition.location.range.end],
            "echo"
        );
    }

    #[test]
    fn extension_completion_filters_total_order_requirement() {
        for (element, value, ordered) in [("i32", "1", true), ("f64", "1.0", false)] {
            let text = format!(
                "use std::collections::{{List}};\nimpl<T: Ord> List<T> {{ fn ordered(self) -> usize {{ self.len() }} }} impl<T: PartialEq> List<T> {{ fn comparable(self) -> usize {{ self.len() }} }} fn main() {{ val values: List<{element}> = Vec::from([{value}]); values. }}"
            );
            let mut sources = SourceDatabase::default();
            let id = sources
                .set("list-completion.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let snapshot = test_analysis()
                .snapshot(sources.snapshot(), &Default::default())
                .unwrap();
            let diagnostics = snapshot.file(id).unwrap().result().diagnostics();
            assert!(
                diagnostics.iter().all(|diagnostic| matches!(
                    diagnostic.kind,
                    kagari_source::diagnostic::DiagnosticKind::ExpectedFieldName
                )),
                "{text}: {diagnostics:?}"
            );
            let items = snapshot
                .file(id)
                .unwrap()
                .method_completions(text.find("values. }").unwrap() + 7);
            assert_eq!(items.iter().any(|item| item.name == "ordered"), ordered);
            assert!(items.iter().any(|item| item.name == "comparable"));
        }
    }
}
