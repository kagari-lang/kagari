use crate::source::lower::{
    MirLoweringError, ModuleLoweringInputs, instances::state::InstanceState,
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::DefinitionMappingError,
        table::{DefinitionId, DefinitionTable, module::ModuleHandle},
    },
    span::Span,
};
use kagari_contract::{
    native_import::NativeImport, representation::semantic_representation,
    statics::StaticDescriptor, types::ConcreteFunctionIdentity, updates::BodyClassification,
};
use kagari_hir::{
    AnalyzedModule,
    aggregates::{
        AggregateCatalog, implementations::ImplementationSearchLimits, traits::MethodDefault,
    },
    declarations::DeclarationId,
    hir::{
        expr::ExprKind,
        ids::{BodyOwner, ExprId, FunctionId, StaticId},
    },
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, GenericBounds, TypedFunction, scalar::ScalarValue},
    types::{
        GenericParameterType, NominalType, TypeId, TypeSubstitution,
        semantic::{lower_nominal_type, lower_type},
    },
};
use kagari_mir::{ids::InstanceId, passes::PassOptions};
use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};
use kagari_types::{
    language::Protocol,
    ty::{GenericParam, NominalTy, Ty},
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    slice,
};

pub(crate) mod bridge_identities;

use bridge_identities::{BridgeIdentities, BridgeKey};

mod adapters;
mod callables;
mod calls;
mod coercions;
mod constructions;
mod defaults;
mod locals;
mod native;
mod operation_dependencies;
mod requests;
mod results;
mod shared;
pub(super) mod state;
mod statics;
mod tuple_bridges;
mod views;

#[derive(Debug, Clone)]
pub struct MirLoweringOptions {
    /// Optional bounded MIR simplification; None preserves the diagnostic lowering form.
    pub optimization: Option<PassOptions>,
    pub max_generic_instances: usize,
    pub max_type_nodes: usize,
    pub max_type_depth: usize,
    pub max_instructions: usize,
    pub cancel: CancellationToken,
}

impl Default for MirLoweringOptions {
    fn default() -> Self {
        Self {
            optimization: None,
            max_generic_instances: 1024,
            max_type_nodes: 8192,
            max_type_depth: 64,
            max_instructions: 1_000_000,
            cancel: CancellationToken::default(),
        }
    }
}

/// Checked source arguments remain local to specialization planning. They are
/// converted only when a concrete MIR identity is emitted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct InstanceKey {
    pub declaration: DefinitionId,
    pub arguments: Vec<TypeId>,
}

#[derive(Debug, Clone)]
pub(super) struct Instance {
    pub classification: InstanceClassification,
    pub callable: Option<CallableInstance>,
    pub resume: bool,
    pub origin: ModuleHandle,
    pub id: InstanceId,
    pub body: InstanceBody,
    pub key: InstanceKey,
    pub substitution: TypeSubstitution,
    pub closure: Option<ExprId>,
    pub protocol: Option<(Protocol, TypeId)>,
}

/// Chosen at source/planner creation, never from capture count or generated spelling.
#[derive(Debug, Clone)]
pub(super) enum InstanceClassification {
    Named(DefinitionId),
    Closure(ConcreteFunctionIdentity),
    Async(ConcreteFunctionIdentity),
    Resume(ConcreteFunctionIdentity),
    Adapter(ConcreteFunctionIdentity),
    IntrinsicAdapter,
    StaticInitializer(DefinitionId),
}

impl InstanceClassification {
    pub fn lower(&self, identity: &ConcreteFunctionIdentity) -> BodyClassification {
        match self {
            Self::Named(id) => BodyClassification::Named(*id),
            Self::Closure(owner) => BodyClassification::Closure(owner.clone()),
            Self::Async(owner) => BodyClassification::Async(owner.clone()),
            Self::Resume(owner) => BodyClassification::Resume(owner.clone()),
            Self::Adapter(owner) => BodyClassification::Adapter(owner.clone()),
            Self::IntrinsicAdapter => BodyClassification::IntrinsicAdapter(identity.clone()),
            Self::StaticInitializer(id) => BodyClassification::StaticInitializer(*id),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum InstanceBody {
    Function(FunctionId),
    Static(StaticId),
}

impl InstanceBody {
    pub fn owner(self) -> BodyOwner {
        match self {
            Self::Function(id) => BodyOwner::Function(id),
            Self::Static(id) => BodyOwner::Static(id),
        }
    }

    pub fn function(self) -> Option<FunctionId> {
        match self {
            Self::Function(id) => Some(id),
            Self::Static(_) => None,
        }
    }

    pub fn span(self, module: &AnalyzedModule) -> Span {
        match self {
            Self::Function(id) => module.lowered.source_map.function_span(id),
            Self::Static(id) => module.lowered.source_map.static_span(id),
        }
    }
}

impl Instance {
    pub(super) fn lower(
        &self,
        catalog: &AggregateCatalog,
        options: &MirLoweringOptions,
        definitions: &DefinitionTable,
        span: Span,
    ) -> Result<ConcreteFunctionIdentity, MirLoweringError> {
        let mut remaining = options.max_type_nodes;
        let arguments = self
            .key
            .arguments
            .iter()
            .enumerate()
            .filter(|(_, argument)| !matches!(argument, TypeId::Generic(parameter)
                if definitions.contains(parameter.owner, self.key.declaration).expect("checked instance owner")))
            .map(|(index, argument)| {
                let argument = instantiate(argument, None, options, &mut remaining, 0, span)?;
                if !argument.is_concrete() {
                    return Err(unresolved_type(&argument, definitions, span));
                }
                // A protocol adapter's second identity argument describes a
                // declaration application, not a runtime interface value. In
                // particular Iterable must not acquire an erased Iter output.
                if index == 1 && self.protocol.is_some() {
                    let TypeId::Trait(interface) = argument else {
                        return Err(MirLoweringError::MissingBinding("protocol adapter application"));
                    };
                    Ok(Ty::Trait(lower_nominal_type(
                        &catalog.canonical_interface_application(definitions, &interface),
                    )))
                } else {
                    Ok(lower_type(&catalog.canonical_value_type(definitions, &argument)))
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(ConcreteFunctionIdentity {
            declaration: self.key.declaration,
            arguments,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct CallableInstance {
    pub receiver: TypeId,
    pub interface: NominalType,
    pub span: Span,
}

#[derive(Clone)]
pub(super) struct InterfaceBridgeRequest {
    pub declaration: DefinitionId,
    pub parameters: Vec<GenericParam>,
    pub receiver: TypeId,
    pub interface: NominalType,
    pub span: Span,
}

pub(super) struct InstancePlanner<'a> {
    module: &'a AnalyzedModule,
    pub context: &'a DefinitionContext,
    pub bridge_identities: &'a mut BridgeIdentities,
    pub catalog: &'a AggregateCatalog,
    modules: &'a HashMap<ModuleHandle, &'a AnalyzedModule>,
    pub options: &'a MirLoweringOptions,
    pub instances: Vec<Instance>,
    pub static_slots: Vec<StaticDescriptor<InstanceId>>,
    pub layout_roots: Vec<(TypeId, Span)>,
    pub host_types: BTreeSet<DefinitionId>,
    pub interface_bridges: Vec<InterfaceBridgeRequest>,
    pub native_targets: Vec<NativeImport>,
    pub interface_instances: Vec<ConcreteFunctionIdentity>,
    keys: HashMap<InstanceKey, InstanceId>,
    // One receiving identity per authored local declaration for this planner.
    local_function_placements: HashMap<DefinitionId, DefinitionId>,
    // Shared by all generated descendants of one initializer in this planner.
    static_lexical_tokens: HashMap<(ModuleHandle, StaticId), Vec<Span>>,
    generic_count: usize,
    operation_depth: usize,
    interfaces: HashSet<(DefinitionId, Vec<TypeId>)>,
    instruction_count: usize,
    failure: Option<Diagnostic>,
}

impl<'a> InstancePlanner<'a> {
    /// Encode value types after source substitution and implementation selection.
    pub fn encode_type(&self, ty: &TypeId) -> Ty {
        lower_type(&self.catalog.canonical_value_type(&self.definitions(), ty))
    }

    /// Encode an applied declaration without turning its constraints into a value view.
    pub fn encode_nominal(&self, nominal: &NominalType) -> NominalTy {
        lower_nominal_type(
            &self
                .catalog
                .canonical_interface_application(&self.definitions(), nominal),
        )
    }

    pub fn definitions(&self) -> DefinitionTable {
        self.context.snapshot()
    }

    pub fn module_handle(&self, id: DefinitionId) -> ModuleHandle {
        self.definitions()
            .module_of(id)
            .expect("checked compilation definition")
    }

    pub fn owner_handle(&self) -> ModuleHandle {
        self.module
            .names
            .catalog
            .definitions()
            .lookup_module(self.module.lowered.source.module_identity())
            .expect("checked source module")
    }

    pub fn enqueue_resume(
        &mut self,
        parent: &Instance,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let mut key = parent.key.clone();
        key.declaration = self
            .context
            .intern_child(key.declaration, DefinitionKind::Function, "$resume", 0)
            .map_err(DefinitionMappingError::from)?;
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        self.charge_layout_instance(span)?;
        let mut resume = parent.clone();
        resume.id = InstanceId::new(self.instances.len());
        resume.key = key.clone();
        resume.resume = true;
        resume.classification = InstanceClassification::Resume(parent.lower(
            self.catalog,
            self.options,
            &self.definitions(),
            span,
        )?);
        self.keys.insert(key, resume.id);
        self.instances.push(resume);
        Ok(InstanceId::new(self.instances.len() - 1))
    }

    pub fn new(
        module: &'a AnalyzedModule,
        options: &'a MirLoweringOptions,
        inputs: &'a ModuleLoweringInputs<'a>,
        bridge_identities: &'a mut BridgeIdentities,
        state: InstanceState,
    ) -> Self {
        Self {
            bridge_identities,
            context: inputs.context,
            catalog: inputs.catalog,
            modules: &inputs.modules_by_handle,
            module,
            options,
            instances: state.instances,
            static_slots: state.static_slots,
            layout_roots: state.layout_roots,
            host_types: state.host_types,
            interface_bridges: state.interface_bridges,
            interface_instances: state.interface_instances,
            native_targets: state.native_targets,
            keys: state.keys,
            local_function_placements: state.local_function_placements,
            static_lexical_tokens: state.static_lexical_tokens,
            generic_count: state.generic_count,
            operation_depth: 0,
            interfaces: state.interfaces,
            instruction_count: state.instruction_count,
            failure: state.failure,
        }
    }

    pub fn into_state(self) -> InstanceState {
        debug_assert_eq!(self.operation_depth, 0);
        InstanceState {
            instances: self.instances,
            static_slots: self.static_slots,
            layout_roots: self.layout_roots,
            host_types: self.host_types,
            interface_bridges: self.interface_bridges,
            interface_instances: self.interface_instances,
            native_targets: self.native_targets,
            keys: self.keys,
            local_function_placements: self.local_function_placements,
            static_lexical_tokens: self.static_lexical_tokens,
            generic_count: self.generic_count,
            interfaces: self.interfaces,
            instruction_count: self.instruction_count,
            failure: self.failure,
        }
    }

    pub fn origin(&self, instance: &Instance) -> &'a AnalyzedModule {
        self.modules[&instance.origin]
    }

    pub fn aggregate_catalog(&self, declaration: &DefinitionId) -> &'a AggregateCatalog {
        self.modules
            .get(&self.module_handle(*declaration))
            .map(|module| &module.aggregates)
            .unwrap_or(self.catalog)
    }

    pub fn enqueue_protocol(
        &mut self,
        parent: &Instance,
        protocol: Protocol,
        ty: &TypeId,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.enqueue_protocol_body(
            (parent.origin, parent.body, parent.substitution.clone()),
            protocol,
            ty,
            span,
            None,
            false,
        )
    }

    pub(super) fn enqueue_selected_protocol(
        &mut self,
        protocol: Protocol,
        ty: &TypeId,
        interface: &NominalType,
        span: Span,
        shared_tuple: bool,
    ) -> Result<InstanceId, MirLoweringError> {
        let parent = self
            .module
            .lowered
            .module
            .functions
            .first()
            .map(|function| InstanceBody::Function(function.id))
            .or_else(|| {
                self.module
                    .lowered
                    .module
                    .statics
                    .first()
                    .map(|item| InstanceBody::Static(item.id))
            })
            .ok_or(MirLoweringError::MissingBinding(
                "protocol adapter source context",
            ))?;
        self.enqueue_protocol_body(
            (self.owner_handle(), parent, TypeSubstitution::default()),
            protocol,
            ty,
            span,
            (protocol.iteration()
                || matches!(
                    protocol,
                    Protocol::Fn
                        | Protocol::From
                        | Protocol::Neg
                        | Protocol::Add
                        | Protocol::Sub
                        | Protocol::Mul
                        | Protocol::Div
                        | Protocol::Rem
                ))
            .then_some(interface),
            shared_tuple,
        )
    }

    fn enqueue_protocol_body(
        &mut self,
        context: (ModuleHandle, InstanceBody, TypeSubstitution),
        protocol: Protocol,
        ty: &TypeId,
        span: Span,
        interface: Option<&NominalType>,
        shared_tuple: bool,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let (origin, body, substitution) = context;
        let arity = match ty {
            TypeId::Tuple(items)
                if (shared_tuple || !ty.is_concrete())
                    && matches!(protocol, Protocol::PartialEq | Protocol::Hash) =>
            {
                Some(items.len())
            }
            _ => None,
        };
        let root = self
            .context
            .intern_root(self.module.lowered.source.module_identity())
            .map_err(DefinitionMappingError::from)?;
        let declaration = self
            .context
            .intern_child(
                root,
                DefinitionKind::Function,
                &format!(
                    "$derived_{}{}",
                    if arity.is_some() { "tuple_" } else { "" },
                    protocol.name()
                ),
                arity.unwrap_or(0) as u32,
            )
            .map_err(DefinitionMappingError::from)?;
        let shared_arguments = arity.map(|arity| {
            (0..arity)
                .map(|position| {
                    TypeId::Generic(GenericParameterType {
                        owner: declaration,
                        position,
                        name: format!("T{position}"),
                    })
                })
                .collect::<Vec<_>>()
        });
        let receiver = shared_arguments
            .as_ref()
            .map(|arguments| TypeId::Tuple(arguments.clone()))
            .unwrap_or_else(|| ty.clone());
        // A shared adapter owns its binders even when first requested by a
        // concrete call. Its body must not depend on the requester's binders.
        let substitution = shared_arguments
            .as_ref()
            .map(|arguments| {
                arguments
                    .iter()
                    .map(|argument| {
                        let TypeId::Generic(parameter) = argument else {
                            unreachable!("canonical tuple adapter parameter");
                        };
                        (parameter.clone(), argument.clone())
                    })
                    .collect()
            })
            .unwrap_or(substitution);
        let key = InstanceKey {
            declaration,
            arguments: shared_arguments.unwrap_or_else(|| {
                [Some(ty.clone()), interface.cloned().map(TypeId::Trait)]
                    .into_iter()
                    .flatten()
                    .collect()
            }),
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if self.generic_count >= self.options.max_generic_instances {
            return Err(MirLoweringError::diagnostic(limit_diagnostic(
                "generic instances",
                self.options.max_generic_instances,
                span,
            )));
        }
        self.generic_count += 1;
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            classification: InstanceClassification::IntrinsicAdapter,
            resume: false,
            callable: None,
            origin,
            id,
            body,
            key,
            substitution,
            closure: None,
            protocol: Some((protocol, receiver)),
        });
        self.record_layout_root(ty, &Default::default(), span)?;
        Ok(id)
    }

    pub fn native_function(&self, declaration: &DefinitionId) -> Option<&TypedFunction> {
        let module = self.modules.get(&self.module_handle(*declaration))?;
        let ResolvedName::Function(id) = module.declarations.definition_target(declaration)? else {
            return None;
        };
        module.typed.functions.iter().find(|function| {
            function.id == id
                && matches!(function.implementation, FunctionImplementation::Native(_))
        })
    }

    pub fn constant(&self, declaration: &DefinitionId) -> Option<ScalarValue> {
        let module = self.modules.get(&self.module_handle(*declaration))?;
        let ResolvedName::Const(id) = module.declarations.definition_target(declaration)? else {
            return None;
        };
        module.typed.const_values.get(&id).cloned()
    }

    pub fn enqueue_declaration(
        &mut self,
        declaration: &DefinitionId,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        if let Some(ResolvedName::Function(function)) =
            self.module.declarations.definition_target(declaration)
        {
            return self.enqueue(function, arguments, span);
        }
        let (implementation, method) = self
            .catalog
            .default_method(&self.definitions(), declaration)
            .ok_or(MirLoweringError::MissingBinding(
                "default method declaration",
            ))?;
        if method.default != Some(MethodDefault::Script) {
            return Err(MirLoweringError::MissingBinding(
                "native default requires template selection",
            ));
        }
        let contract = self
            .catalog
            .trait_(&method.owner)
            .ok_or(MirLoweringError::MissingBinding("default trait contract"))?;
        let count = implementation.generic_params.len();
        let method_params = &method.generic_params[contract.generic_params.len()..];
        if arguments.len() != count + method_params.len()
            || arguments.iter().any(|ty| {
                !ty.is_concrete()
                    && !matches!(ty, TypeId::Generic(parameter) if parameter.owner == *declaration)
            })
        {
            return Err(MirLoweringError::MissingBinding("default method arguments"));
        }
        let key = InstanceKey {
            declaration: *declaration,
            arguments,
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        let impl_substitution = implementation
            .generic_params
            .iter()
            .cloned()
            .zip(key.arguments[..count].iter().cloned())
            .collect();
        let receiver = implementation.for_type.instantiate(&impl_substitution);
        let applied = implementation.trait_type.instantiate(&impl_substitution);
        let mut substitution: TypeSubstitution = contract
            .generic_params
            .iter()
            .cloned()
            .zip(applied.arguments)
            .chain(
                method_params
                    .iter()
                    .cloned()
                    .zip(key.arguments[count..].iter().cloned()),
            )
            .collect();
        substitution.insert_receiver(contract.id, receiver);
        let origin = self.modules.get(&self.module_handle(method.id)).ok_or(
            MirLoweringError::MissingBinding("default body source module"),
        )?;
        let Some(ResolvedName::Function(function)) =
            origin.declarations.definition_target(&method.id)
        else {
            return Err(MirLoweringError::MissingBinding(
                "default body source function",
            ));
        };
        let origin = self.module_handle(method.id);
        if !key.arguments.is_empty() {
            self.charge_layout_instance(span)?;
        }
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            classification: InstanceClassification::Named(*declaration),
            resume: false,
            callable: None,
            origin,
            id,
            body: InstanceBody::Function(function),
            key,
            substitution,
            closure: None,
            protocol: None,
        });
        Ok(id)
    }

    pub fn check(&self) -> Result<(), MirLoweringError> {
        self.options
            .cancel
            .check()
            .map_err(|_| MirLoweringError::Cancelled)?;
        if let Some(error) = &self.failure {
            return Err(MirLoweringError::diagnostic(error.clone()));
        }
        Ok(())
    }

    pub fn intrinsic_interface_bridge(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        span: Span,
    ) -> Result<DefinitionId, MirLoweringError> {
        self.check()?;
        if let Some(request) = self
            .interface_bridges
            .iter()
            .find(|request| &request.receiver == receiver && &request.interface == interface)
        {
            return Ok(request.declaration);
        }

        let root = self
            .context
            .intern_root(self.module.lowered.source.module_identity())
            .map_err(DefinitionMappingError::from)?;
        let id = self.bridge_identities.intern(
            self.context,
            root,
            self.module.lowered.module.impls.len(),
            BridgeKey::Intrinsic(receiver.clone(), interface.clone()),
        )?;
        self.interface_bridges.push(InterfaceBridgeRequest {
            declaration: id,
            parameters: Vec::new(),
            receiver: receiver.clone(),
            interface: interface.clone(),
            span,
        });
        self.interface_instances.push(ConcreteFunctionIdentity {
            declaration: id,
            arguments: vec![],
        });
        let protocol = Protocol::from_definition(&interface.declaration, &self.definitions())
            .ok_or(MirLoweringError::MissingBinding(
                "intrinsic interface protocol",
            ))?;
        self.enqueue_selected_protocol(protocol, receiver, interface, span, false)?;
        Ok(id)
    }

    pub fn charge_instruction(&mut self, span: Span) {
        if self.instruction_count >= self.options.max_instructions {
            self.failure.get_or_insert_with(|| {
                limit_diagnostic(
                    "generated instructions",
                    self.options.max_instructions,
                    span,
                )
            });
        } else {
            self.instruction_count += 1;
        }
    }

    pub(super) fn charge_layout_instance(&mut self, span: Span) -> Result<(), MirLoweringError> {
        self.check()?;
        if self.generic_count >= self.options.max_generic_instances {
            return Err(MirLoweringError::diagnostic(limit_diagnostic(
                "generic instances",
                self.options.max_generic_instances,
                span,
            )));
        }
        self.generic_count += 1;
        Ok(())
    }

    pub(super) fn record_interface(
        &mut self,
        declaration: &DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let canonical;
        let arguments = if arguments.iter().all(TypeId::is_concrete) {
            arguments
        } else {
            let contract = self.catalog.implementation_signature(declaration).ok_or(
                MirLoweringError::MissingBinding("shared interface template"),
            )?;
            canonical = contract
                .generic_params
                .iter()
                .cloned()
                .map(TypeId::Generic)
                .collect::<Vec<_>>();
            canonical.as_slice()
        };
        if self.interfaces.insert((*declaration, arguments.to_vec())) {
            if !arguments.is_empty() {
                self.charge_layout_instance(span)?;
            }
            self.interface_instances.push(ConcreteFunctionIdentity {
                declaration: *declaration,
                arguments: arguments.iter().map(|ty| self.encode_type(ty)).collect(),
            });
            self.require_iterator_view(declaration, arguments, span)?;
            if let Some(contract) = self.catalog.implementation_signature(declaration) {
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(arguments.iter().cloned())
                    .collect();
                let receiver = contract.for_type.instantiate(&substitution);
                let interface = contract.trait_type.instantiate(&substitution);
                let assumptions = contract
                    .bounds
                    .iter()
                    .map(|(ty, constraints)| {
                        (
                            ty.instantiate(&substitution),
                            constraints
                                .iter()
                                .map(|parent| parent.instantiate(&substitution))
                                .collect(),
                        )
                    })
                    .collect();
                self.require_parent_interfaces(&receiver, &interface, &assumptions, span)?;
            }
            if self.module_handle(*declaration) == self.owner_handle()
                && let Some(contract) = self.catalog.implementation_signature(declaration)
            {
                for method in self
                    .catalog
                    .implementation_methods(&self.definitions(), contract)
                {
                    self.enqueue_interface_method(&method, arguments, span)?;
                }
            }
        }
        Ok(())
    }

    pub fn require_parent_interfaces(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        assumptions: &GenericBounds,
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let parents = self
            .catalog
            .trait_closure(interface, receiver, &self.options.cancel)
            .map_err(|_| MirLoweringError::MissingBinding("checked inheritance closure"))?;
        for parent in parents.into_iter().skip(1) {
            let selected = self
                .catalog
                .concrete_interface_implementation(
                    &self.definitions(),
                    &parent,
                    receiver,
                    assumptions,
                    ImplementationSearchLimits {
                        max_checks: 100_000,
                        max_depth: 64,
                    },
                    &self.options.cancel,
                )
                .map_err(|_| MirLoweringError::MissingBinding("parent interface search"))?;
            let Some((declaration, arguments)) = selected else {
                // Static-only parents keep operation evidence. A compatible
                // parent additionally needs a table for dynamic upcasts.
                if self.catalog.intrinsic_implementation(
                    &self.definitions(),
                    &parent,
                    receiver,
                    assumptions,
                ) {
                    if self
                        .catalog
                        .interface_incompatibility(
                            &self.definitions(),
                            &parent,
                            &self.options.cancel,
                        )
                        .map_err(|_| {
                            MirLoweringError::MissingBinding("parent interface compatibility")
                        })?
                        .is_none()
                    {
                        self.intrinsic_interface_bridge(receiver, &parent, span)?;
                    }
                    continue;
                }
                return Err(MirLoweringError::MissingBinding(
                    "parent interface implementation",
                ));
            };
            self.record_interface(&declaration, &arguments, span)?;
            if self.module_handle(declaration) == self.owner_handle() {
                let signature = self.catalog.implementation_signature(&declaration).ok_or(
                    MirLoweringError::MissingBinding("parent interface contract"),
                )?;
                let methods = self
                    .catalog
                    .implementation_methods(&self.definitions(), signature);
                for method in methods {
                    self.enqueue_interface_method(&method, &arguments, span)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn record_layout_root(
        &mut self,
        ty: &TypeId,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let concrete = self
            .arguments(slice::from_ref(ty), substitution, span)?
            .remove(0);
        self.layout_roots.push((concrete, span));
        Ok(())
    }

    pub fn enqueue(
        &mut self,
        function: FunctionId,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.enqueue_script(self.module, function, arguments, None, span)
    }

    /// Instantiates a checked script callable from its owning source module.
    fn enqueue_script(
        &mut self,
        module: &'a AnalyzedModule,
        function: FunctionId,
        arguments: Vec<TypeId>,
        placement: Option<DefinitionId>,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let declaration = module
            .declarations
            .target(ResolvedName::Function(function))
            .ok_or(MirLoweringError::MissingBinding("function declaration"))?;
        let DeclarationId::Definition(declaration) = &declaration.id else {
            return Err(MirLoweringError::MissingBinding("function identity"));
        };
        let typed = module
            .typed
            .functions
            .iter()
            .find(|typed| typed.id == function)
            .ok_or(MirLoweringError::MissingTypedFunction(function))?;
        if typed.implementation != FunctionImplementation::Script {
            return Err(MirLoweringError::MissingBinding(
                "script body implementation",
            ));
        }
        if arguments.len() != typed.generic_params.len() {
            return Err(MirLoweringError::MissingBinding("checked type arguments"));
        }
        for argument in &arguments {
            if !argument.is_concrete()
                && !matches!(argument, TypeId::Generic(parameter) if parameter.owner == *declaration)
            {
                return Err(unresolved_type(argument, &self.definitions(), span));
            }
        }
        let key = InstanceKey {
            declaration: placement.unwrap_or(*declaration),
            arguments,
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if !typed.generic_params.is_empty() {
            if self.generic_count >= self.options.max_generic_instances {
                return Err(MirLoweringError::diagnostic(limit_diagnostic(
                    "generic instances",
                    self.options.max_generic_instances,
                    span,
                )));
            }
            self.generic_count += 1;
        }
        if self.instances.len() >= u32::MAX as usize {
            return Err(MirLoweringError::diagnostic(limit_diagnostic(
                "function instances",
                u32::MAX as usize,
                span,
            )));
        }
        let id = InstanceId::new(self.instances.len());
        let substitution = typed
            .generic_params
            .iter()
            .cloned()
            .zip(key.arguments.iter().cloned())
            .collect();
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            classification: InstanceClassification::Named(*declaration),
            resume: false,
            callable: None,
            origin: self.module_handle(*declaration),
            id,
            body: InstanceBody::Function(function),
            key,
            substitution,
            closure: None,
            protocol: None,
        });
        Ok(id)
    }

    pub fn enqueue_closure(
        &mut self,
        parent: &Instance,
        closure: ExprId,
        resume: bool,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let position = self
            .static_lexical_position(parent, span)?
            .map_or(closure.index(), |position| position as usize);
        let declaration = self
            .context
            .intern_child(
                parent.key.declaration,
                DefinitionKind::Function,
                &format!("closure_{position}"),
                0,
            )
            .map_err(DefinitionMappingError::from)?;
        let key = InstanceKey {
            declaration,
            arguments: parent.key.arguments.clone(),
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if self.instances.len() >= u32::MAX as usize {
            return Err(MirLoweringError::diagnostic(limit_diagnostic(
                "function instances",
                u32::MAX as usize,
                span,
            )));
        }
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        let owner = parent.lower(self.catalog, self.options, &self.definitions(), span)?;
        let classification = if resume
            || matches!(
                self.origin(parent).lowered.module.expr(closure).kind,
                ExprKind::Closure { is_async: true, .. }
            ) {
            InstanceClassification::Async(owner)
        } else {
            InstanceClassification::Closure(owner)
        };
        self.instances.push(Instance {
            classification,
            resume,
            callable: None,
            origin: parent.origin,
            id,
            body: parent.body,
            key,
            substitution: parent.substitution.clone(),
            closure: Some(closure),
            protocol: None,
        });
        Ok(id)
    }

    pub fn arguments(
        &self,
        arguments: &[TypeId],
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<Vec<TypeId>, MirLoweringError> {
        let mut remaining = self.options.max_type_nodes;
        arguments
            .iter()
            .map(|ty| {
                instantiate(
                    ty,
                    Some(substitution),
                    self.options,
                    &mut remaining,
                    0,
                    span,
                )
                .map(|ty| self.catalog.normalize_type(&self.definitions(), &ty))
            })
            .collect()
    }

    pub fn value_type(
        &self,
        ty: &TypeId,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<ValueType, MirLoweringError> {
        self.check()?;
        let mut remaining = self.options.max_type_nodes;
        let ty = instantiate(
            ty,
            Some(substitution),
            self.options,
            &mut remaining,
            0,
            span,
        )?;
        let ty = self.catalog.normalize_type(&self.definitions(), &ty);
        if !ty.is_concrete()
            && !substitution
                .values()
                .any(|value| matches!(value, TypeId::Generic(_)))
        {
            return Err(unresolved_type(&ty, &self.definitions(), span));
        }
        Ok(semantic_representation(&lower_type(&ty)))
    }
}

fn limit_diagnostic(resource: &'static str, limit: usize, span: Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit }).with_span(span)
}

fn unresolved_type(ty: &TypeId, definitions: &DefinitionTable, span: Span) -> MirLoweringError {
    MirLoweringError::diagnostic(
        Diagnostic::error(DiagnosticKind::UnresolvedConcreteType {
            type_name: ty.display_name(definitions),
        })
        .with_span(span),
    )
}

// Count while cloning, including replacement trees. Growing recursive instances
// cannot allocate an unbounded type before discovering that the limit was crossed.
fn instantiate(
    ty: &TypeId,
    substitution: Option<&TypeSubstitution>,
    options: &MirLoweringOptions,
    remaining: &mut usize,
    depth: usize,
    span: Span,
) -> Result<TypeId, MirLoweringError> {
    options
        .cancel
        .check()
        .map_err(|_| MirLoweringError::Cancelled)?;
    if depth > options.max_type_depth {
        return Err(MirLoweringError::diagnostic(limit_diagnostic(
            "instantiated type depth",
            options.max_type_depth,
            span,
        )));
    }
    if let TypeId::Generic(parameter) = ty
        && let Some(replacement) = substitution.and_then(|substitution| substitution.get(parameter))
    {
        return instantiate(replacement, None, options, remaining, depth, span);
    }
    if let TypeId::SelfType(owner) = ty
        && let Some(replacement) =
            substitution.and_then(|substitution| substitution.receiver(owner))
    {
        return instantiate(replacement, None, options, remaining, depth, span);
    }
    if *remaining == 0 {
        return Err(MirLoweringError::diagnostic(limit_diagnostic(
            "instantiated type nodes",
            options.max_type_nodes,
            span,
        )));
    }
    *remaining -= 1;
    let mut child = |ty| instantiate(ty, substitution, options, remaining, depth + 1, span);
    Ok(match ty {
        TypeId::Tuple(elements) => {
            TypeId::Tuple(elements.iter().map(&mut child).collect::<Result<_, _>>()?)
        }
        TypeId::Function { params, result } => TypeId::Function {
            params: params.iter().map(&mut child).collect::<Result<_, _>>()?,
            result: Box::new(child(result)?),
        },
        TypeId::Range(element, kind) => TypeId::Range(Box::new(child(element)?), *kind),
        TypeId::Iter(element) => TypeId::Iter(Box::new(child(element)?)),
        TypeId::Array(element) => TypeId::Array(Box::new(child(element)?)),
        TypeId::Reference { target, access } => TypeId::Reference {
            target: Box::new(child(target)?),
            access: *access,
        },
        TypeId::Set(element, access) => TypeId::Set(Box::new(child(element)?), *access),
        TypeId::Map { key, value, access } => TypeId::Map {
            key: Box::new(child(key)?),
            value: Box::new(child(value)?),
            access: *access,
        },

        TypeId::NativeObject(nominal)
        | TypeId::Struct(nominal)
        | TypeId::Enum(nominal)
        | TypeId::Trait(nominal) => {
            let instance = NominalType {
                associated_types: nominal
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Ok((*id, child(ty)?)))
                    .collect::<Result<_, MirLoweringError>>()?,
                declaration: nominal.declaration,
                arguments: nominal
                    .arguments
                    .iter()
                    .map(&mut child)
                    .collect::<Result<_, _>>()?,
            };
            match ty {
                TypeId::NativeObject(_) => TypeId::NativeObject(instance),
                TypeId::Struct(_) => TypeId::Struct(instance),
                TypeId::Enum(_) => TypeId::Enum(instance),
                _ => TypeId::Trait(instance),
            }
        }
        TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => TypeId::Projection {
            arguments: arguments.iter().map(&mut child).collect::<Result<_, _>>()?,
            receiver: Box::new(child(receiver)?),
            interface: Box::new(NominalType {
                declaration: interface.declaration,
                arguments: interface
                    .arguments
                    .iter()
                    .map(&mut child)
                    .collect::<Result<_, _>>()?,
                associated_types: interface
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Ok((*id, child(ty)?)))
                    .collect::<Result<_, MirLoweringError>>()?,
            }),
            member: *member,
        },
        _ => ty.clone(),
    })
}
