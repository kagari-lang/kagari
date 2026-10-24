//! Reserve supplier-local initializer indices before planning any ordinary callable.
use crate::source::lower::{
    MirLoweringError,
    instances::{
        Instance, InstanceBody, InstanceClassification, InstanceKey, InstancePlanner,
        limit_diagnostic,
    },
    statics::static_contract,
};
use kagari_common::{
    identity::{DefinitionKind, mapping::DefinitionMappingError},
    span::Span,
};
use kagari_contract::statics::StaticDescriptor;
use kagari_hir::{AnalyzedModule, imports::SourceItem, resolver::resolved::ResolvedName};
use kagari_mir::ids::{InstanceId, ModuleSlotId};
use kagari_syntax::{lexer::lex_with_cancellation, token::TokenKind};
use std::collections::hash_map::Entry;

impl InstancePlanner<'_> {
    /// Use an initializer-relative token position, not an arena index or byte offset.
    /// Nested generated bodies retain `InstanceBody::Static`, so they use the same
    /// lexical coordinates while their parent identity keeps each scope distinct.
    pub(super) fn static_lexical_position(
        &mut self,
        parent: &Instance,
        site: Span,
    ) -> Result<Option<u32>, MirLoweringError> {
        let InstanceBody::Static(id) = parent.body else {
            return Ok(None);
        };
        self.check()?;
        let source = self.origin(parent);
        let initializer = source
            .lowered
            .source_map
            .static_initializer_span(id)
            .ok_or(MirLoweringError::MissingBinding(
                "static initializer lexical span",
            ))?;
        if site.start < initializer.start || site.end > initializer.end {
            return Err(MirLoweringError::MissingBinding(
                "static generated body lexical site",
            ));
        }
        let tokens = match self.static_lexical_tokens.entry((parent.origin, id)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let text = source
                    .lowered
                    .source
                    .text()
                    .get(initializer.start..initializer.end)
                    .ok_or(MirLoweringError::MissingBinding(
                        "static initializer source",
                    ))?;
                let tokens = lex_with_cancellation(text, &self.options.cancel)
                    .map_err(|_| MirLoweringError::Cancelled)?;
                let mut meaningful = Vec::new();
                for token in tokens {
                    self.options
                        .cancel
                        .check()
                        .map_err(|_| MirLoweringError::Cancelled)?;
                    if !matches!(
                        token.kind,
                        TokenKind::Whitespace
                            | TokenKind::LineComment
                            | TokenKind::BlockComment
                            | TokenKind::Eof
                    ) {
                        meaningful.push(Span {
                            start: initializer.start + token.span.start,
                            end: initializer.start + token.span.end,
                        });
                    }
                }
                entry.insert(meaningful)
            }
        };
        let position = tokens.partition_point(|token| token.end <= site.start);
        if tokens
            .get(position)
            .is_none_or(|token| token.start != site.start)
        {
            return Err(MirLoweringError::MissingBinding(
                "static generated body token",
            ));
        }
        Ok(Some(u32::try_from(position).map_err(|_| {
            MirLoweringError::MissingBinding("static generated body token index")
        })?))
    }

    pub(crate) fn enqueue_statics(&mut self) -> Result<(), MirLoweringError> {
        let module = self.module;
        let root = self
            .context
            .intern_root(module.lowered.source.module_identity())
            .map_err(DefinitionMappingError::from)?;
        // This prefix is shared by all importing planners. No layout/adapter work
        // may enqueue a callable until every initializer has its reserved index.
        for item in &module.lowered.module.statics {
            self.check()?;
            let span = module.lowered.source_map.static_span(item.id);
            if self.static_slots.len() >= u16::MAX as usize {
                return Err(MirLoweringError::diagnostic(limit_diagnostic(
                    "module slots",
                    u16::MAX as usize,
                    span,
                )));
            }
            let declaration = self
                .context
                .intern_child(
                    root,
                    DefinitionKind::Function,
                    &format!("$static_init_{}", item.name),
                    0,
                )
                .map_err(DefinitionMappingError::from)?;
            let key = InstanceKey {
                declaration,
                arguments: Vec::new(),
            };
            let id = InstanceId::new(self.instances.len());
            self.keys.insert(key.clone(), id);
            self.instances.push(Instance {
                classification: InstanceClassification::StaticInitializer(
                    static_contract(module, item.id).declaration,
                ),
                callable: None,
                resume: false,
                origin: self.owner_handle(),
                id,
                body: InstanceBody::Static(item.id),
                key,
                substitution: Default::default(),
                closure: None,
                protocol: None,
            });
            self.static_slots.push(StaticDescriptor {
                contract: static_contract(module, item.id),
                initializer: id,
            });
        }
        for item in &module.lowered.module.statics {
            self.record_layout_root(
                &module.typed.statics[&item.id],
                &Default::default(),
                module.lowered.source_map.static_span(item.id),
            )?;
        }
        Ok(())
    }

    pub(crate) fn static_slot(
        &mut self,
        source: &AnalyzedModule,
        resolved: &ResolvedName,
    ) -> Result<Option<ModuleSlotId>, MirLoweringError> {
        self.check()?;
        let declaration = match resolved {
            ResolvedName::Static(id) => static_contract(source, *id).declaration,
            ResolvedName::Source(reference) if matches!(reference.item, SourceItem::Static(_)) => {
                source
                    .imported_statics
                    .get(reference)
                    .ok_or(MirLoweringError::MissingBinding("checked imported static"))?
                    .declaration
            }
            _ => return Ok(None),
        };
        if let Some(index) = self
            .static_slots
            .iter()
            .position(|slot| slot.contract.declaration == declaration)
        {
            return Ok(Some(ModuleSlotId::new(index)));
        }
        let supplier = *self.modules.get(&self.module_handle(declaration)).ok_or(
            MirLoweringError::MissingBinding("static supplier source module"),
        )?;
        let Some(ResolvedName::Static(id)) = supplier.declarations.definition_target(&declaration)
        else {
            return Err(MirLoweringError::MissingBinding(
                "static supplier declaration",
            ));
        };
        let initializer = supplier
            .lowered
            .module
            .statics
            .iter()
            .position(|item| item.id == id)
            .ok_or(MirLoweringError::MissingBinding("static initializer index"))?;
        let contract = static_contract(supplier, id);
        let span = supplier.lowered.source_map.static_span(id);
        self.record_layout_root(&supplier.typed.statics[&id], &Default::default(), span)?;
        if self.static_slots.len() >= u16::MAX as usize {
            return Err(MirLoweringError::diagnostic(limit_diagnostic(
                "module slots",
                u16::MAX as usize,
                span,
            )));
        }
        let slot = ModuleSlotId::new(self.static_slots.len());
        self.static_slots.push(StaticDescriptor {
            contract,
            initializer: InstanceId::new(initializer),
        });
        Ok(Some(slot))
    }
}
