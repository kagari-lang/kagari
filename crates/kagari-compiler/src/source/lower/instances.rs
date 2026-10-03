use crate::source::lower::MirLoweringError;
mod callables;
mod calls;
mod defaults;
mod native;
mod results;
mod shared;
mod views;
use kagari_hir::{
    AnalyzedModule,
    aggregates::{AggregateCatalog, traits::MethodDefault},
    declarations::DeclarationId,
    hir::ids::{ExprId, FunctionId},
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, TypedFunction, scalar::ScalarValue},
    types::{NominalType, TypeId, TypeSubstitution, abi::lower_type},
};

use kagari_common::{
    cancellation::CancellationToken,
    diagnostic::{Diagnostic, DiagnosticKind},
    identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity},
    span::Span,
};
use {
    kagari_abi::representation::ValueType,
    kagari_contract::{
        language::Protocol, native_import::NativeImport, types::ConcreteFunctionIdentity,
    },
};

use std::{
    collections::{BTreeSet, HashMap, HashSet},
    slice,
};

use kagari_mir::{ids::InstanceId, passes::PassOptions};

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
    pub declaration: DefinitionPath,
    pub arguments: Vec<TypeId>,
}

impl InstanceKey {
    pub(super) fn lower(
        &self,
        options: &MirLoweringOptions,
        span: Span,
    ) -> Result<ConcreteFunctionIdentity, MirLoweringError> {
        let mut remaining = options.max_type_nodes;
        let arguments = self
            .arguments
            .iter()
            .filter(|argument| !matches!(argument, TypeId::Generic(parameter)
                if parameter.owner.module == self.declaration.module && self.declaration.path.starts_with(&parameter.owner.path)))
            .map(|argument| {
                let argument = instantiate(argument, None, options, &mut remaining, 0, span)?;
                if !argument.is_concrete() {
                    return Err(unresolved_type(&argument, span));
                }
                Ok(lower_type(&argument))
            })
            .collect::<Result<_, _>>()?;
        Ok(ConcreteFunctionIdentity {
            declaration: self.declaration.clone(),
            arguments,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct Instance {
    pub callable: Option<CallableInstance>,
    pub origin: ModuleIdentity,
    pub id: InstanceId,
    pub function: FunctionId,
    pub key: InstanceKey,
    pub substitution: TypeSubstitution,
    pub closure: Option<ExprId>,
    pub protocol: Option<(Protocol, TypeId)>,
}

#[derive(Debug, Clone)]
pub(super) struct CallableInstance {
    pub receiver: TypeId,
    pub interface: NominalType,
    pub span: Span,
}

pub(super) struct InstancePlanner<'a> {
    module: &'a AnalyzedModule,
    pub catalog: &'a AggregateCatalog,
    modules: HashMap<ModuleIdentity, &'a AnalyzedModule>,
    pub options: &'a MirLoweringOptions,
    pub instances: Vec<Instance>,
    pub layout_roots: Vec<(TypeId, Span)>,
    pub host_types: BTreeSet<DefinitionPath>,
    pub host_interfaces: Vec<(DefinitionPath, TypeId, NominalType, Span)>,
    pub native_targets: Vec<NativeImport>,
    pub interface_instances: Vec<ConcreteFunctionIdentity>,
    keys: HashMap<InstanceKey, InstanceId>,
    generic_count: usize,
    interfaces: HashSet<(DefinitionPath, Vec<TypeId>)>,
    instruction_count: usize,
    failure: Option<Diagnostic>,
}

impl<'a> InstancePlanner<'a> {
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
        let mut declaration = parent.key.declaration.clone();
        declaration.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: "$callable".into(),
            occurrence: body.span.start as u32,
        });
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
            callable: Some(body),
            origin: parent.origin.clone(),
            id,
            function: parent.function,
            key,
            substitution: parent.substitution.clone(),
            closure: None,
            protocol: None,
        });
        Ok(id)
    }

    pub fn new(
        module: &'a AnalyzedModule,
        options: &'a MirLoweringOptions,
        modules: &'a [AnalyzedModule],
        catalog: &'a AggregateCatalog,
    ) -> Self {
        Self {
            catalog,
            modules: modules
                .iter()
                .map(|module| (module.lowered.source.module_identity().clone(), module))
                .collect(),
            module,
            options,
            instances: Vec::new(),
            layout_roots: Vec::new(),
            host_types: Default::default(),
            host_interfaces: Vec::new(),
            interface_instances: Vec::new(),
            native_targets: Vec::new(),
            keys: HashMap::new(),
            generic_count: 0,
            interfaces: Default::default(),
            instruction_count: 0,
            failure: None,
        }
    }

    pub fn origin(&self, instance: &Instance) -> &'a AnalyzedModule {
        self.modules[&instance.origin]
    }

    pub fn aggregate_catalog(&self, declaration: &DefinitionPath) -> &'a AggregateCatalog {
        self.modules
            .get(&declaration.module)
            .map(|module| &module.aggregates)
            .unwrap_or(self.catalog)
    }

    pub fn owner(&self) -> &'a AnalyzedModule {
        self.module
    }

    pub fn enqueue_protocol(
        &mut self,
        parent: &Instance,
        protocol: Protocol,
        ty: &TypeId,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.enqueue_protocol_body(
            (
                parent.origin.clone(),
                parent.function,
                parent.substitution.clone(),
            ),
            protocol,
            ty,
            span,
            None,
        )
    }

    pub(super) fn enqueue_selected_protocol(
        &mut self,
        protocol: Protocol,
        ty: &TypeId,
        interface: &NominalType,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        let parent = self.module.lowered.module.functions.first().ok_or(
            MirLoweringError::MissingBinding("protocol adapter source context"),
        )?;
        self.enqueue_protocol_body(
            (
                self.module.lowered.source.module_identity().clone(),
                parent.id,
                TypeSubstitution::default(),
            ),
            protocol,
            ty,
            span,
            (protocol.iteration() || protocol == Protocol::Fn).then_some(interface),
        )
    }

    fn enqueue_protocol_body(
        &mut self,
        context: (ModuleIdentity, FunctionId, TypeSubstitution),
        protocol: Protocol,
        ty: &TypeId,
        span: Span,
        interface: Option<&NominalType>,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let (origin, function, substitution) = context;
        let declaration = DefinitionPath {
            module: self.module.lowered.source.module_identity().clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Function,
                name: format!("$derived_{}", protocol.name()),
                occurrence: 0,
            }],
        };
        let key = InstanceKey {
            declaration,
            arguments: [Some(ty.clone()), interface.cloned().map(TypeId::Trait)]
                .into_iter()
                .flatten()
                .collect(),
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
            callable: None,
            origin,
            id,
            function,
            key,
            substitution,
            closure: None,
            protocol: Some((protocol, ty.clone())),
        });
        self.record_layout_root(ty, &Default::default(), span)?;
        Ok(id)
    }

    pub fn native_function(&self, declaration: &DefinitionPath) -> Option<&TypedFunction> {
        let module = self.modules.get(&declaration.module)?;
        let ResolvedName::Function(id) = module.declarations.definition_target(declaration)? else {
            return None;
        };
        module.typed.functions.iter().find(|function| {
            function.id == id
                && matches!(function.implementation, FunctionImplementation::Native(_))
        })
    }

    pub fn constant(&self, declaration: &DefinitionPath) -> Option<ScalarValue> {
        let module = self.modules.get(&declaration.module)?;
        let ResolvedName::Const(id) = module.declarations.definition_target(declaration)? else {
            return None;
        };
        module.typed.const_values.get(&id).cloned()
    }

    pub fn enqueue_declaration(
        &mut self,
        declaration: &DefinitionPath,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        if let Some(ResolvedName::Function(function)) =
            self.module.declarations.definition_target(declaration)
        {
            return self.enqueue(function, arguments, span);
        }
        let (implementation, method) =
            self.catalog
                .default_method(declaration)
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
            declaration: declaration.clone(),
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
        substitution.insert_receiver(contract.id.clone(), receiver);
        let origin =
            self.modules
                .get(&method.id.module)
                .ok_or(MirLoweringError::MissingBinding(
                    "default body source module",
                ))?;
        let Some(ResolvedName::Function(function)) =
            origin.declarations.definition_target(&method.id)
        else {
            return Err(MirLoweringError::MissingBinding(
                "default body source function",
            ));
        };
        let origin = method.id.module.clone();
        if !key.arguments.is_empty() {
            self.charge_layout_instance(span)?;
        }
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            callable: None,
            origin,
            id,
            function,
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

    pub fn host_interface(
        &mut self,
        receiver: &TypeId,
        interface: &NominalType,
        span: Span,
    ) -> Result<DefinitionPath, MirLoweringError> {
        self.check()?;
        if let Some((id, ..)) = self
            .host_interfaces
            .iter()
            .find(|(_, ty, applied, _)| ty == receiver && applied == interface)
        {
            return Ok(id.clone());
        }

        let base = self.module.lowered.module.impls.len();
        let occurrence = u32::try_from(base + self.host_interfaces.len())
            .map_err(|_| MirLoweringError::MissingBinding("host interface identity limit"))?;
        let id = DefinitionPath {
            module: self.module.lowered.source.module_identity().clone(),
            path: vec![DefinitionPathSegment {
                kind: DefinitionKind::Impl,
                name: String::new(),
                occurrence,
            }],
        };
        self.host_interfaces
            .push((id.clone(), receiver.clone(), interface.clone(), span));
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
        declaration: &DefinitionPath,
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
        if self
            .interfaces
            .insert((declaration.clone(), arguments.to_vec()))
        {
            if !arguments.is_empty() {
                self.charge_layout_instance(span)?;
            }
            self.interface_instances.push(ConcreteFunctionIdentity {
                declaration: declaration.clone(),
                arguments: arguments.iter().map(lower_type).collect(),
            });
            self.require_iterator_view(declaration, arguments, span)?;
            if declaration.module == *self.module.lowered.source.module_identity()
                && let Some(contract) = self.catalog.implementation_signature(declaration)
            {
                for method in self.catalog.implementation_methods(contract) {
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
        span: Span,
    ) -> Result<(), MirLoweringError> {
        let parents = self
            .catalog
            .trait_closure(interface, receiver, &self.options.cancel)
            .map_err(|_| MirLoweringError::MissingBinding("checked inheritance closure"))?;
        for parent in parents.into_iter().skip(1) {
            if self
                .module
                .names
                .hosts
                .interface_implementation(&parent, receiver)
                .is_some()
            {
                self.host_interface(receiver, &parent, span)?;
                continue;
            }
            let (declaration, arguments) = self
                .catalog
                .concrete_interface_implementation(
                    &parent,
                    receiver,
                    &Default::default(),
                    100_000,
                    64,
                    &self.options.cancel,
                )
                .map_err(|_| MirLoweringError::MissingBinding("parent interface search"))?
                .ok_or(MirLoweringError::MissingBinding(
                    "parent interface implementation",
                ))?;
            self.record_interface(&declaration, &arguments, span)?;
            if declaration.module == *self.module.lowered.source.module_identity() {
                let signature = self.catalog.implementation_signature(&declaration).ok_or(
                    MirLoweringError::MissingBinding("parent interface contract"),
                )?;
                let methods = self.catalog.implementation_methods(signature);
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
        self.check()?;
        let declaration = self
            .module
            .declarations
            .target(ResolvedName::Function(function))
            .ok_or(MirLoweringError::MissingBinding("function declaration"))?;
        let DeclarationId::Definition(declaration) = &declaration.id else {
            return Err(MirLoweringError::MissingBinding("function identity"));
        };
        let typed = self
            .module
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
                return Err(unresolved_type(argument, span));
            }
        }
        let key = InstanceKey {
            declaration: declaration.clone(),
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
            callable: None,
            origin: self.module.lowered.source.module_identity().clone(),
            id,
            function,
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
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        self.check()?;
        let mut declaration = parent.key.declaration.clone();
        declaration.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Function,
            name: format!("closure_{}", closure.index()),
            occurrence: 0,
        });
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
        self.instances.push(Instance {
            callable: None,
            origin: parent.origin.clone(),
            id,
            function: parent.function,
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
                .map(|ty| self.catalog.normalize_type(&ty))
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
        let ty = self.catalog.normalize_type(&ty);
        if !ty.is_concrete()
            && !substitution
                .values()
                .any(|value| matches!(value, TypeId::Generic(_)))
        {
            return Err(unresolved_type(&ty, span));
        }
        Ok(lower_type(&ty).representation())
    }
}

fn limit_diagnostic(resource: &'static str, limit: usize, span: Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit }).with_span(span)
}

fn unresolved_type(ty: &TypeId, span: Span) -> MirLoweringError {
    MirLoweringError::diagnostic(
        Diagnostic::error(DiagnosticKind::UnresolvedConcreteType {
            type_name: ty.display_name(),
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
        TypeId::Array(element, access) => TypeId::Array(Box::new(child(element)?), *access),
        TypeId::Set(element, access) => TypeId::Set(Box::new(child(element)?), *access),
        TypeId::Map { key, value, access } => TypeId::Map {
            key: Box::new(child(key)?),
            value: Box::new(child(value)?),
            access: *access,
        },
        TypeId::StandardEnum { kind, args } => TypeId::StandardEnum {
            kind: *kind,
            args: args.iter().map(&mut child).collect::<Result<_, _>>()?,
        },
        TypeId::NativeObject(nominal)
        | TypeId::Struct(nominal)
        | TypeId::Enum(nominal)
        | TypeId::Trait(nominal) => {
            let instance = NominalType {
                associated_types: nominal
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Ok((id.clone(), child(ty)?)))
                    .collect::<Result<_, MirLoweringError>>()?,
                declaration: nominal.declaration.clone(),
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
                declaration: interface.declaration.clone(),
                arguments: interface
                    .arguments
                    .iter()
                    .map(&mut child)
                    .collect::<Result<_, _>>()?,
                associated_types: interface
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Ok((id.clone(), child(ty)?)))
                    .collect::<Result<_, MirLoweringError>>()?,
            }),
            member: member.clone(),
        },
        _ => ty.clone(),
    })
}
