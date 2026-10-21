//! Declaration-derived bounds shared by method and construction capabilities.
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::{
    declaration::TraitDef,
    ty::{
        GenericBound, Ty, inheritance,
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};

/// Expand parents and immediate non-generic associated constraints once.
/// This is finite declaration traversal; associated bounds do not recursively expand.
pub(crate) fn expanded_bounds<'a>(
    bounds: &[GenericBound],
    lookup: &impl Fn(&DefinitionId) -> Option<&'a TraitDef>,
    cancel: &CancellationToken,
) -> Result<Vec<GenericBound>, TypeTransformError> {
    if bounds.len() > 4096 || bounds.iter().any(|bound| bound.constraints.len() > 4096) {
        return Err(TypeTransformError::LimitExceeded);
    }
    cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
    // A bound also promises its immediate associated-type contracts. In
    // particular Iterable supplies the selected next for its Iter projection.
    let copier = TypeSubstitution::default();
    let mut expanded = bounds
        .iter()
        .map(|bound| {
            Ok(GenericBound {
                ty: copier.apply(&bound.ty, cancel)?,
                constraints: bound
                    .constraints
                    .iter()
                    .map(|interface| copier.apply_nominal(interface, cancel))
                    .collect::<Result<_, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, TypeTransformError>>()?;
    for bound in bounds {
        for interface in &bound.constraints {
            for interface in inheritance::trait_closure(interface, &bound.ty, cancel, lookup)? {
                let contract =
                    lookup(&interface.declaration).ok_or(TypeTransformError::InvalidContract)?;
                let mut substitution = TypeSubstitution::default();
                substitution.bind_receiver(&interface.declaration, &bound.ty);
                for (parameter, argument) in
                    contract.generic_params.iter().zip(&interface.arguments)
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                for member in &contract.associated_types {
                    if !member.generic_params.is_empty() || member.bounds.is_empty() {
                        continue;
                    }
                    let apply = |ty: &Ty| {
                        resolve_associated_outputs(
                            &substitution.apply(ty, cancel)?,
                            &interface,
                            &bound.ty,
                            cancel,
                        )
                    };
                    let receiver = apply(&Ty::Projection {
                        receiver: Box::new(Ty::SelfType(interface.declaration)),
                        interface: Box::new(interface.clone()),
                        member: member.declaration,
                        arguments: vec![],
                    })?;
                    let constraints = member
                        .bounds
                        .iter()
                        .map(|required| {
                            let Ty::Trait(required) = apply(&Ty::Trait(required.clone()))? else {
                                return Err(TypeTransformError::InvalidContract);
                            };
                            Ok(required)
                        })
                        .collect::<Result<_, _>>()?;
                    expanded.push(GenericBound {
                        ty: receiver,
                        constraints,
                    });
                    if expanded.len() > 4096 {
                        return Err(TypeTransformError::LimitExceeded);
                    }
                }
            }
        }
    }
    Ok(expanded)
}
