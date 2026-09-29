//! Source query regressions, including native implementations and protocol views.

use crate::{
    builtin::{
        declarations::{self, ApiImplementationSemantics, ApiTypeSemantics},
        traits::StandardTraitSemantics,
    },
    declarations::DeclarationId,
    types::TypeId,
};
use kagari_abi::standard::traits::StandardTrait;
use kagari_common::collection::CollectionAccess;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AnalysisDatabase;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn ranges_and_interval_copy_expose_source_owned_api_queries() {
        for (source, receiver, expected) in [
            (
                "fn main() { val range = 0u8..3u8; range. }",
                "range. }",
                vec!["iter", "start_bound", "end_bound"],
            ),
            (
                "fn main() { val values = [0; 4]; values. }",
                "values. }",
                vec!["fill", "copy_from", "copy_within"],
            ),
        ] {
            let mut sources = SourceDatabase::default();
            let file = sources
                .set("ranges.kgr", source.into(), SourceLayer::Base)
                .unwrap();
            let snapshot = AnalysisDatabase::default()
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let analysis = snapshot.file(file).unwrap();
            let candidates = analysis.method_completions(
                source.find(receiver).unwrap() + receiver.find('.').unwrap() + 1,
            );
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
    fn native_iterator_implementation_exposes_members_and_inherited_defaults() {
        let item_type = TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32);
        let receiver = TypeId::Iter(Box::new(item_type.clone()));
        let implementations = declarations::implementations(&receiver);
        assert_eq!(implementations.len(), 1);
        let implementation = implementations[0];
        assert_eq!(implementation.interface, "Iterator");
        let (member, value) = &implementation.associated_types[0];
        assert_eq!(
            value.instantiate(&implementation.arguments(&receiver).unwrap()),
            item_type
        );
        assert_eq!(member.path.last().unwrap().1, "Item");
        assert_ne!(
            member.identity(),
            implementation.trait_declaration().associated_types[0]
                .item
                .identity()
        );
        assert!(
            declarations::implementations(&item_type)
                .iter()
                .all(|implementation| implementation.interface != "Iterator")
        );

        let text = "fn main() { val values: Iter<i32> = [20,22].iter(); values. }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("iterator.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        let candidates = analysis.method_completions(text.find("values. }").unwrap() + 7);
        for name in ["next", "map", "filter", "collect"] {
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
                }
            );
            let declaration = snapshot.declaration(&api.declaration).unwrap();
            let source = snapshot.source(declaration.location.file).unwrap();
            assert_eq!(
                &source.text()[declaration.location.range.start..declaration.location.range.end],
                name
            );
        }
        assert!(
            implementation.methods[0]
                .item
                .signature
                .contains("intrinsic(IterNext)")
        );
    }

    #[test]
    fn collection_implementation_catalog_retains_constraints_and_source_members() {
        use kagari_abi::standard::{surface::StandardEnum, traits::StandardTrait};
        let integer = TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32);
        let string = TypeId::Builtin(kagari_abi::scalar::BuiltinType::String);
        let target = TypeId::Map {
            key: Box::new(integer.clone()),
            value: Box::new(string.clone()),
            access: CollectionAccess::Mutable,
        };
        let implementations = declarations::implementations(&target);
        assert_eq!(implementations.len(), 4);
        let collect = implementations
            .iter()
            .find(|i| i.interface == "FromIterator")
            .unwrap();
        let mut interface = StandardTrait::FromIterator.nominal();
        interface
            .arguments
            .push(TypeId::Tuple(vec![integer.clone(), string.clone()]));
        let arguments = collect.applied_arguments(&target, &interface).unwrap();
        assert_eq!(collect.bounds[0].0.instantiate(&arguments), integer);
        assert_eq!(
            collect.bounds[0]
                .1
                .iter()
                .map(|b| b.name)
                .collect::<Vec<_>>(),
            ["Eq", "Hash"]
        );
        let method = &collect.methods[0].item;
        assert_eq!(method.uri, "kagari://std/map.kgr");
        assert!(method.signature.contains("CollectionFromIterator"));
        let declarations = AnalysisDatabase::default()
            .declarations(SourceDatabase::default().snapshot(), &Default::default())
            .unwrap();
        let source = declarations
            .files()
            .find(|file| file.source().name() == method.uri)
            .unwrap();
        let declaration = source.declarations().site_at(method.start).unwrap();
        assert_eq!(declarations.declaration(&declaration.id), Some(declaration));
        assert!(
            declarations
                .documentation(&declaration.id)
                .unwrap()
                .written_signature
                .contains("CollectionFromIterator")
        );

        let result = TypeId::StandardEnum {
            kind: StandardEnum::Result,
            args: vec![target, string.clone()],
        };
        let mut interface = StandardTrait::FromIterator.nominal();
        interface.arguments.push(TypeId::StandardEnum {
            kind: StandardEnum::Result,
            args: vec![TypeId::Tuple(vec![integer, string.clone()]), string],
        });
        let implementation = declarations::implementations(&result)[0];
        let arguments = implementation
            .applied_arguments(&result, &interface)
            .unwrap();
        assert_eq!(implementation.bounds[0].1[0].name, "FromIterator");
        assert!(
            implementation.bounds[0]
                .0
                .instantiate(&arguments)
                .is_concrete()
        );
        assert_eq!(
            implementation.methods[0].item.uri,
            "kagari://std/result.kgr"
        );
    }

    #[test]
    fn native_calls_types_and_variants_navigate_to_documented_source() {
        let text = "use std::array::ArrayList::get as lookup; fn main() { val value: Result<i32,String> = Ok(7); val values=[7]; lookup(values, values.len()); value.is_ok(); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("main.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        for (needle, expected) in [
            ("lookup(values", "get"),
            ("len()", "len"),
            ("Result<i32", "Result"),
            ("Ok(7)", "Ok"),
            ("is_ok()", "is_ok"),
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
            assert!(!api.documentation.is_empty());
            assert_eq!(snapshot.declaration(&definition.id), Some(definition));
        }
        let signature = analysis
            .call_signature_at(text.find("lookup(values").unwrap())
            .unwrap();
        assert_eq!(
            signature.parameters[0].1,
            TypeId::Array(
                Box::new(TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32)),
                CollectionAccess::Mutable
            )
        );
        let signature = analysis
            .call_signature_at(text.find("is_ok()").unwrap())
            .unwrap();
        assert!(signature.parameters.is_empty());
        assert_eq!(
            signature.result,
            TypeId::Builtin(kagari_abi::scalar::BuiltinType::Bool)
        );
    }

    #[test]
    fn incomplete_members_keep_standard_candidates_across_snapshots() {
        let mut sources = SourceDatabase::default();
        let text = "fn main() { val text=\"hi\"; text. }";
        let file = sources
            .set("main.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let old = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let offset = text.find("text. ").unwrap() + 5;
        let candidates = old.file(file).unwrap().method_completions(offset);
        assert!(candidates.iter().any(|item| item.name == "len_bytes"));
        sources
            .set(
                "main.kgr",
                "fn main() { missing; }".into(),
                SourceLayer::Overlay,
            )
            .unwrap();
        let _new = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
    use crate::analysis::AnalysisDatabase;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};
    #[test]
    fn iterator_defaults_navigate_to_source_and_expose_checked_signatures() {
        let text = "fn main(){val result: ArrayList<i32> = [20,22].iter().map(|x|x).collect();}";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("pipeline.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let analysis = snapshot.file(file).unwrap();
        assert!(
            analysis.result().diagnostics().is_empty(),
            "{:?}",
            analysis.result().diagnostics()
        );
        for name in ["iter", "map", "collect"] {
            let offset = text.find(&format!(".{name}(")).unwrap() + 1;
            let api = snapshot.documentation_at(file, offset).unwrap();
            assert_eq!(api.declaration.name, name);
            assert!(
                snapshot
                    .source(api.declaration.location.file)
                    .unwrap()
                    .name()
                    .ends_with("iter.kgr")
            );
            let signature = analysis.call_signature_at(offset).unwrap();
            assert_eq!(signature.declaration, api.declaration.id);
            assert_eq!(signature.parameters.len(), usize::from(name == "map"));
            assert!(signature.result.is_concrete());
            if name != "collect" {
                assert_eq!(signature.result.display_name(), "Iter<i32>");
            }
        }
    }
    #[test]
    fn standard_trait_members_keep_source_identity_without_user_shadowing() {
        let mut sources = SourceDatabase::default();
        let text = "fn identity<T: Eq>(a:T,b:T)->bool { a.eq(b) } fn get(value: i32)->i32 { value } fn main() { get(7); }";
        let file = sources
            .set("traits.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
        let iterator = analysis
            .result()
            .facts()
            .aggregates
            .trait_(&StandardTrait::Iterator.nominal().declaration)
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
                .ends_with("iter.kgr")
        );
    }
}

#[cfg(test)]
mod interpolation_queries {
    use super::*;
    use crate::analysis::AnalysisDatabase;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn hole_bindings_retain_original_locations_and_survive_snapshot_rebasing() {
        let text =
            "fn render(value:i32)->String { f\"中文 😀 {value}\" }\r\nfn broken() { missing; }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("queries.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let mut db = AnalysisDatabase::default();
        let old = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
            Some(TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32))
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
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
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
    fn join_completion_requires_string_items() {
        for (body, available) in [
            ("[1].", false),
            ("[\"one\"].", true),
            ("[1].iter().", false),
            ("[\"one\"].iter().", true),
            ("val xs: List<i32> = [1]; xs.", false),
            ("val xs: List<String> = [\"one\"]; xs.", true),
            ("val xs: MutableList<String> = [\"one\"]; xs.", true),
        ] {
            let text = format!("fn main() {{ {body} }}");
            let mut sources = SourceDatabase::default();
            let file = sources
                .set("completion.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let mut db = AnalysisDatabase::default();
            let snapshot = db
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let completions = snapshot
                .file(file)
                .unwrap()
                .method_completions(text.find(". }").unwrap() + 1);
            assert_eq!(
                completions.iter().any(|item| item.name == "join"),
                available
            );
        }
    }
}

#[cfg(test)]
mod collection_access_tests {
    use super::*;
    use crate::analysis::AnalysisDatabase;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};
    #[test]
    fn native_collection_witnesses_match_the_declared_interface_signatures() {
        use kagari_abi::standard::traits::StandardTrait as S;
        let mut sources = SourceDatabase::default();
        let root = sources
            .set("contracts.kgr", "fn main() {}".into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let catalog = &snapshot.file(root).unwrap().result().facts().aggregates;
        let integer = TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32);
        let receivers = [
            TypeId::Array(Box::new(integer.clone()), CollectionAccess::Mutable),
            TypeId::Map {
                key: Box::new(integer.clone()),
                value: Box::new(integer.clone()),
                access: CollectionAccess::Mutable,
            },
            TypeId::Set(Box::new(integer), CollectionAccess::Mutable),
        ];
        let mut checked = 0;
        for receiver in receivers {
            for implementation in declarations::implementations(&receiver) {
                let kind = S::from_name(implementation.interface).unwrap();
                if !kind.collection() {
                    continue;
                }
                let arguments = implementation.arguments(&receiver).unwrap();
                let mut interface = kind.nominal();
                interface.arguments = implementation
                    .trait_arguments
                    .iter()
                    .map(|ty| ty.instantiate(&arguments))
                    .collect();
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
                assert_eq!(
                    implementation.methods.len(),
                    contract
                        .methods
                        .iter()
                        .filter(|method| method.default.is_none())
                        .count()
                );
                for method in implementation.methods {
                    let declared = contract
                        .methods
                        .iter()
                        .find(|m| m.name == method.item.name)
                        .unwrap();
                    assert_eq!(
                        method
                            .params
                            .iter()
                            .map(|p| p.ty.instantiate(&arguments))
                            .collect::<Vec<_>>(),
                        declared
                            .params
                            .iter()
                            .map(|p| instantiate(&p.ty))
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        method.result.instantiate(&arguments),
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
        let text = "fn main() { val a = ArrayList::from([1]); val b = ArrayList::from(a); val c: Map<i32,i32> = LinkedHashMap::new(); val d: LinkedHashMap<i32,i32> = LinkedHashMap::new(); val e=LinkedHashSet::from([1]); val f=LinkedHashSet::from([1]); }";
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("constructors.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let file = snapshot.file(file).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let mut identities = std::collections::HashSet::new();
        for spelling in [
            "ArrayList::from",
            "LinkedHashMap::new",
            "LinkedHashSet::from",
        ] {
            let offset = text.find(spelling).unwrap() + spelling.find("::").unwrap() + 2;
            let definition = file.definition_at(offset).unwrap();
            assert!(identities.insert(definition.id.clone()));
            let api = snapshot
                .documentation_at(file.source().id(), offset)
                .unwrap();
            assert!(api.documentation.contains("# Examples"));
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
            ("List<i32>", "ArrayList::from([1])", false, "push"),
            ("ArrayList<i32>", "ArrayList::from([1])", true, "push"),
            (
                "Map<i32,i32>",
                "LinkedHashMap::from([(1,2)])",
                false,
                "insert",
            ),
            (
                "LinkedHashMap<i32,i32>",
                "LinkedHashMap::from([(1,2)])",
                true,
                "insert",
            ),
            ("Set<i32>", "LinkedHashSet::from([1])", false, "insert"),
            (
                "LinkedHashSet<i32>",
                "LinkedHashSet::from([1])",
                true,
                "insert",
            ),
        ] {
            let text = format!("fn main() {{ val values: {annotation}={constructor}; values. }}");
            let mut sources = SourceDatabase::default();
            let id = sources
                .set("completion.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let snapshot = AnalysisDatabase::default()
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let candidates = snapshot
                .file(id)
                .unwrap()
                .method_completions(text.find("values. }").unwrap() + 7);
            assert!(candidates.iter().any(|item| item.name == "len"));
            assert_eq!(candidates.iter().any(|item| item.name == write), mutable);
        }
    }
    #[test]
    fn string_queries_navigate_to_documented_declarations() {
        let text = "fn main() { val text = \"hello\"; text.strip_prefix(\"he\"); }";
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("string-api.kgr", text.into(), SourceLayer::Base)
            .unwrap();
        let snapshot = AnalysisDatabase::default()
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let file = snapshot.file(id).unwrap();
        assert!(
            file.result().diagnostics().is_empty(),
            "{:?}",
            file.result().diagnostics()
        );
        let offset = text.find("strip_prefix").unwrap();
        let definition = file.definition_at(offset).unwrap();
        let api = snapshot.documentation_at(id, offset).unwrap();
        assert!(api.documentation.contains("# Examples"));
        let source = snapshot.source(definition.location.file).unwrap();
        assert_eq!(
            &source.text()[definition.location.range.start..definition.location.range.end],
            "strip_prefix"
        );
    }
    #[test]
    fn list_completion_filters_total_order_requirement() {
        for (element, value, ordered) in [("i32", "1", true), ("f64", "1.0", false)] {
            let text = format!("fn main() {{ val values: List<{element}> = [{value}]; values. }}");
            let mut sources = SourceDatabase::default();
            let id = sources
                .set("list-completion.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let snapshot = AnalysisDatabase::default()
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let items = snapshot
                .file(id)
                .unwrap()
                .method_completions(text.find("values. }").unwrap() + 7);
            assert_eq!(
                items.iter().any(|item| item.name == "binary_search"),
                ordered
            );
            assert!(items.iter().any(|item| item.name == "contains"));
        }
    }
}
