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
    pub id: InstanceId,
    pub function: hir::FunctionId,
    pub key: FunctionInstance,
    pub substitution: TypeSubstitution,
}

pub(super) struct InstancePlanner<'a> {
    module: &'a AnalyzedModule,
    pub options: &'a IrLoweringOptions,
    pub instances: Vec<Instance>,
    keys: HashMap<FunctionInstance, InstanceId>,
    generic_count: usize,
    instruction_count: usize,
    failure: Option<Diagnostic>,
}

impl<'a> InstancePlanner<'a> {
    pub fn new(module: &'a AnalyzedModule, options: &'a IrLoweringOptions) -> Self {
        Self {
            module,
            options,
            instances: Vec::new(),
            keys: HashMap::new(),
            generic_count: 0,
            instruction_count: 0,
            failure: None,
        }
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
            id,
            function,
            key,
            substitution,
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
        _ => ty.clone(),
    })
}
