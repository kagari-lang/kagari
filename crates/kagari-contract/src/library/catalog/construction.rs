//! Foundational conversion, parsing and aggregation contracts. Algorithms are
//! supplied by ordinary implementations, independently of declaration ownership.
use crate::library::catalog::contracts;
use crate::library::catalog::key::{self, RegistrationTrait};
use kagari_common::identity::associated_type_id;
use kagari_types::{
    declaration::{conversion::ConversionAdapter, module::ModuleDecl},
    scalar::BuiltinType,
    surface::StandardEnum,
    ty::{Constraint, GenericBound, GenericParam, Ty},
};

pub(super) fn declare(module: &mut ModuleDecl) {
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
        let mut declaration = contracts::contract(kind, parameters);
        let owner = key::identity(kind);
        let this = contracts::receiver(kind);
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
                .push(contracts::associated(&owner, name, vec![]));
            contracts::enum_type(
                StandardEnum::Result,
                vec![
                    result,
                    Ty::Projection {
                        receiver: Box::new(this.clone()),
                        interface: Box::new(key::applied(
                            kind,
                            declaration
                                .generic_params
                                .iter()
                                .map(GenericParam::as_type)
                                .collect(),
                        )),
                        member: associated_type_id(&owner, name),
                        arguments: vec![],
                    },
                ],
            )
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
                let forward = key::identity(if kind == RegistrationTrait::Into {
                    RegistrationTrait::From
                } else {
                    RegistrationTrait::TryFrom
                });
                Some(ConversionAdapter::Reverse {
                    method: ModuleDecl::method_id(
                        &forward,
                        if kind == RegistrationTrait::Into {
                            "from"
                        } else {
                            "try_from"
                        },
                    ),
                    error: (kind == RegistrationTrait::TryInto).then(|| {
                        (
                            associated_type_id(&owner, "Error"),
                            associated_type_id(&forward, "Error"),
                        )
                    }),
                    origin: forward,
                })
            }
            RegistrationTrait::TryFrom => Some(ConversionAdapter::CheckedNumeric {
                method: ModuleDecl::method_id(&owner, "try_from"),
                error: associated_type_id(&owner, "Error"),
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
        let mut declaration = contracts::contract(kind, &["T"]);
        let owner = key::identity(kind);
        let source = GenericParam {
            owner: ModuleDecl::method_id(&owner, name),
            position: 0,
        };
        let mut method = contracts::method(name, vec![source.as_type()], contracts::receiver(kind));
        method.params[0].name = "source".into();
        method.bounds.push(GenericBound {
            ty: source.as_type(),
            constraints: vec![Constraint::Trait(contracts::applied_item(
                RegistrationTrait::Iterable,
                declaration.generic_params[0].as_type(),
            ))],
        });
        method.generic_params.push(source);
        declaration.methods.push(method);
        module
            .documentation
            .insert(owner, format!("Language-owned {} contract.", kind.name()));
        module.traits.push(declaration);
    }
}
