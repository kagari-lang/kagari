//! Applied executable identity contains checked metadata identities, never receivers.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        environments::EnvironmentId,
        groups::{OperationGroupId, OperationId},
    },
    frame::types::{
        arguments::TypeArgument,
        compatibility::TypeIdentity,
        operations::{OperationBindings, OperationSegment},
    },
    module::{LoadedModule, ModuleKey},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{NominalTy, Ty};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum MethodIdentity {
    Interface {
        owner: ModuleKey,
        table: usize,
        slot: usize,
        interface: NominalTy<DefinitionId>,
        result_adapter: Option<AdapterIdentity>,
        arguments: Vec<Ty<DefinitionId>>,
        environment: Option<EnvironmentId>,
    },
    Operation(OperationId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AdapterIdentity {
    pub(crate) owner: ModuleKey,
    pub(crate) table: usize,
    pub(crate) arguments: Vec<Ty<DefinitionId>>,
    pub(crate) environment: Option<EnvironmentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ApplicationKey {
    method: MethodIdentity,
    arguments: Vec<TypeIdentity>,
    receiver_operations: Option<OperationGroupId>,
    operations: Option<Arc<Vec<OperationSegment>>>,
}

impl ApplicationKey {
    pub(crate) fn new(
        method: MethodIdentity,
        owner: &LoadedModule,
        arguments: &[TypeArgument],
        receiver_operations: Option<OperationGroupId>,
        operations: &OperationBindings,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            method,
            arguments: arguments
                .iter()
                .map(|argument| {
                    argument
                        .view(owner)
                        .identity()
                        .ok_or_else(|| RuntimeError::module_validation("application type identity"))
                })
                .collect::<Result<_, _>>()?,
            receiver_operations,
            operations: operations.identity(),
        })
    }
}
