//! Applied interface method signatures retain separate receiver and method binders.
use crate::{
    callable::witness::{OperationWitness, required_operations},
    native_import::callables::normalize_requirement,
    representation::semantic_representation,
    types::{PublicItem, TraitContract, proofs::ProofCatalog, trait_contract},
};
use kagari_abi::representation::ValueType;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    declaration::{TraitDef, requirement::NativeCallableRequirement, verify::types_in_scope},
    ty::{
        Constraint, GenericBound, GenericParam, NominalTy, Ty,
        inheritance::{interface_views, trait_closure},
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCallContract {
    /// Associated projections resolved by the compiler for this application.
    /// Linked verification checks every fact against the supplying declarations.
    pub normalizations: Vec<TypeNormalization>,
    /// None dispatches a boxed interface; Some invokes the caller's checked
    /// constraint operation on an unboxed value of this type.
    pub receiver: Option<Ty>,
    pub operations: Vec<OperationWitness>,
    pub interface: NominalTy,
    pub method_slot: u32,
    /// Arguments for this method only. The interface owns its trait arguments.
    pub arguments: Vec<Ty>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeNormalization {
    pub source: Ty,
    pub result: Ty,
}

pub struct InterfaceMethodSignature {
    pub params: Vec<Ty>,
    pub result: Ty,
    pub bounds: Vec<GenericBound>,
}

impl InterfaceMethodSignature {
    pub fn types_valid(
        &self,
        parameters: &[GenericParam],
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> bool {
        types_in_scope(
            self.params.iter().chain([&self.result]),
            parameters,
            cancel,
            definitions,
        ) && bounds_in_scope(&self.bounds, parameters, cancel, definitions)
    }

    pub fn physical_types(
        &self,
        parameters: &[GenericParam],
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> Option<(Vec<ValueType>, ValueType)> {
        self.types_valid(parameters, cancel, definitions).then(|| {
            (
                self.params.iter().map(semantic_representation).collect(),
                semantic_representation(&self.result),
            )
        })
    }
}

impl InterfaceCallContract {
    pub fn signature_in(
        &self,
        owner: &ModuleIdentity,
        items: &[PublicItem],
        contracts: &[TraitContract],
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> Result<InterfaceMethodSignature, TypeTransformError> {
        if definitions
            .resolve(self.interface.declaration)
            .map(|view| view.module())
            != Ok(owner)
        {
            return Err(TypeTransformError::InvalidContract);
        }
        let contract = trait_contract(
            owner,
            items,
            contracts,
            &self.interface.declaration,
            definitions,
        )
        .ok_or(TypeTransformError::InvalidContract)?;
        self.signature(contract, cancel, definitions)
    }

    /// Apply a validated declaration without resolving any implementation. A
    /// shared caller may forward its own parameters; check() verifies their scope
    /// and required bounds independently of this substitution.
    pub fn signature(
        &self,
        contract: &TraitDef,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> Result<InterfaceMethodSignature, TypeTransformError> {
        let mut signature = self.declared_signature(contract, cancel, definitions)?;
        if self.normalizations.len() > 4096 {
            return Err(TypeTransformError::LimitExceeded);
        }
        for (index, fact) in self.normalizations.iter().enumerate() {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            if fact.source == fact.result
                || !fact.source.within_wire_limits()
                || !fact.result.within_wire_limits()
                || self.normalizations[..index]
                    .iter()
                    .any(|previous| previous.source == fact.source)
                || !signature
                    .params
                    .iter()
                    .chain([&signature.result])
                    .any(|ty| ty == &fact.source)
            {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        for ty in signature.params.iter_mut().chain([&mut signature.result]) {
            if let Some(fact) = self.normalizations.iter().find(|fact| &fact.source == ty) {
                *ty = fact.result.clone();
            }
        }
        Ok(signature)
    }

    fn declared_signature(
        &self,
        contract: &TraitDef,
        cancel: &CancellationToken,
        definitions: &DefinitionTable,
    ) -> Result<InterfaceMethodSignature, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let invalid = TypeTransformError::InvalidContract;
        let interface = &self.interface;
        let view = definitions
            .resolve(interface.declaration)
            .map_err(|_| invalid)?;
        if view.segments().count() != 1
            || !view.last().is_some_and(|part| {
                part.kind == DefinitionKind::Trait
                    && part.occurrence == 0
                    && part.name == contract.name
            })
            || interface.arguments.len() != contract.generic_params.len()
            || (self.receiver.is_none()
                && (!contract.associated_consts.is_empty()
                    || contract
                        .associated_types
                        .iter()
                        .any(|member| !member.generic_params.is_empty())))
            || interface.associated_types.keys().any(|id| {
                !contract
                    .associated_types
                    .iter()
                    .any(|member| member.declaration == *id)
            })
            || (self.receiver.is_none()
                && contract
                    .associated_types
                    .iter()
                    .any(|member| !interface.associated_types.contains_key(&member.declaration)))
        {
            return Err(invalid);
        }
        let method = contract
            .methods
            .get(self.method_slot as usize)
            .ok_or(invalid)?;
        let has_receiver = method.params.first().is_some_and(|parameter| {
            parameter.name == "self" && parameter.ty == Ty::SelfType(interface.declaration)
        });
        if method.generic_params.len() != self.arguments.len()
            || (self.receiver.is_none() && !has_receiver)
        {
            return Err(invalid);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in contract
            .generic_params
            .iter()
            .zip(&interface.arguments)
            .chain(method.generic_params.iter().zip(&self.arguments))
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        if let Some(receiver) = &self.receiver {
            substitution.bind_receiver(&interface.declaration, receiver);
        }
        let projection_receiver = self
            .receiver
            .clone()
            .unwrap_or(Ty::SelfType(interface.declaration));
        let apply = |ty: &Ty| {
            resolve_associated_outputs(
                &substitution.apply(ty, cancel)?,
                interface,
                &projection_receiver,
                cancel,
            )
        };
        let mut params = if has_receiver {
            vec![
                self.receiver
                    .clone()
                    .unwrap_or_else(|| Ty::Trait(interface.clone())),
            ]
        } else {
            vec![]
        };
        for parameter in method.params.iter().skip(usize::from(has_receiver)) {
            params.push(apply(&parameter.ty)?);
        }
        let mut bounds = substitution.apply_bounds(&contract.bounds, definitions, cancel)?;
        bounds.extend(substitution.apply_bounds(&method.bounds, definitions, cancel)?);
        let bounds = resolve_interface_bounds(
            bounds,
            interface,
            &projection_receiver,
            self.receiver.is_none(),
            cancel,
        )?;
        Ok(InterfaceMethodSignature {
            params,
            result: apply(&method.return_type)?,
            bounds,
        })
    }

    /// The caller's scope and assumptions come from its verified body contract.
    /// A type argument is never accepted merely because it has a slot layout.
    pub fn check(
        &self,
        contract: &TraitDef,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let definitions = catalog.definitions();
        if self.receiver.is_none()
            && !dynamic_interface_valid(
                parameters,
                &self.interface,
                &|id| {
                    if *id == self.interface.declaration {
                        Some(contract)
                    } else {
                        catalog.trait_contract(id)
                    }
                },
                definitions,
                cancel,
            )?
        {
            return Ok(false);
        }
        let mut signature = self.signature(contract, cancel, definitions)?;
        for fact in &self.normalizations {
            if catalog.normalize(&fact.source, cancel)? != fact.result {
                return Ok(false);
            }
        }
        if !types_in_scope(
            self.arguments
                .iter()
                .chain(signature.params.iter())
                .chain([&signature.result]),
            parameters,
            cancel,
            definitions,
        ) {
            return Ok(false);
        }
        for bound in &mut signature.bounds {
            bound.ty = catalog.normalize(&bound.ty, cancel)?;
            for constraint in &mut bound.constraints {
                if let Constraint::Trait(interface) = constraint {
                    let Ty::Trait(resolved) =
                        catalog.normalize(&Ty::Trait(interface.clone()), cancel)?
                    else {
                        unreachable!()
                    };
                    *interface = resolved;
                }
            }
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(false);
            }
        }
        if let Some(receiver) = &self.receiver
            && !catalog.holds(&self.interface, receiver, assumptions, cancel)?
        {
            return Ok(false);
        }
        let expected = required_operations(
            &signature.bounds,
            &|id| catalog.trait_contract(id),
            cancel,
            definitions,
        )?
        .iter()
        .map(|required| normalize_requirement(required, catalog, cancel))
        .collect::<Result<Vec<_>, _>>()?;
        if expected.len() != self.operations.len()
            || expected.iter().any(|required| {
                self.operations
                    .iter()
                    .filter(|operation| operation.requirement() == required)
                    .count()
                    != 1
            })
        {
            return Ok(false);
        }
        if let Some(receiver) = &self.receiver {
            let required = NativeCallableRequirement {
                receiver: receiver.clone(),
                interface: self.interface.clone(),
                member: definitions
                    .lookup_child(
                        self.interface.declaration,
                        DefinitionKind::Method,
                        &contract.methods[self.method_slot as usize].name,
                        0,
                    )
                    .ok_or(TypeTransformError::InvalidContract)?,
                arguments: vec![],
            };
            let available = required_operations(
                assumptions,
                &|id| catalog.trait_contract(id),
                cancel,
                definitions,
            )?;
            if !available.contains(&required) {
                return Ok(false);
            }
        }
        for operation in &self.operations {
            if !operation.valid(catalog, parameters, assumptions, cancel)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

mod mapping;

/// Validate every applied member and parent before admitting a dynamic value or call.
/// Caller parameters remain valid output bindings; an erased receiver never does.
/// Static constraint dispatch does not use this admission check. `lookup` supplies
/// the complete declaration closure; `parameters` is the caller's binder scope.
///
/// Returns false for incompatible surfaces. Malformed declarations, cancellation
/// and bounded traversal exhaustion return a type transformation error.
pub fn dynamic_interface_valid<'a>(
    parameters: &[GenericParam],
    interface: &NominalTy,
    lookup: &impl Fn(&DefinitionId) -> Option<&'a TraitDef>,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<bool, TypeTransformError> {
    if !types_in_scope(
        [&Ty::Trait(interface.clone())],
        parameters,
        cancel,
        definitions,
    ) {
        return Ok(false);
    }
    let preserved = trait_closure(
        interface,
        &Ty::SelfType(interface.declaration),
        cancel,
        lookup,
    )?;
    let erased = trait_closure(interface, &Ty::Trait(interface.clone()), cancel, lookup)?;
    if preserved != erased {
        return Ok(false);
    }
    for view in interface_views(
        interface,
        &Ty::Trait(interface.clone()),
        cancel,
        definitions,
        lookup,
    )? {
        let Some(contract) = lookup(&view.declaration) else {
            return Ok(false);
        };
        if !contract.associated_consts.is_empty()
            || contract
                .associated_types
                .iter()
                .any(|member| !member.generic_params.is_empty())
            || view.associated_types.len() != contract.associated_types.len()
            || contract
                .associated_types
                .iter()
                .any(|member| !view.associated_types.contains_key(&member.declaration))
        {
            return Ok(false);
        }
        let substitution = TypeSubstitution::for_owner(&view.declaration, &view.arguments);
        let bounds = resolve_interface_bounds(
            substitution.apply_bounds(&contract.bounds, definitions, cancel)?,
            &view,
            &Ty::SelfType(view.declaration),
            true,
            cancel,
        )?;
        if !bounds_in_scope(&bounds, parameters, cancel, definitions) {
            return Ok(false);
        }
        for (slot, method) in contract.methods.iter().enumerate() {
            let call = InterfaceCallContract {
                normalizations: vec![],
                receiver: None,
                operations: vec![],
                interface: view.clone(),
                method_slot: slot as u32,
                arguments: method
                    .generic_params
                    .iter()
                    .map(GenericParam::as_type)
                    .collect(),
            };
            let signature = call.declared_signature(contract, cancel, definitions)?;
            let scope = parameters
                .iter()
                .chain(&method.generic_params)
                .cloned()
                .collect::<Vec<_>>();
            if !signature.types_valid(&scope, cancel, definitions) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn resolve_interface_bounds(
    mut bounds: Vec<GenericBound>,
    interface: &NominalTy,
    receiver: &Ty,
    dynamic: bool,
    cancel: &CancellationToken,
) -> Result<Vec<GenericBound>, TypeTransformError> {
    for bound in &mut bounds {
        bound.ty = resolve_associated_outputs(&bound.ty, interface, receiver, cancel)?;
        for constraint in &mut bound.constraints {
            if let Constraint::Trait(required) = constraint {
                let Ty::Trait(resolved) = resolve_associated_outputs(
                    &Ty::Trait(required.clone()),
                    interface,
                    receiver,
                    cancel,
                )?
                else {
                    return Err(TypeTransformError::InvalidContract);
                };
                *required = resolved;
            }
        }
    }
    if dynamic {
        // A boxed receiver already proves membership in its own interface.
        for bound in &mut bounds {
            if &bound.ty == receiver {
                bound.constraints.retain(|constraint| {
                    !matches!(constraint,
                        Constraint::Trait(required) if interface.satisfies(required))
                });
            }
        }
        bounds.retain(|bound| !bound.constraints.is_empty());
    }
    Ok(bounds)
}

fn bounds_in_scope(
    bounds: &[GenericBound],
    parameters: &[GenericParam],
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> bool {
    bounds.iter().all(|bound| {
        types_in_scope([&bound.ty], parameters, cancel, definitions)
            && bound.constraints.iter().all(|constraint| match constraint {
                Constraint::Standard(_) => true,
                Constraint::Trait(required) => types_in_scope(
                    [&Ty::Trait(required.clone())],
                    parameters,
                    cancel,
                    definitions,
                ),
            })
    })
}
