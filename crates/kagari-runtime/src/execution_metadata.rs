//! Executable metadata edges are separate from immutable layout provenance.
pub(crate) mod application_key;
pub(crate) mod applications;
pub(crate) mod call_contracts;
mod environment_transport;
pub(crate) mod environments;
pub(crate) mod groups;
pub(crate) mod identity;
pub(crate) mod managed;
pub(crate) mod operation;

use crate::{
    execution_metadata::{
        applications::ApplicationId,
        environments::EnvironmentId,
        groups::{OperationGroupId, OperationId},
    },
    frame::types::TypeEnvironment,
    module::ExecutableModule,
    objects::interfaces::description::{InterfaceDescription, InterfaceResultBinding},
};

/// Immutable direct relationships consumed only with actual protected providers.
/// Semantic identities select inside that supplied graph; none is a root or an
/// independent table lookup capability. Descriptor bindings expand locally.
#[derive(Clone, Copy)]
pub(crate) enum MetadataEdge<'a> {
    Program(&'a ExecutableModule),
    Environment(EnvironmentId),
    EnvironmentFacts(&'a TypeEnvironment),
    Operation(OperationId),
    Group(OperationGroupId),
    InterfaceDescription(&'a InterfaceDescription),
    Application(ApplicationId),
    Binding(&'a InterfaceResultBinding),
}
