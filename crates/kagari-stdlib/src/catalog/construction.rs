//! Foundational conversion, parsing and aggregation contracts. Algorithms are
//! supplied by ordinary implementations, independently of declaration ownership.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::{
        conversion::ConversionAdapter,
        module::{DeclarationError, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{GenericBound, GenericParam, Ty},
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for (kind, name, parameters, error) in [
        (RegistrationTrait::Into, "into", &["Target"][..], None),
        (
            RegistrationTrait::TryFrom,
            "try_from",
            &["Source"][..],
            Some("Error"),
        ),
        (
            RegistrationTrait::TryInto,
            "try_into",
            &["Target"][..],
            Some("Error"),
        ),
        (RegistrationTrait::FromStr, "from_str", &[][..], Some("Err")),
    ] {
        let mut declaration = contracts::contract(ids, kind, parameters)?;
        let owner = key::identity(ids, kind)?;
        let this = contracts::receiver(ids, kind)?;
        let input = if kind == RegistrationTrait::FromStr {
            Ty::Builtin(BuiltinType::String)
        } else {
            declaration.generic_params[0].as_type()
        };
        let instance = matches!(kind, RegistrationTrait::Into | RegistrationTrait::TryInto);
        let result = if instance {
            input.clone()
        } else {
            this.clone()
        };
        let result = if let Some(name) = error {
            declaration
                .associated_types
                .push(contracts::associated(ids, &owner, name, vec![])?);
            contracts::enum_type(
                ids,
                "Result",
                vec![
                    result,
                    Ty::Projection {
                        receiver: Box::new(this.clone()),
                        interface: Box::new(key::applied(
                            ids,
                            kind,
                            declaration
                                .generic_params
                                .iter()
                                .map(GenericParam::as_type)
                                .collect(),
                        )?),
                        member: identities::associated(ids, &owner, name)?,
                        arguments: vec![],
                    },
                ],
            )?
        } else {
            result
        };
        let mut method = contracts::method(name, vec![if instance { this } else { input }], result);
        if !instance {
            method.params[0].name = if kind == RegistrationTrait::FromStr {
                "text"
            } else {
                "value"
            }
            .into();
        }
        declaration.conversion_adapter = match kind {
            RegistrationTrait::Into | RegistrationTrait::TryInto => {
                let forward = key::identity(
                    ids,
                    if kind == RegistrationTrait::Into {
                        RegistrationTrait::From
                    } else {
                        RegistrationTrait::TryFrom
                    },
                )?;
                Some(ConversionAdapter::Reverse {
                    method: identities::method(
                        ids,
                        &forward,
                        if kind == RegistrationTrait::Into {
                            "from"
                        } else {
                            "try_from"
                        },
                    )?,
                    error: if kind == RegistrationTrait::TryInto {
                        Some((
                            identities::associated(ids, &owner, "Error")?,
                            identities::associated(ids, &forward, "Error")?,
                        ))
                    } else {
                        None
                    },
                    origin: forward,
                    result: if kind == RegistrationTrait::TryInto {
                        Some(identities::definition(ids, DefinitionKind::Enum, "Result")?)
                    } else {
                        None
                    },
                })
            }
            RegistrationTrait::TryFrom => Some(ConversionAdapter::Forward {
                method: identities::method(ids, &owner, "try_from")?,
                error: identities::associated(ids, &owner, "Error")?,
                result: identities::definition(ids, DefinitionKind::Enum, "Result")?,
            }),
            _ => None,
        };
        declaration.methods.push(method);
        module
            .documentation
            .insert(owner, format!("Language-owned {} contract.", kind.name()));
        module.traits.push(declaration);
    }
    for (kind, name) in [
        (RegistrationTrait::FromIterator, "from_iter"),
        (RegistrationTrait::Sum, "sum"),
        (RegistrationTrait::Product, "product"),
    ] {
        let mut declaration = contracts::contract(ids, kind, &["T"])?;
        let owner = key::identity(ids, kind)?;
        let source = GenericParam {
            owner: identities::method(ids, &owner, name)?,
            position: 0,
        };
        let mut method = contracts::method(
            name,
            vec![source.as_type()],
            contracts::receiver(ids, kind)?,
        );
        method.params[0].name = "source".into();
        method.bounds.push(GenericBound {
            ty: source.as_type(),
            constraints: vec![contracts::applied_item(
                ids,
                RegistrationTrait::Iterable,
                declaration.generic_params[0].as_type(),
            )?],
        });
        method.generic_params.push(source);
        declaration.methods.push(method);
        module
            .documentation
            .insert(owner, format!("Language-owned {} contract.", kind.name()));
        module.traits.push(declaration);
    }

    Ok(())
}
