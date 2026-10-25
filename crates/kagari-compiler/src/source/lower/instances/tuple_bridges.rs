//! Shared intrinsic tuple tables carry explicit element capabilities.
use crate::source::lower::{
    MirLoweringError,
    instances::{InstancePlanner, InterfaceBridgeRequest, bridge_identities::BridgeKey},
};
use kagari_common::{identity::mapping::DefinitionMappingError, span::Span};
use kagari_contract::types::ConcreteFunctionIdentity;
use kagari_hir::types::{GenericParameterType, NominalType, TypeId};
use kagari_types::{language::Protocol, ty::GenericParam};

impl InstancePlanner<'_> {
    pub(super) fn tuple_interface_bridge(
        &mut self,
        items: &[TypeId],
        interface: &NominalType,
        span: Span,
    ) -> Result<ConcreteFunctionIdentity, MirLoweringError> {
        let protocol = Protocol::from_definition(&interface.declaration, &self.definitions())
            .filter(|protocol| {
                matches!(
                    protocol,
                    Protocol::PartialEq | Protocol::Eq | Protocol::Hash
                )
            })
            .ok_or(MirLoweringError::MissingBinding("tuple interface protocol"))?;
        let existing = self
            .interface_bridges
            .iter()
            .find(|request| {
                request.parameters.len() == items.len()
                    && !request.parameters.is_empty()
                    && &request.interface == interface
            })
            .map(|request| request.declaration);
        let declaration = if let Some(declaration) = existing {
            declaration
        } else {
            let root = self
                .context
                .intern_root(self.module.lowered.source.module_identity())
                .map_err(DefinitionMappingError::from)?;
            self.charge_layout_instance(span)?;
            let declaration = self.bridge_identities.intern(
                self.context,
                root,
                self.module.lowered.module.impls.len(),
                BridgeKey::Tuple(items.len(), interface.clone()),
            )?;
            let parameters: Vec<_> = (0..items.len())
                .map(|position| GenericParam {
                    owner: declaration,
                    position,
                })
                .collect();
            let receiver = TypeId::Tuple(
                parameters
                    .iter()
                    .map(|parameter| {
                        TypeId::Generic(GenericParameterType {
                            owner: parameter.owner,
                            position: parameter.position,
                            name: format!("T{}", parameter.position),
                        })
                    })
                    .collect(),
            );
            if protocol != Protocol::Eq {
                self.enqueue_selected_protocol(protocol, &receiver, interface, span, true)?;
            }
            self.interface_instances.push(ConcreteFunctionIdentity {
                declaration,
                arguments: parameters.iter().map(GenericParam::as_type).collect(),
            });
            self.interface_bridges.push(InterfaceBridgeRequest {
                declaration,
                parameters,
                receiver,
                interface: interface.clone(),
                span,
            });
            declaration
        };
        Ok(ConcreteFunctionIdentity {
            declaration,
            arguments: items.iter().map(|item| self.encode_type(item)).collect(),
        })
    }
}
