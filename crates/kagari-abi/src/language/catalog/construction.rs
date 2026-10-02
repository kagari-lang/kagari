//! Foundational conversion, parsing and aggregation contracts. Algorithms are
//! supplied by ordinary implementations, independently of declaration ownership.
use crate::{
    declaration::ModuleDecl,
    language::{self, Protocol, catalog::contracts, primitive},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AbiType, ConstraintAbi, GenericBoundAbi, GenericParameterAbi},
};
use kagari_common::identity::associated_type_id;

pub(super) fn declare(module: &mut ModuleDecl) {
    for (kind, name, parameters, error) in [
        (Protocol::Into, "into", &["Target"][..], None),
        (
            Protocol::TryFrom,
            "try_from",
            &["Source"][..],
            Some("Error"),
        ),
        (
            Protocol::TryInto,
            "try_into",
            &["Target"][..],
            Some("Error"),
        ),
        (Protocol::FromStr, "from_str", &[][..], Some("Err")),
    ] {
        let mut declaration = contracts::contract(kind, parameters);
        let owner = language::identity(kind);
        let this = contracts::receiver(kind);
        let input = if kind == Protocol::FromStr {
            AbiType::Builtin(BuiltinType::String)
        } else {
            declaration.generic_params[0].as_type()
        };
        let instance = matches!(kind, Protocol::Into | Protocol::TryInto);
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
                    AbiType::Projection {
                        receiver: Box::new(this.clone()),
                        interface: Box::new(primitive::applied(
                            kind,
                            declaration
                                .generic_params
                                .iter()
                                .map(GenericParameterAbi::as_type)
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
            method.params[0].name = if kind == Protocol::FromStr {
                "text"
            } else {
                "value"
            }
            .into();
        }
        declaration.methods.push(method);
        module
            .documentation
            .insert(owner, format!("Language-owned {} contract.", kind.name()));
        module.traits.push(declaration);
    }
    for (kind, name) in [
        (Protocol::FromIterator, "from_iter"),
        (Protocol::Sum, "sum"),
        (Protocol::Product, "product"),
    ] {
        let mut declaration = contracts::contract(kind, &["T"]);
        let owner = language::identity(kind);
        let source = GenericParameterAbi {
            owner: ModuleDecl::method_id(&owner, name),
            position: 0,
        };
        let mut method = contracts::method(name, vec![source.as_type()], contracts::receiver(kind));
        method.params[0].name = "source".into();
        method.bounds.push(GenericBoundAbi {
            ty: source.as_type(),
            constraints: vec![ConstraintAbi::Trait(contracts::applied_item(
                Protocol::Iterable,
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
