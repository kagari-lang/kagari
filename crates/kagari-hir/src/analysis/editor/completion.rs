//! Bounded visible names and receiver members from the editor's retained semantic context.

use crate::{
    aggregates::implementations::ImplementationSearchError,
    analysis::editor::EditorAnalysis,
    declarations::DeclarationId,
    imports::{
        ResolvedTarget, SourceItem,
        catalog::{LookupContext, LookupResult, NamespaceResult},
    },
    types::{TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_types::declaration::names::NameNamespace;
use std::{collections::BTreeMap, slice::from_ref};

/// One admissible source spelling with semantic navigation and type facts when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    pub name: String,
    pub declaration: Option<DeclarationId>,
    pub ty: Option<TypeId>,
}

/// A deterministic replacement range and a complete bounded candidate set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completions {
    /// UTF-8 byte range in the retained physical source, including an existing suffix.
    pub replacement: Span,
    pub candidates: Vec<CompletionCandidate>,
}

impl EditorAnalysis {
    /// Collects visible lexical, module/path or receiver candidates.
    ///
    /// Lookup uses the retained namespace catalog and checked receiver surfaces.
    /// Search exhaustion or more than `limit` candidates returns an explicit error.
    pub fn completions(
        &self,
        offset: usize,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Result<Completions, ImplementationSearchError> {
        let cancelled = |_| ImplementationSearchError::Cancelled;
        cancel.check().map_err(cancelled)?;
        let text = self.source().text();
        let Some(before) = text.get(..offset) else {
            return Ok(Completions {
                replacement: Span::new(offset, offset),
                candidates: vec![],
            });
        };
        let identifier = |c: char| c == '_' || c.is_alphanumeric();
        let start = before
            .char_indices()
            .rev()
            .find(|(_, c)| !identifier(*c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let end = text[offset..]
            .char_indices()
            .find(|(_, c)| !identifier(*c))
            .map_or(text.len(), |(i, _)| offset + i);
        let replacement = Span::new(start, end);
        let prefix = &text[start..offset];
        let mut candidates = BTreeMap::<String, CompletionCandidate>::new();
        let mut insert = |candidate: CompletionCandidate| -> Result<(), ImplementationSearchError> {
            cancel.check().map_err(cancelled)?;
            if candidate.name.starts_with(prefix) {
                candidates
                    .entry(candidate.name.clone())
                    .or_insert(candidate);
                if candidates.len() > limit {
                    return Err(ImplementationSearchError::LimitExceeded);
                }
            }
            Ok(())
        };
        if before[..start].ends_with('.') {
            if let Some(facts) = self.editor_facts() {
                if let Some(TypeId::Struct(receiver)) = self.member_receiver_type(offset)
                    && let Some(structure) = facts.aggregates.structure(&receiver.declaration)
                {
                    let substitution: TypeSubstitution = structure
                        .generic_params
                        .iter()
                        .cloned()
                        .zip(receiver.arguments.iter().cloned())
                        .collect();
                    for field in &structure.fields {
                        cancel.check().map_err(cancelled)?;
                        let owner = self
                            .definitions()
                            .resolve(field.owner)
                            .expect("checked field owner");
                        if field
                            .visibility
                            .allows(owner.module(), self.source().module_identity())
                        {
                            insert(CompletionCandidate {
                                name: field.name.clone(),
                                declaration: Some(field.declaration.id.clone()),
                                ty: Some(field.ty.instantiate(&substitution)),
                            })?;
                        }
                    }
                }
                for method in self.method_completions(offset, cancel)? {
                    insert(CompletionCandidate {
                        name: method.name,
                        declaration: Some(method.declaration),
                        ty: None,
                    })?;
                }
            }
        } else {
            let names = &self.file.prepared.names.facts;
            let catalog = &names.catalog;
            let importer = catalog
                .definitions()
                .lookup_module(self.source().module_identity())
                .expect("checked importer");
            let context = LookupContext::new(&importer);
            let path_text = &before[..start];
            let path = path_text
                .char_indices()
                .rev()
                .find(|(_, c)| !(identifier(*c) || *c == ':'))
                .map_or(path_text, |(i, c)| &path_text[i + c.len_utf8()..]);
            if let Some(path) = path.strip_suffix("::") {
                let components: Vec<String> = path.split("::").map(str::to_owned).collect();
                if let Some(root) = components.first() {
                    let mut target = names.items.hit(root, NameNamespace::Type);
                    if !matches!(target, LookupResult::Found(_)) {
                        target = catalog
                            .absolute(&context, from_ref(root), NameNamespace::Type, cancel)
                            .map_err(cancelled)?;
                    }
                    let target = catalog
                        .resolve_path(
                            &context,
                            target,
                            &components[1..],
                            NameNamespace::Type,
                            cancel,
                        )
                        .map_err(cancelled)?;
                    if let LookupResult::Found(target) = target
                        && let NamespaceResult::Found(namespace) = catalog
                            .namespace_of(&context, &target.target, cancel)
                            .map_err(cancelled)?
                        && let Some(table) = catalog.namespaces.get(&namespace)
                    {
                        for name in table.names.entries.keys() {
                            for category in [NameNamespace::Type, NameNamespace::Value] {
                                if let LookupResult::Found(hit) = catalog
                                    .lookup_member(
                                        &context,
                                        &namespace,
                                        name.as_str(),
                                        category,
                                        cancel,
                                    )
                                    .map_err(cancelled)?
                                {
                                    insert(self.completion_target(name.as_str(), hit.target))?;
                                }
                            }
                        }
                    }
                }
            } else {
                for name in names.items.entries.keys() {
                    for category in [NameNamespace::Type, NameNamespace::Value] {
                        if let LookupResult::Found(hit) = names.items.hit(name.as_str(), category) {
                            insert(self.completion_target(name.as_str(), hit.target))?;
                        }
                    }
                }
                // Lexical bindings take precedence over module spellings.
                for binding in self.visible_bindings(offset) {
                    cancel.check().map_err(cancelled)?;
                    if binding.declaration.name.starts_with(prefix) {
                        candidates.insert(
                            binding.declaration.name.clone(),
                            CompletionCandidate {
                                name: binding.declaration.name,
                                declaration: Some(binding.declaration.id),
                                ty: Some(binding.ty),
                            },
                        );
                        if candidates.len() > limit {
                            return Err(ImplementationSearchError::LimitExceeded);
                        }
                    }
                }
            }
        }
        cancel.check().map_err(cancelled)?;
        Ok(Completions {
            replacement,
            candidates: candidates.into_values().collect(),
        })
    }

    fn completion_target(&self, name: &str, target: ResolvedTarget) -> CompletionCandidate {
        let declaration = if let ResolvedTarget::Source(target) = target {
            self.signatures
                .file(target.unit.file)
                .and_then(|file| {
                    if !target
                        .unit
                        .matches(&file.prepared.lowered, file.definitions())
                    {
                        return None;
                    }
                    if let SourceItem::Variant(id) = target.item {
                        file.declarations().variant(id)
                    } else {
                        file.declarations().target(target.item.local()?)
                    }
                })
                .map(|declaration| declaration.id.clone())
        } else {
            None
        };
        CompletionCandidate {
            name: name.into(),
            declaration,
            ty: None,
        }
    }
}
