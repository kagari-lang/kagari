//! Standard API queries consume checked semantic targets and the bundled source catalog.
use super::*;
use crate::{
    builtin::{
        declarations::{self, ApiItem, Arguments},
        surface,
    },
    declarations::{Declaration, DeclarationId},
    typeck::CallTarget,
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
        let CallTarget::StandardIntrinsic(intrinsic) = call.target else {
            return None;
        };
        let spec = surface::standard_function_by_intrinsic(intrinsic)?;
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
        use surface::StandardMethodReceiver as Receiver;
        let Some(ty) = self.member_receiver_type(offset) else {
            return Vec::new();
        };
        surface::standard_methods()
            .iter()
            .filter(|method| match (&ty, method.receiver) {
                (TypeId::Array(_, _), Receiver::Array)
                | (TypeId::Map { .. }, Receiver::Map)
                | (TypeId::Set(_, _), Receiver::Set)
                | (TypeId::Builtin(crate::types::BuiltinType::String), Receiver::String)
                | (
                    TypeId::StandardEnum {
                        kind: surface::StandardEnum::Option,
                        ..
                    },
                    Receiver::Option,
                )
                | (
                    TypeId::StandardEnum {
                        kind: surface::StandardEnum::Result,
                        ..
                    },
                    Receiver::Result,
                ) => true,
                (_, Receiver::Iterable) => surface::iterable_protocol(&ty).is_some(),
                _ => false,
            })
            .filter(|method| {
                let Some(spec) = surface::standard_function_by_intrinsic(method.intrinsic) else {
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
            })
            .filter_map(|method| declarations::function(method.intrinsic))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::source_database::{SourceDatabase, SourceLayer};

    #[test]
    fn native_calls_types_and_variants_navigate_to_documented_source() {
        let text = "use std::array::get as lookup; fn main() { val value: Result<i32,String> = Ok(7); val values=[7]; lookup(values, values.len()); value.is_ok(); }";
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
                Box::new(TypeId::Builtin(crate::types::BuiltinType::I32)),
                CollectionAccess::Mutable
            )
        );
        let signature = analysis
            .standard_signature_at(text.find("is_ok()").unwrap())
            .unwrap();
        assert!(signature.parameters.is_empty());
        assert_eq!(
            signature.result,
            TypeId::Builtin(crate::types::BuiltinType::Bool)
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
    use kagari_common::source_database::{SourceDatabase, SourceLayer};
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
        let iterator = crate::builtin::traits::StandardTrait::Iterator.contract();
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
            Some(TypeId::Builtin(crate::types::BuiltinType::I32))
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
    fn join_completion_requires_a_string_array_receiver() {
        for (array, available) in [("[1]", false), ("[\"one\"]", true)] {
            let text = format!("fn main() {{ {array}. }}");
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
