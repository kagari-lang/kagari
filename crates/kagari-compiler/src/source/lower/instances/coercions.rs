//! Plan source-local callable coercions with their actual enclosing computation.
use crate::source::lower::{
    MirLoweringError,
    instances::{CallableInstance, Instance, InstanceClassification, InstanceKey, InstancePlanner},
};
use kagari_common::identity::{DefinitionKind, mapping::DefinitionMappingError};
use kagari_hir::types::TypeId;
use kagari_mir::ids::InstanceId;

impl InstancePlanner<'_> {
    pub fn enqueue_callable(
        &mut self,
        parent: &Instance,
        mut body: CallableInstance,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        body.receiver = self
            .arguments(&[body.receiver], &parent.substitution, body.span)?
            .remove(0);
        let TypeId::Trait(interface) = self
            .arguments(
                &[TypeId::Trait(body.interface)],
                &parent.substitution,
                body.span,
            )?
            .remove(0)
        else {
            unreachable!()
        };
        body.interface = interface;
        let declaration = self
            .context
            .intern_child(
                parent.key.declaration,
                DefinitionKind::Function,
                "$callable",
                body.span.start as u32,
            )
            .map_err(DefinitionMappingError::from)?;
        let key = InstanceKey {
            declaration,
            arguments: vec![body.receiver.clone(), TypeId::Trait(body.interface.clone())],
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        self.charge_layout_instance(body.span)?;
        self.record_layout_root(&body.receiver, &Default::default(), body.span)?;
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            classification: InstanceClassification::Adapter(parent.lower(
                self.catalog,
                self.options,
                &self.definitions(),
                body.span,
            )?),
            resume: false,
            callable: Some(body),
            origin: parent.origin,
            id,
            body: parent.body,
            key,
            substitution: parent.substitution.clone(),
            closure: None,
            protocol: None,
        });
        Ok(id)
    }
}
