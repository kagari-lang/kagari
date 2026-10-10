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
    Interface(Arc<InterfaceMethodIdentity>),
    Operation(OperationId),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InterfaceMethodIdentity {
    pub(crate) owner: ModuleKey,
    pub(crate) table: usize,
    pub(crate) slot: usize,
    pub(crate) interface: NominalTy<DefinitionId>,
    pub(crate) result_adapter: Option<AdapterIdentity>,
    pub(crate) arguments: Vec<Ty<DefinitionId>>,
    pub(crate) environment: Option<EnvironmentId>,
}

/// Supplied types and their exact provenance are prepared together, independently
/// of receiver values. Host entry constructs these once; linked calls retain them.
#[derive(Debug)]
pub(crate) struct ApplicationArguments {
    values: Vec<TypeArgument>,
    identities: Arc<[Arc<TypeIdentity>]>,
}

impl ApplicationArguments {
    pub(crate) fn values(&self) -> &[TypeArgument] {
        &self.values
    }

    pub(crate) fn new(
        owner: &LoadedModule,
        values: Vec<TypeArgument>,
    ) -> Result<Self, RuntimeError> {
        let identities = values
            .iter()
            .map(|argument| {
                argument
                    .identity(owner)
                    .ok_or_else(|| RuntimeError::module_validation("application type identity"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { values, identities })
    }
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
    arguments: Arc<[Arc<TypeIdentity>]>,
    receiver_operations: Option<OperationGroupId>,
    operations: Option<Arc<Vec<OperationSegment>>>,
}

impl ApplicationKey {
    pub(crate) fn new(
        method: MethodIdentity,
        arguments: &ApplicationArguments,
        receiver_operations: Option<OperationGroupId>,
        operations: &OperationBindings,
    ) -> Self {
        Self {
            method,
            arguments: arguments.identities.clone(),
            receiver_operations,
            operations: operations.identity(),
        }
    }
}
