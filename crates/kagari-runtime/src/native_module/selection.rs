//! Resolve typed dependency descriptors under the registered declaration binder.
use crate::{
    error::RuntimeError,
    native_module::{
        Method, invalid, nominal,
        types::{Scope, TypeExpression},
    },
};
use kagari_abi::{
    native_api::NativeModule,
    native_import::callables::NativeCallableRequirement,
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi,
        substitution::{TypeSubstitution, resolve_associated_outputs},
    },
};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionKind};
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Selection {
    pub bounds: Vec<GenericBoundAbi>,
    pub requirements: Vec<NativeCallableRequirement>,
}

pub(super) fn resolve(scope: &Scope<'_>, method: &Method) -> Result<Selection, RuntimeError> {
    let mut bounds = BTreeMap::<AbiType, BTreeSet<ConstraintAbi>>::new();
    let mut requirements = vec![];
    let mut pending: Vec<_> = method.params.iter().map(|(_, ty)| ty).collect();
    pending.push(&method.result);
    for selected in &method.selected {
        pending.extend([&selected.receiver, &selected.interface, &selected.signature]);
    }
    while let Some(expression) = pending.pop() {
        match expression {
            TypeExpression::Constrained { ty, constraint } => {
                bounds
                    .entry(scope.resolve(ty)?)
                    .or_default()
                    .insert(ConstraintAbi::Standard(*constraint));
                pending.push(ty);
            }
            TypeExpression::Named {
                arguments,
                bindings,
                ..
            } => {
                pending.extend(arguments);
                pending.extend(bindings.iter().map(|(_, ty)| ty));
            }
            TypeExpression::Projection {
                receiver,
                interface,
                ..
            } => {
                pending.extend([receiver.as_ref(), interface.as_ref()]);
            }
            TypeExpression::Array(ty)
            | TypeExpression::MutableArray(ty)
            | TypeExpression::Iter(ty)
            | TypeExpression::Range(ty, _) => pending.push(ty),
            TypeExpression::Tuple(types) => pending.extend(types),
            TypeExpression::Function { params, result } => {
                pending.extend(params);
                pending.push(result);
            }
            TypeExpression::Parameter(_) | TypeExpression::Associated(_) => {}
        }
    }
    let cancel = CancellationToken::default();
    for selected in &method.selected {
        let receiver = scope.resolve(&selected.receiver)?;
        let interface = nominal(scope.resolve(&selected.interface)?)?;
        let local = scope.module.traits.iter().find(|contract| {
            scope
                .module
                .definition(DefinitionKind::Trait, &contract.name)
                == interface.declaration
        });
        let contract = if let Some(local) = local {
            local
        } else {
            scope
                .catalog
                .get(&interface.declaration)
                .ok_or_else(|| invalid("typed selection requires a registered trait"))?
        };
        let member = contract
            .methods
            .iter()
            .find(|member| member.name == selected.member)
            .ok_or_else(|| invalid("typed selection names an absent trait member"))?;
        if interface.arguments.len() != contract.generic_params.len()
            || !member.generic_params.is_empty()
        {
            return Err(invalid(
                "typed selection has unsupported trait or member arguments",
            ));
        }
        let mut substitution =
            TypeSubstitution::for_owner(&interface.declaration, &interface.arguments);
        substitution.bind_receiver(&interface.declaration, &receiver);
        let applied = |ty: &AbiType| {
            let ty = substitution
                .apply(ty, &cancel)
                .map_err(|_| invalid("typed selected member substitution"))?;
            resolve_associated_outputs(&ty, &interface, &cancel)
                .map_err(|_| invalid("typed selected member associated output"))
        };
        let expected = AbiType::Function {
            params: member
                .params
                .iter()
                .map(|param| applied(&param.ty))
                .collect::<Result<_, _>>()?,
            result: Box::new(applied(&member.return_type)?),
        };
        if scope.resolve(&selected.signature)? != expected {
            return Err(invalid(
                "typed selected argument/result signature differs from its trait",
            ));
        }
        // A projected callback also requires its base receiver's actual trait
        // application. An embedded Output = U cannot establish that authority.
        let mut base = &receiver;
        while let AbiType::Projection {
            receiver,
            interface,
            ..
        } = base
        {
            bounds
                .entry((**receiver).clone())
                .or_default()
                .insert(ConstraintAbi::Trait((**interface).clone()));
            base = receiver;
        }
        bounds
            .entry(receiver.clone())
            .or_default()
            .insert(ConstraintAbi::Trait(interface.clone()));
        requirements.push(NativeCallableRequirement {
            receiver,
            member: NativeModule::method_id(&interface.declaration, selected.member),
            interface,
            arguments: vec![],
        });
    }
    Ok(Selection {
        bounds: bounds
            .into_iter()
            .map(|(ty, constraints)| GenericBoundAbi {
                ty,
                constraints: constraints.into_iter().collect(),
            })
            .collect(),
        requirements,
    })
}
