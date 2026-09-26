use std::collections::HashMap;

use kagari_common::{Diagnostic, DiagnosticKind, Span, cancellation::CancellationToken};
use kagari_hir::{
    AnalyzedModule,
    declarations::DeclarationId,
    hir,
    resolver::ResolvedName,
    types::{TypeId, TypeSubstitution},
};

use super::IrLoweringError;
use crate::module::{function::FunctionInstance, ids::InstanceId, types::ValueType};

#[derive(Debug, Clone)]
pub struct IrLoweringOptions {
    pub max_generic_instances: usize,
    pub max_type_nodes: usize,
    pub max_type_depth: usize,
    pub max_instructions: usize,
    pub cancel: CancellationToken,
}

impl Default for IrLoweringOptions {
    fn default() -> Self {
        Self {
            max_generic_instances: 1024,
            max_type_nodes: 8192,
            max_type_depth: 64,
            max_instructions: 1_000_000,
            cancel: CancellationToken::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Instance {
    pub origin: kagari_common::identity::ModuleIdentity,
    pub id: InstanceId,
    pub function: hir::FunctionId,
    pub key: FunctionInstance,
    pub substitution: TypeSubstitution,
    pub closure: Option<hir::ExprId>,
}

pub(super) struct InstancePlanner<'a> {
    module: &'a AnalyzedModule,
    pub catalog: &'a kagari_hir::aggregates::AggregateCatalog,
    modules: HashMap<kagari_common::identity::ModuleIdentity, &'a AnalyzedModule>,
    pub options: &'a IrLoweringOptions,
    pub instances: Vec<Instance>,
    pub layout_roots: Vec<(TypeId, Span)>,
    pub host_types: std::collections::BTreeSet<kagari_common::identity::DefinitionId>,
    pub host_interfaces: Vec<(
        kagari_common::identity::DefinitionId,
        TypeId,
        kagari_hir::types::NominalType,
        Span,
    )>,
    pub interface_instances: Vec<FunctionInstance>,
    keys: HashMap<FunctionInstance, InstanceId>,
    generic_count: usize,
    interfaces: std::collections::HashSet<(kagari_common::identity::DefinitionId, Vec<TypeId>)>,
    instruction_count: usize,
    failure: Option<Diagnostic>,
}

impl<'a> InstancePlanner<'a> {
    pub fn new(
        module: &'a AnalyzedModule,
        options: &'a IrLoweringOptions,
        modules: &'a [kagari_hir::CheckedAnalysis],
    ) -> Self {
        Self {
            catalog: &module.aggregates,
            modules: modules
                .iter()
                .map(|module| (module.lowered.source.module_identity().clone(), &**module))
                .collect(),
            module,
            options,
            instances: Vec::new(),
            layout_roots: Vec::new(),
            host_types: Default::default(),
            host_interfaces: Vec::new(),
            interface_instances: Vec::new(),
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
    pub fn owner(&self) -> &'a AnalyzedModule {
        self.module
    }
    pub fn constant(
        &self,
        declaration: &kagari_common::identity::DefinitionId,
    ) -> Option<kagari_hir::typeck::ScalarValue> {
        let module = self.modules.get(&declaration.module)?;
        let ResolvedName::Const(id) = module.declarations.definition_target(declaration)? else {
            return None;
        };
        module.typed.const_values.get(&id).cloned()
    }

    pub fn enqueue_declaration(
        &mut self,
        declaration: &kagari_common::identity::DefinitionId,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, IrLoweringError> {
        if let Some(ResolvedName::Function(function)) =
            self.module.declarations.definition_target(declaration)
        {
            return self.enqueue(function, arguments, span);
        }
        let (implementation, method) =
            self.catalog
                .default_method(declaration)
                .ok_or(IrLoweringError::MissingBinding(
                    "default method declaration",
                ))?;
        let contract = self
            .catalog
            .trait_(&method.owner)
            .ok_or(IrLoweringError::MissingBinding("default trait contract"))?;
        let count = implementation.generic_params.len();
        let method_params = &method.generic_params[contract.generic_params.len()..];
        if arguments.len() != count + method_params.len()
            || arguments.iter().any(|ty| !ty.is_concrete())
        {
            return Err(IrLoweringError::MissingBinding("default method arguments"));
        }
        let key = FunctionInstance {
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
        let origin = self
            .modules
            .get(&method.id.module)
            .ok_or(IrLoweringError::MissingBinding(
                "default body source module",
            ))?;
        let Some(ResolvedName::Function(function)) =
            origin.declarations.definition_target(&method.id)
        else {
            return Err(IrLoweringError::MissingBinding(
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
            origin,
            id,
            function,
            key,
            substitution,
            closure: None,
        });
        Ok(id)
    }

    pub fn check(&self) -> Result<(), IrLoweringError> {
        self.options
            .cancel
            .check()
            .map_err(|_| IrLoweringError::Cancelled)?;
        if let Some(error) = &self.failure {
            return Err(IrLoweringError::diagnostic(error.clone()));
        }
        Ok(())
    }

    pub fn host_interface(
        &mut self,
        receiver: &TypeId,
        interface: &kagari_hir::types::NominalType,
        span: Span,
    ) -> Result<kagari_common::identity::DefinitionId, IrLoweringError> {
        self.check()?;
        if let Some((id, ..)) = self
            .host_interfaces
            .iter()
            .find(|(_, ty, applied, _)| ty == receiver && applied == interface)
        {
            return Ok(id.clone());
        }
        use kagari_common::identity::{DefinitionId, DefinitionKind, DefinitionPathSegment};
        let base = self.module.lowered.module.impls.len();
        let occurrence = u32::try_from(base + self.host_interfaces.len())
            .map_err(|_| IrLoweringError::MissingBinding("host interface identity limit"))?;
        let id = DefinitionId {
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

    pub(super) fn charge_layout_instance(&mut self, span: Span) -> Result<(), IrLoweringError> {
        self.check()?;
        if self.generic_count >= self.options.max_generic_instances {
            return Err(IrLoweringError::diagnostic(limit_diagnostic(
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
        declaration: &kagari_common::identity::DefinitionId,
        arguments: &[TypeId],
        span: Span,
    ) -> Result<(), IrLoweringError> {
        if self
            .interfaces
            .insert((declaration.clone(), arguments.to_vec()))
        {
            if !arguments.is_empty() {
                self.charge_layout_instance(span)?;
            }
            self.interface_instances.push(FunctionInstance {
                declaration: declaration.clone(),
                arguments: arguments.to_vec(),
            });
        }
        Ok(())
    }

    pub fn require_parent_interfaces(
        &mut self,
        receiver: &TypeId,
        interface: &kagari_hir::types::NominalType,
        span: Span,
    ) -> Result<(), IrLoweringError> {
        let parents = self
            .module
            .aggregates
            .trait_closure(interface, receiver, &self.options.cancel)
            .map_err(|_| IrLoweringError::MissingBinding("checked inheritance closure"))?;
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
                .module
                .aggregates
                .concrete_interface_implementation(
                    &parent,
                    receiver,
                    &Default::default(),
                    100_000,
                    64,
                    &self.options.cancel,
                )
                .map_err(|_| IrLoweringError::MissingBinding("parent interface search"))?
                .ok_or(IrLoweringError::MissingBinding(
                    "parent interface implementation",
                ))?;
            self.record_interface(&declaration, &arguments, span)?;
            if declaration.module == *self.module.lowered.source.module_identity() {
                let signature = self
                    .module
                    .aggregates
                    .implementation_signature(&declaration)
                    .ok_or(IrLoweringError::MissingBinding("parent interface contract"))?;
                let methods = self.catalog.implementation_methods(signature);
                for method in methods {
                    self.enqueue_declaration(&method, arguments.clone(), span)?;
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
    ) -> Result<(), IrLoweringError> {
        let concrete = self
            .arguments(std::slice::from_ref(ty), substitution, span)?
            .remove(0);
        self.layout_roots.push((concrete, span));
        Ok(())
    }

    pub fn enqueue(
        &mut self,
        function: hir::FunctionId,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, IrLoweringError> {
        self.check()?;
        let declaration = self
            .module
            .declarations
            .target(ResolvedName::Function(function))
            .ok_or(IrLoweringError::MissingBinding("function declaration"))?;
        let DeclarationId::Definition(declaration) = &declaration.id else {
            return Err(IrLoweringError::MissingBinding("function identity"));
        };
        let typed = self
            .module
            .typed
            .functions
            .iter()
            .find(|typed| typed.id == function)
            .ok_or(IrLoweringError::MissingTypedFunction(function))?;
        if arguments.len() != typed.generic_params.len() {
            return Err(IrLoweringError::MissingBinding("checked type arguments"));
        }
        for argument in &arguments {
            if !argument.is_concrete() {
                return Err(unresolved_type(argument, span));
            }
        }
        let key = FunctionInstance {
            declaration: declaration.clone(),
            arguments,
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if !typed.generic_params.is_empty() {
            if self.generic_count >= self.options.max_generic_instances {
                return Err(IrLoweringError::diagnostic(limit_diagnostic(
                    "generic instances",
                    self.options.max_generic_instances,
                    span,
                )));
            }
            self.generic_count += 1;
        }
        if self.instances.len() >= u32::MAX as usize {
            return Err(IrLoweringError::diagnostic(limit_diagnostic(
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
            origin: self.module.lowered.source.module_identity().clone(),
            id,
            function,
            key,
            substitution,
            closure: None,
        });
        Ok(id)
    }

    pub fn enqueue_closure(
        &mut self,
        parent: &Instance,
        closure: hir::ExprId,
        span: Span,
    ) -> Result<InstanceId, IrLoweringError> {
        self.check()?;
        let mut declaration = parent.key.declaration.clone();
        declaration
            .path
            .push(kagari_common::identity::DefinitionPathSegment {
                kind: kagari_common::identity::DefinitionKind::Function,
                name: format!("closure_{}", closure.index()),
                occurrence: 0,
            });
        let key = FunctionInstance {
            declaration,
            arguments: parent.key.arguments.clone(),
        };
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        if self.instances.len() >= u32::MAX as usize {
            return Err(IrLoweringError::diagnostic(limit_diagnostic(
                "function instances",
                u32::MAX as usize,
                span,
            )));
        }
        let id = InstanceId::new(self.instances.len());
        self.keys.insert(key.clone(), id);
        self.instances.push(Instance {
            origin: parent.origin.clone(),
            id,
            function: parent.function,
            key,
            substitution: parent.substitution.clone(),
            closure: Some(closure),
        });
        Ok(id)
    }

    pub fn arguments(
        &self,
        arguments: &[TypeId],
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<Vec<TypeId>, IrLoweringError> {
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
                .map(|ty| self.module.aggregates.normalize_type(&ty))
            })
            .collect()
    }

    pub fn value_type(
        &self,
        ty: &TypeId,
        substitution: &TypeSubstitution,
        span: Span,
    ) -> Result<ValueType, IrLoweringError> {
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
        let ty = self.module.aggregates.normalize_type(&ty);
        if !ty.is_concrete() {
            return Err(unresolved_type(&ty, span));
        }
        Ok(ValueType::from_type_id(&ty))
    }
}

fn limit_diagnostic(resource: &'static str, limit: usize, span: Span) -> Diagnostic {
    Diagnostic::error(DiagnosticKind::CompileLimitExceeded { resource, limit }).with_span(span)
}

fn unresolved_type(ty: &TypeId, span: Span) -> IrLoweringError {
    IrLoweringError::diagnostic(
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
    options: &IrLoweringOptions,
    remaining: &mut usize,
    depth: usize,
    span: Span,
) -> Result<TypeId, IrLoweringError> {
    options
        .cancel
        .check()
        .map_err(|_| IrLoweringError::Cancelled)?;
    if depth > options.max_type_depth {
        return Err(IrLoweringError::diagnostic(limit_diagnostic(
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
        return Err(IrLoweringError::diagnostic(limit_diagnostic(
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
        TypeId::Array(element) => TypeId::Array(Box::new(child(element)?)),
        TypeId::Set(element) => TypeId::Set(Box::new(child(element)?)),
        TypeId::Map { key, value } => TypeId::Map {
            key: Box::new(child(key)?),
            value: Box::new(child(value)?),
        },
        TypeId::StandardEnum { kind, args } => TypeId::StandardEnum {
            kind: *kind,
            args: args.iter().map(&mut child).collect::<Result<_, _>>()?,
        },
        TypeId::Struct(nominal) | TypeId::Enum(nominal) | TypeId::Trait(nominal) => {
            let instance = kagari_hir::types::NominalType {
                associated_types: nominal
                    .associated_types
                    .iter()
                    .map(|(id, ty)| Ok((id.clone(), child(ty)?)))
                    .collect::<Result<_, IrLoweringError>>()?,
                declaration: nominal.declaration.clone(),
                arguments: nominal
                    .arguments
                    .iter()
                    .map(&mut child)
                    .collect::<Result<_, _>>()?,
            };
            match ty {
                TypeId::Struct(_) => TypeId::Struct(instance),
                TypeId::Enum(_) => TypeId::Enum(instance),
                _ => TypeId::Trait(instance),
            }
        }
        TypeId::Projection {
            receiver,
            interface,
            member,
        } => TypeId::Projection {
            receiver: Box::new(child(receiver)?),
            interface: Box::new(kagari_hir::types::NominalType {
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
                    .collect::<Result<_, IrLoweringError>>()?,
            }),
            member: member.clone(),
        },
        _ => ty.clone(),
    })
}
