//! Reserve supplier-local initializer indices before planning any ordinary callable.
use crate::source::lower::{
    MirLoweringError,
    instances::{
        Instance, InstanceBody, InstanceClassification, InstanceKey, InstancePlanner,
        limit_diagnostic,
    },
    statics::static_contract,
};
use kagari_common::identity::{DefinitionKind, mapping::DefinitionMappingError};
use kagari_contract::statics::StaticDescriptor;
use kagari_hir::{AnalyzedModule, imports::SourceItem, resolver::resolved::ResolvedName};
use kagari_mir::ids::{InstanceId, ModuleSlotId};

impl InstancePlanner<'_> {
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
