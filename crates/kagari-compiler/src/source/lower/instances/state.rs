//! Owned planner inventories survive processing contexts and candidate snapshots.

use crate::source::lower::instances::{Instance, InstanceKey, InterfaceBridgeRequest};
use kagari_common::{
    identity::table::{DefinitionId, module::ModuleHandle},
    span::Span,
};
use kagari_contract::{
    native_import::NativeImport, statics::StaticDescriptor, types::ConcreteFunctionIdentity,
};
use kagari_hir::{hir::ids::StaticId, types::TypeId};
use kagari_mir::ids::InstanceId;
use kagari_source::diagnostic::Diagnostic;
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Default)]
pub(in crate::source::lower) struct InstanceState {
    pub instances: Vec<Instance>,
    pub static_slots: Vec<StaticDescriptor<InstanceId>>,
    pub layout_roots: Vec<(TypeId, Span)>,
    pub host_types: BTreeSet<DefinitionId>,
    pub interface_bridges: Vec<InterfaceBridgeRequest>,
    pub native_targets: Vec<NativeImport>,
    pub interface_instances: Vec<ConcreteFunctionIdentity>,
    pub keys: HashMap<InstanceKey, InstanceId>,
    pub local_function_placements: HashMap<DefinitionId, DefinitionId>,
    pub static_lexical_tokens: HashMap<(ModuleHandle, StaticId), Vec<Span>>,
    pub generic_count: usize,
    pub interfaces: HashSet<(DefinitionId, Vec<TypeId>)>,
    pub instruction_count: usize,
    pub failure: Option<Diagnostic>,
}
