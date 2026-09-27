//! Engine-owned trait templates expanded directly from standard descriptors.
use crate::standard::declarations::{ApiBound, ApiMethod};
use crate::standard::resolve::Arguments;
use crate::standard::traits::StandardTrait;
use crate::types::{
    AbiType, AssociatedTypeAbi, ConstraintAbi, FunctionAbi, GenericBoundAbi, GenericParameterAbi,
    NominalAbiType, ParameterAbi, TraitAbi,
};
use std::collections::BTreeMap;

pub(crate) fn trait_contract(kind: StandardTrait) -> Option<TraitAbi> {
    let declaration = kind.declaration();
    let id = declaration.item.identity();
    let generic_params: Vec<_> = declaration
        .generics
        .iter()
        .enumerate()
        .map(|(position, _)| GenericParameterAbi {
            owner: id.clone(),
            position,
        })
        .collect();
    let mut arguments: Arguments = declaration
        .generics
        .iter()
        .copied()
        .zip(generic_params.iter().map(parameter_type))
        .collect();
    arguments.insert("Self", AbiType::SelfType(id.clone()));
    arguments.insert(
        "@self_trait",
        AbiType::Trait(NominalAbiType {
            declaration: id,
            arguments: generic_params.iter().map(parameter_type).collect(),
            associated_types: BTreeMap::new(),
        }),
    );
    let mut associated_types: Vec<_> = declaration
        .associated_types
        .iter()
        .map(|member| {
            Some(AssociatedTypeAbi {
                declaration: member.item.identity(),
                generic_params: vec![],
                parameter_bounds: vec![],
                bounds: constraints(member.bounds, &arguments)?,
            })
        })
        .collect::<Option<_>>()?;
    // The source contract used a map: preserve canonical identity order on wire.
    associated_types.sort_by(|a, b| a.declaration.cmp(&b.declaration));
    Some(TraitAbi {
        name: kind.name().into(),
        generic_params,
        bounds: vec![],
        supertraits: declaration
            .supertraits
            .iter()
            .map(|bound| bound.resolve(&arguments))
            .collect::<Option<_>>()?,
        associated_consts: vec![],
        associated_types,
        default_methods: declaration
            .methods
            .iter()
            .enumerate()
            .filter_map(|(slot, method)| method.native_default.map(|_| slot))
            .collect(),
        methods: declaration
            .methods
            .iter()
            .map(|method| method_contract(method, &arguments))
            .collect::<Option<_>>()?,
    })
}

fn parameter_type(parameter: &GenericParameterAbi) -> AbiType {
    AbiType::Parameter {
        owner: parameter.owner.clone(),
        position: parameter.position,
    }
}

fn constraints(bounds: &[ApiBound], arguments: &Arguments) -> Option<Vec<ConstraintAbi>> {
    bounds
        .iter()
        .map(|bound| Some(ConstraintAbi::Trait(bound.resolve(arguments)?)))
        .collect()
}

fn method_contract(method: &ApiMethod, outer: &Arguments) -> Option<FunctionAbi> {
    let mut arguments = outer.clone();
    let generic_params: Vec<_> = method
        .generics
        .iter()
        .enumerate()
        .map(|(position, generic)| {
            let parameter = GenericParameterAbi {
                owner: method.item.identity(),
                position,
            };
            arguments.insert(generic.name, parameter_type(&parameter));
            parameter
        })
        .collect();
    let mut bounds = BTreeMap::<AbiType, Vec<ConstraintAbi>>::new();
    for generic in method.generics {
        if let Some(bound) = generic
            .bounds
            .iter()
            .find(|bound| matches!(bound.name, "Iterator" | "Iterable"))
        {
            arguments.insert(
                generic.projection_key,
                AbiType::Trait(bound.resolve(&arguments)?),
            );
        }
        bounds.insert(
            arguments.get(generic.name)?.clone(),
            constraints(generic.bounds, &arguments)?,
        );
    }
    for (target, required) in method.bounds {
        bounds
            .entry(target.resolve(&arguments)?)
            .or_default()
            .extend(constraints(required, &arguments)?);
    }
    let bounds = bounds
        .into_iter()
        .filter_map(|(ty, mut constraints)| {
            constraints.sort();
            constraints.dedup();
            (!constraints.is_empty()).then_some(GenericBoundAbi { ty, constraints })
        })
        .collect();
    Some(FunctionAbi {
        name: method.item.path.last()?.1.into(),
        generic_params,
        bounds,
        params: method
            .params
            .iter()
            .map(|parameter| {
                Some(ParameterAbi {
                    name: parameter.name.into(),
                    ty: parameter.ty.resolve(&arguments)?,
                    mutable: false,
                })
            })
            .collect::<Option<_>>()?,
        return_type: method.result.resolve(&arguments)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standard::traits;
    use kagari_common::identity::associated_type_id;

    #[test]
    fn every_generated_trait_expands_into_a_portable_contract() {
        for kind in StandardTrait::ALL {
            let contract = trait_contract(kind).unwrap_or_else(|| panic!("{}", kind.name()));
            assert_eq!(contract.methods.len(), kind.declaration().methods.len());
            assert_eq!(
                contract.generic_params.len(),
                kind.declaration().generics.len()
            );
            assert!(
                contract
                    .associated_types
                    .windows(2)
                    .all(|pair| pair[0].declaration < pair[1].declaration)
            );
        }
    }

    #[test]
    fn operator_signature_preserves_self_and_associated_output_identity() {
        let kind = StandardTrait::Add;
        let id = traits::identity(kind);
        let contract = trait_contract(kind).unwrap();
        let method = &contract.methods[0];
        assert_eq!(method.params[0].ty, AbiType::SelfType(id.clone()));
        let AbiType::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = &method.return_type
        else {
            panic!("operator output must retain its projection");
        };
        assert_eq!(**receiver, AbiType::SelfType(id.clone()));
        assert_eq!(interface.declaration, id);
        assert_eq!(*member, associated_type_id(&id, "Output"));
        assert!(arguments.is_empty());
    }
}
