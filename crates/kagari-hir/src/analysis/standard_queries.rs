//! Standard API queries consume checked semantic targets and the bundled source catalog.

#[cfg(test)]
use crate::builtin::declarations::ApiItemSemantics;
use crate::builtin::declarations::{
    ApiBoundSemantics, ApiImplementationSemantics, ApiTypeSemantics,
};
#[cfg(test)]
use crate::builtin::traits::StandardTraitSemantics;
use kagari_abi::standard::{
    declarations::ApiItem,
    surface::{self as standard_surface, STANDARD_TRAITS, StandardMethodReceiver},
    traits::StandardTrait,
};

use crate::{
    analysis::FileAnalysis,
    builtin::{
        declarations::{self, Arguments},
        traits,
    },
    declarations::{Declaration, DeclarationId},
    hir::ExprKind,
    typeck::CallTarget,
    types::TypeId,
};
#[cfg(test)]
use kagari_common::collection::CollectionAccess;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardSignature {
    pub declaration: &'static ApiItem,
    /// Receiver parameters are omitted for method syntax.
    pub parameters: Vec<(&'static str, TypeId)>,
    pub result: TypeId,
}

impl FileAnalysis {
    pub(super) fn standard_type_definition_at(
        &self,
        offset: usize,
    ) -> Option<&'static Declaration> {
        let facts = self.result.facts();
        let (index, _) = facts
            .lowered
            .source_map
            .type_spans()
            .iter()
            .enumerate()
            .filter(|(_, span)| span.start <= offset && offset < span.end)
            .min_by_key(|(_, span)| span.end - span.start)?;
        let id = facts.lowered.source_map.type_id(index);
        let span = facts.lowered.source_map.type_name_span(id)?;
        if !(span.start <= offset && offset < span.end) {
            return None;
        }
        let resolved = facts.typed.type_table.type_ref(id)?;
        if resolved.target.is_some() {
            return None;
        }
        let item = declarations::native_type(&resolved.ty)?;
        declarations::declaration(&DeclarationId::Definition(item.identity()))
    }

    /// The declaration, Markdown and written signature for a resolved standard symbol.
    pub fn standard_api_at(&self, offset: usize) -> Option<&'static ApiItem> {
        declarations::item(&self.definition_at(offset)?.id)
    }

    /// Instantiate a native call signature using its already checked type arguments.
    /// The smallest enclosing call wins, including positions within its arguments.
    pub fn standard_signature_at(&self, offset: usize) -> Option<StandardSignature> {
        let facts = self.result.facts();
        let (_, id) = facts
            .lowered
            .module
            .body
            .expressions()
            .filter_map(|(id, expr)| {
                if !matches!(expr.kind, ExprKind::Call { .. }) {
                    return None;
                }
                let span = facts.lowered.source_map.expr_span(id);
                (span.start <= offset && offset < span.end).then_some((span.end - span.start, id))
            })
            .min_by_key(|(len, _)| *len)?;
        let call = facts.typed.type_table.call_resolution(id)?;
        if let CallTarget::TraitMethod { method, .. } = &call.target {
            let api = STANDARD_TRAITS
                .iter()
                .flat_map(|t| t.methods)
                .find(|m| m.item.identity() == *method)?;
            let ExprKind::Call { args, .. } = &facts.lowered.module.expr(id).kind else {
                return None;
            };
            return Some(StandardSignature {
                declaration: &api.item,
                parameters: api
                    .params
                    .iter()
                    .skip(usize::from(call.receiver.is_some()))
                    .zip(args)
                    .map(|(p, expr)| {
                        (
                            p.name,
                            facts
                                .typed
                                .type_table
                                .expr_type(*expr)
                                .unwrap_or(TypeId::Unknown),
                        )
                    })
                    .collect(),
                result: facts
                    .typed
                    .type_table
                    .expr_type(id)
                    .unwrap_or(TypeId::Unknown),
            });
        }
        let CallTarget::StandardIntrinsic(intrinsic) = call.target else {
            return None;
        };
        let spec = standard_surface::standard_function_by_intrinsic(intrinsic)?;
        let mut arguments: Arguments = spec
            .type_params
            .iter()
            .copied()
            .zip(call.type_arguments.iter().cloned())
            .collect();
        // Tolerant calls may not have complete inferred type arguments yet.
        for param in spec.type_params {
            arguments.entry(param).or_insert(TypeId::Unknown);
        }
        Some(StandardSignature {
            declaration: declarations::function(intrinsic)?,
            parameters: spec
                .api
                .params
                .iter()
                .skip(usize::from(call.receiver.is_some()))
                .map(|p| (p.name, p.ty.instantiate(&arguments)))
                .collect(),
            result: spec.api.result.instantiate(&arguments),
        })
    }

    /// Native method candidates for a complete or incomplete member expression.
    /// Trait-method completion can compose these with the lexical trait scope.
    pub fn standard_method_completions(&self, offset: usize) -> Vec<&'static ApiItem> {
        let Some(ty) = self.member_receiver_type(offset) else {
            return Vec::new();
        };
        let mut candidates: Vec<_> = standard_surface::standard_methods()
            .iter()
            .filter(|method| {
                matches!(
                    (&ty, method.receiver),
                    (TypeId::Array(_, _), StandardMethodReceiver::Array)
                        | (TypeId::Map { .. }, StandardMethodReceiver::Map)
                        | (TypeId::Set(_, _), StandardMethodReceiver::Set)
                        | (
                            TypeId::Builtin(kagari_abi::scalar::BuiltinType::String),
                            StandardMethodReceiver::String
                        )
                        | (
                            TypeId::StandardEnum {
                                kind: kagari_abi::standard::surface::StandardEnum::Option,
                                ..
                            },
                            StandardMethodReceiver::Option,
                        )
                        | (
                            TypeId::StandardEnum {
                                kind: kagari_abi::standard::surface::StandardEnum::Result,
                                ..
                            },
                            StandardMethodReceiver::Result,
                        )
                )
            })
            .filter(|method| {
                let Some(spec) = standard_surface::standard_function_by_intrinsic(method.intrinsic)
                else {
                    return false;
                };
                let mut arguments: Arguments = spec
                    .type_params
                    .iter()
                    .map(|name| (*name, TypeId::Unknown))
                    .collect();
                let receiver = &spec.api.params[0].ty;
                receiver.infer(&ty, &mut arguments);
                !receiver.instantiate(&arguments).conflicts_with(&ty)
                    || ty.can_weaken_to(&receiver.instantiate(&arguments))
            })
            .filter_map(|method| declarations::function(method.intrinsic))
            .collect();
        for implementation in declarations::implementations(&ty) {
            candidates.extend(
                implementation
                    .methods
                    .iter()
                    .filter(|m| m.params.first().is_some_and(|p| p.name == "self"))
                    .map(|m| &m.item),
            );
            candidates.extend(
                implementation
                    .trait_declaration()
                    .methods
                    .iter()
                    .filter(|m| m.params.first().is_some_and(|p| p.name == "self"))
                    .filter(|method| {
                        !implementation
                            .methods
                            .iter()
                            .any(|m| m.item.path.last() == method.item.path.last())
                    })
                    .map(|m| &m.item),
            );
        }
        if let TypeId::Trait(interface) = &ty {
            for parent in self
                .result
                .facts()
                .aggregates
                .trait_closure(interface, &ty, &Default::default())
                .unwrap_or_default()
            {
                if let Some(contract) = STANDARD_TRAITS
                    .iter()
                    .find(|t| t.item.identity() == parent.declaration)
                {
                    candidates.extend(
                        contract
                            .methods
                            .iter()
                            .filter(|m| m.params.first().is_some_and(|p| p.name == "self"))
                            .map(|m| &m.item),
                    );
                }
            }
        }
        // Source-declared associated-item constraints also govern completion.
        candidates.retain(|item| {
            let Some(method) = STANDARD_TRAITS
                .iter()
                .flat_map(|t| t.methods)
                .find(|m| m.item.identity() == item.identity())
            else {
                return true;
            };
            let mut arguments = Arguments::new();
            arguments.insert("Self", ty.clone());
            method.bounds.iter().all(|(target, constraints)| {
                if !matches!(
                    target,
                    kagari_abi::standard::declarations::ApiType::Named("Self", _)
                ) {
                    let owner = STANDARD_TRAITS
                        .iter()
                        .find(|t| {
                            t.methods
                                .iter()
                                .any(|m| m.item.identity() == item.identity())
                        })
                        .unwrap();
                    let owner_id = owner.item.identity();
                    let mut bindings = arguments.clone();
                    if let Some(implementation) = declarations::implementations(&ty)
                        .into_iter()
                        .find(|i| i.trait_declaration().item.identity() == owner_id)
                    {
                        bindings.extend(implementation.arguments(&ty).unwrap());
                    } else if let TypeId::Trait(interface) = &ty
                        && let Some(parent) = self
                            .result
                            .facts()
                            .aggregates
                            .trait_closure(interface, &ty, &Default::default())
                            .unwrap_or_default()
                            .into_iter()
                            .find(|n| n.declaration == owner_id)
                    {
                        bindings.extend(owner.generics.iter().copied().zip(parent.arguments));
                    }
                    let actual = target.instantiate(&bindings);
                    if actual.is_unresolved() {
                        return true;
                    }
                    return constraints.iter().all(|constraint| {
                        let required = constraint.nominal(&bindings);
                        traits::intrinsic_applies(
                            &required,
                            &actual,
                            Some(&self.result.facts().aggregates),
                            &Default::default(),
                        ) || self
                            .result
                            .facts()
                            .aggregates
                            .concrete_interface_implementation(
                                &required,
                                &actual,
                                &Default::default(),
                                4096,
                                64,
                                &Default::default(),
                            )
                            .is_ok_and(|i| i.is_some())
                    });
                }
                constraints.iter().all(|constraint| {
                    let Some(kind @ (StandardTrait::Iterator | StandardTrait::Iterable)) =
                        StandardTrait::from_name(constraint.name)
                    else {
                        return true;
                    };
                    let required = constraint.nominal(&arguments);
                    let outputs = traits::iteration_outputs(
                        kind,
                        &ty,
                        Some(&self.result.facts().aggregates),
                        &Default::default(),
                    );
                    required.associated_types.iter().all(|(member, expected)| {
                        outputs
                            .as_ref()
                            .and_then(|items| items.get(member))
                            .is_some_and(|actual| !actual.conflicts_with(expected))
                    })
                })
            })
        });
        candidates
    }
}

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
            let candidates = analysis.standard_method_completions(
                source.find(receiver).unwrap() + receiver.find('.').unwrap() + 1,
            );
            for name in expected {
                let matches = candidates
                    .iter()
                    .filter(|m| m.path.last().unwrap().1 == name)
                    .collect::<Vec<_>>();
                assert_eq!(matches.len(), 1, "{name}");
                let api = matches[0];
                let declaration = snapshot
                    .declaration(&DeclarationId::Definition(api.identity()))
                    .unwrap();
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
        let candidates = analysis.standard_method_completions(text.find("values. }").unwrap() + 7);
        for name in ["next", "map", "filter", "collect"] {
            let matches: Vec<_> = candidates
                .iter()
                .filter(|m| m.path.last().unwrap().1 == name)
                .collect();
            assert_eq!(matches.len(), 1, "{name}");
            let api = matches[0];
            assert_eq!(
                api.path[0].0,
                if name == "next" {
                    kagari_common::identity::DefinitionKind::Impl
                } else {
                    kagari_common::identity::DefinitionKind::Trait
                }
            );
            let declaration = snapshot
                .declaration(&DeclarationId::Definition(api.identity()))
                .unwrap();
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
        assert_eq!(
            declarations::declaration(&DeclarationId::Definition(method.identity())),
            Some(&method.declaration())
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
            let api = analysis.standard_api_at(offset).unwrap();
            assert!(!api.documentation.is_empty());
            assert_eq!(snapshot.declaration(&definition.id), Some(definition));
        }
        let signature = analysis
            .standard_signature_at(text.find("lookup(values").unwrap())
            .unwrap();
        assert_eq!(
            signature.parameters[0].1,
            TypeId::Array(
                Box::new(TypeId::Builtin(kagari_abi::scalar::BuiltinType::I32)),
                CollectionAccess::Mutable
            )
        );
        let signature = analysis
            .standard_signature_at(text.find("is_ok()").unwrap())
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
        let candidates = old.file(file).unwrap().standard_method_completions(offset);
        assert!(
            candidates
                .iter()
                .any(|item| item.path.last().unwrap().1 == "len_bytes")
        );
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
            old.file(file).unwrap().standard_method_completions(offset),
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
            let api = analysis.standard_api_at(offset).unwrap();
            assert_eq!(api.path.last().unwrap().1, name);
            assert!(api.uri.ends_with("iter.kgr"));
            let signature = analysis.standard_signature_at(offset).unwrap();
            assert_eq!(signature.declaration, api);
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
            let item = analysis
                .standard_api_at(text.find(needle).unwrap())
                .unwrap();
            assert_eq!(item.path.last().unwrap().1, name);
            let declaration = item.declaration();
            assert!(
                snapshot
                    .source(declaration.location.file)
                    .unwrap()
                    .text()
                    .contains("///")
            );
        }
        assert!(
            analysis
                .standard_api_at(text.find("get(7)").unwrap())
                .is_none()
        );
        let iterator = StandardTrait::Iterator.contract();
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
                .standard_method_completions(text.find(". }").unwrap() + 1);
            assert_eq!(
                completions
                    .iter()
                    .any(|item| item.path.last().unwrap().1 == "join"),
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
                let contract = kind.contract();
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
                        .filter(|method| !method.has_default)
                        .count()
                );
                for method in implementation.methods {
                    let declared = contract
                        .methods
                        .iter()
                        .find(|m| m.name == method.item.path.last().unwrap().1)
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
            let api = file.standard_api_at(offset).unwrap();
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
                .standard_method_completions(text.find("values. }").unwrap() + 7);
            assert!(
                candidates
                    .iter()
                    .any(|item| item.path.last().unwrap().1 == "len")
            );
            assert_eq!(
                candidates
                    .iter()
                    .any(|item| item.path.last().unwrap().1 == write),
                mutable
            );
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
        let api = file.standard_api_at(offset).unwrap();
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
                .standard_method_completions(text.find("values. }").unwrap() + 7);
            assert_eq!(
                items
                    .iter()
                    .any(|item| item.path.last().unwrap().1 == "binary_search"),
                ordered
            );
            assert!(
                items
                    .iter()
                    .any(|item| item.path.last().unwrap().1 == "contains")
            );
        }
    }
}
