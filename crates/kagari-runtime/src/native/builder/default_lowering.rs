//! Semantic native-default lowering without adapter preparation or publication.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        scoped::DefinitionReferences,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication, Signature},
    declaration::{
        FnDecl, TraitDef, module::DeclarationError, requirement::NativeCallableRequirement,
    },
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        substitution::{
            MAX_TYPE_DEPTH, MAX_TYPE_NODES, TypeSubstitution, TypeTransformError,
            normalize_projections,
        },
    },
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Inputs to one private default template, all in the supplied definition scope.
///
/// Types remain in the authored trait/method scope. Each ordered `arguments` entry
/// lifts a distinct `SelfType`, `Parameter`, or `Projection` to a template parameter.
/// For example, List's sorted-by-key body supplies `Self, T, K, Self::Iter`;
/// Iterable zip supplies `Self, Self::Item, R, R::Item, Self::Iter, R::Iter`.
/// Callers supply traversal bounds and callable requirements, including associated
/// equalities; the lowerer does not infer library policy or implementation evidence.
pub struct DefaultTemplateSpec<'a> {
    pub method: &'a FnDecl,
    /// An already allocated function identity; its final segment supplies the name.
    pub template: DefinitionId,
    pub arguments: &'a [Ty],
    /// Additional bounds, combined with `method.bounds` before normalization.
    pub assumptions: &'a [GenericBound],
    pub requirements: &'a [NativeCallableRequirement],
    pub concrete_result: Option<&'a Ty>,
}

/// A complete semantic default ready for staged declaration publication.
///
/// The application retains authored arguments, while every template contract uses
/// the private function's binders. This value contains no prepared Rust binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoweredDefault {
    pub template: FnDecl,
    pub application: NativeDefaultApplication,
    pub requirements: Vec<NativeCallableRequirement>,
    pub concrete_result: Option<Ty>,
}

impl LoweredDefault {
    /// Return the Rust body's signature without changing the declared result view.
    pub fn binding_signature(&self) -> Signature {
        Signature {
            params: self
                .template
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect(),
            result: self
                .concrete_result
                .clone()
                .unwrap_or_else(|| self.template.return_type.clone()),
        }
    }
}

/// Collect the assumptions inherited by an ordinary trait default.
///
/// `owner` names `declaration` in the caller's definition table. Its receiver bound
/// supplies parent-trait and associated-output evidence during later checking.
/// Associated output bounds remain projections; no recursive output-equals-itself
/// equality is introduced. The caller passes these assumptions to `lower_default`,
/// which validates their scope, size and cancellation before returning a template.
pub fn trait_default_assumptions(owner: DefinitionId, declaration: &TraitDef) -> Vec<GenericBound> {
    let receiver = Ty::SelfType(owner);
    let interface = NominalTy {
        declaration: owner,
        arguments: declaration
            .generic_params
            .iter()
            .map(GenericParam::as_type)
            .collect(),
        associated_types: BTreeMap::new(),
    };
    let mut assumptions = declaration.bounds.clone();
    for output in &declaration.associated_types {
        if !output.bounds.is_empty() {
            assumptions.push(GenericBound {
                ty: Ty::Projection {
                    receiver: Box::new(receiver.clone()),
                    interface: Box::new(interface.clone()),
                    member: output.declaration,
                    arguments: vec![],
                },
                constraints: output.bounds.clone(),
            });
        }
    }
    assumptions.push(GenericBound {
        ty: receiver,
        constraints: vec![interface],
    });
    assumptions
}

/// Lift an authored default into an ordinary private native function.
///
/// All IDs must belong to `definitions`, including IDs in types eliminated by
/// substitution. Foreign/stale IDs, non-function template identities, unsupported
/// or duplicate lifts, excessive shapes and cancellation return an error without
/// mutating the inputs. Nested projection keys are normalized independently of
/// argument order. Callable requirements retain their original order and count.
///
/// This is a semantic transformation, not proof of trait obligations or Rust ABI
/// compatibility. The caller must validate the complete declaration graph and
/// check the implementation against `binding_signature()` before installation.
pub fn lower_default(
    spec: DefaultTemplateSpec<'_>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<LoweredDefault, DeclarationError> {
    validate_inputs(&spec, definitions, cancel)?;
    let segment = definitions
        .resolve(spec.template)
        .map_err(invalid)?
        .last()
        .filter(|segment| segment.kind == DefinitionKind::Function)
        .ok_or_else(|| invalid("template identity must name a function"))?;
    let parameters: Vec<_> = (0..spec.arguments.len())
        .map(|position| GenericParam {
            owner: spec.template,
            position,
        })
        .collect();
    let types: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
    let mut substitution = TypeSubstitution::default();
    let mut receivers = BTreeSet::new();
    for (argument, ty) in spec.arguments.iter().zip(&types) {
        cancel
            .check()
            .map_err(|_| transform_error(TypeTransformError::Cancelled))?;
        match argument {
            Ty::SelfType(owner) => {
                if !receivers.insert(*owner) {
                    return Err(invalid("duplicate receiver lift"));
                }
                substitution.bind_receiver(owner, ty);
            }
            Ty::Parameter { owner, position } => {
                if substitution.parameter(owner, *position).is_some() {
                    return Err(invalid("duplicate parameter lift"));
                }
                substitution.bind(owner, *position, ty);
            }
            Ty::Projection { .. } => {}
            _ => {
                return Err(invalid(
                    "default arguments must lift receivers, parameters or projections",
                ));
            }
        }
    }
    let projections = ProjectionMap::new(spec.arguments, &types, &substitution, cancel)
        .map_err(transform_error)?;
    let lowering = Lowering {
        substitution,
        projections,
        cancel,
    };
    let mut bounds = Vec::new();
    for bound in spec.assumptions.iter().chain(&spec.method.bounds) {
        bounds.push(GenericBound {
            ty: lowering.ty(&bound.ty).map_err(transform_error)?,
            constraints: bound
                .constraints
                .iter()
                .map(|interface| lowering.nominal(interface))
                .collect::<Result<_, _>>()
                .map_err(transform_error)?,
        });
    }
    let mut template = spec.method.clone();
    template.name = segment.name.to_owned();
    template.implementation = CallableImplementation::Native(spec.template);
    template.generic_params = parameters;
    template.bounds =
        GenericBound::normalize(bounds, definitions, cancel).map_err(transform_error)?;
    for parameter in &mut template.params {
        if parameter.name == "self" {
            parameter.name = "__receiver".into();
        }
        parameter.ty = lowering.ty(&parameter.ty).map_err(transform_error)?;
    }
    template.return_type = lowering
        .ty(&template.return_type)
        .map_err(transform_error)?;
    let requirements = spec
        .requirements
        .iter()
        .map(|requirement| {
            Ok(NativeCallableRequirement {
                receiver: lowering.ty(&requirement.receiver)?,
                interface: lowering.nominal(&requirement.interface)?,
                member: requirement.member,
                arguments: requirement
                    .arguments
                    .iter()
                    .map(|ty| lowering.ty(ty))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<_, TypeTransformError>>()
        .map_err(transform_error)?;
    let concrete_result = spec
        .concrete_result
        .map(|ty| lowering.ty(ty))
        .transpose()
        .map_err(transform_error)?;
    cancel
        .check()
        .map_err(|_| transform_error(TypeTransformError::Cancelled))?;
    Ok(LoweredDefault {
        template,
        application: NativeDefaultApplication {
            declaration: spec.template,
            arguments: spec.arguments.to_vec(),
        },
        requirements,
        concrete_result,
    })
}

fn validate_inputs(
    spec: &DefaultTemplateSpec<'_>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    cancel
        .check()
        .map_err(|_| transform_error(TypeTransformError::Cancelled))?;
    for count in [
        spec.arguments.len(),
        spec.assumptions
            .len()
            .saturating_add(spec.method.bounds.len()),
        spec.requirements.len(),
        spec.method.params.len(),
        spec.method.generic_params.len(),
    ] {
        check_count(count)?;
    }
    check_scope(spec.method, definitions, cancel)?;
    for argument in spec.arguments {
        check_scope(argument, definitions, cancel)?;
    }
    for bound in spec.assumptions.iter().chain(&spec.method.bounds) {
        check_count(bound.constraints.len())?;
        check_scope(bound, definitions, cancel)?;
    }
    for requirement in spec.requirements {
        check_count(requirement.arguments.len())?;
        check_scope(requirement, definitions, cancel)?;
    }
    if let Some(ty) = spec.concrete_result {
        check_scope(ty, definitions, cancel)?;
    }
    Ok(())
}

fn check_count(count: usize) -> Result<(), DeclarationError> {
    if count > MAX_TYPE_NODES {
        Err(transform_error(TypeTransformError::LimitExceeded))
    } else {
        Ok(())
    }
}

fn check_scope(
    value: &impl DefinitionReferences,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), DeclarationError> {
    value
        .visit_definition_ids(
            &mut |id| definitions.resolve(*id).map(|_| ()).map_err(Into::into),
            cancel,
        )
        .map_err(invalid)
}

struct Lowering<'a> {
    substitution: TypeSubstitution<'a>,
    projections: ProjectionMap,
    cancel: &'a CancellationToken,
}

impl Lowering<'_> {
    fn ty(&self, ty: &Ty) -> Result<Ty, TypeTransformError> {
        let applied = self.substitution.apply(ty, self.cancel)?;
        self.projections.normalize(&applied, self.cancel)
    }

    fn nominal(&self, interface: &NominalTy) -> Result<NominalTy, TypeTransformError> {
        let Ty::Trait(interface) = self.ty(&Ty::Trait(interface.clone()))? else {
            return Err(TypeTransformError::InvalidContract);
        };
        Ok(interface)
    }
}

// Refinements remain in the authored contracts, not the projection's identity.
// In particular, Iterable<Item=T>::Iter and Iterable::Iter name the same output.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectionKey {
    receiver: Ty,
    interface: DefinitionId,
    interface_arguments: Vec<Ty>,
    member: DefinitionId,
    member_arguments: Vec<Ty>,
}

impl ProjectionKey {
    fn new(interface: &NominalTy, receiver: &Ty, member: DefinitionId, arguments: &[Ty]) -> Self {
        Self {
            receiver: receiver.clone(),
            interface: interface.declaration,
            interface_arguments: interface.arguments.clone(),
            member,
            member_arguments: arguments.to_vec(),
        }
    }

    fn normalize(
        &self,
        mappings: &ProjectionMap,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        Ok(Self {
            receiver: mappings.normalize(&self.receiver, cancel)?,
            interface: self.interface,
            interface_arguments: self
                .interface_arguments
                .iter()
                .map(|ty| mappings.normalize(ty, cancel))
                .collect::<Result<_, _>>()?,
            member: self.member,
            member_arguments: self
                .member_arguments
                .iter()
                .map(|ty| mappings.normalize(ty, cancel))
                .collect::<Result<_, _>>()?,
        })
    }
}

#[derive(Default, PartialEq, Eq)]
struct ProjectionMap(BTreeMap<ProjectionKey, Ty>);

impl ProjectionMap {
    fn new(
        arguments: &[Ty],
        types: &[Ty],
        substitution: &TypeSubstitution<'_>,
        cancel: &CancellationToken,
    ) -> Result<Self, TypeTransformError> {
        let mut mappings = Self::default();
        for (argument, ty) in arguments.iter().zip(types) {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if let Ty::Projection {
                receiver,
                interface,
                member,
                arguments,
            } = substitution.apply(argument, cancel)?
            {
                mappings.insert(
                    ProjectionKey::new(&interface, &receiver, member, &arguments),
                    ty.clone(),
                )?;
            }
        }
        // A projection's receiver or arguments can themselves be lifted outputs.
        // Normalize keys to the same bottom-up form used by normalize_projections;
        // bounded fixed-point iteration avoids depending on authoring order.
        for _ in 0..=MAX_TYPE_DEPTH {
            let mut normalized = Self::default();
            for (key, ty) in &mappings.0 {
                normalized.insert(key.normalize(&mappings, cancel)?, ty.clone())?;
            }
            if normalized == mappings {
                return Ok(mappings);
            }
            mappings = normalized;
        }
        Err(TypeTransformError::LimitExceeded)
    }

    fn insert(&mut self, key: ProjectionKey, ty: Ty) -> Result<(), TypeTransformError> {
        if self.0.insert(key, ty).is_some() {
            return Err(TypeTransformError::InvalidContract);
        }
        Ok(())
    }

    fn normalize(&self, ty: &Ty, cancel: &CancellationToken) -> Result<Ty, TypeTransformError> {
        normalize_projections(
            ty,
            &|interface, receiver, member, arguments| {
                cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                Ok(self
                    .0
                    .get(&ProjectionKey::new(interface, receiver, *member, arguments))
                    .cloned())
            },
            cancel,
        )
    }
}

fn invalid(error: impl fmt::Display) -> DeclarationError {
    DeclarationError(format!("native default lowering: {error}"))
}

fn transform_error(error: TypeTransformError) -> DeclarationError {
    DeclarationError(format!("native default lowering: {error:?}"))
}

#[cfg(test)]
mod tests;
