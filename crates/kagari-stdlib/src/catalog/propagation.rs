//! Ordinary library declarations and implementations behind propagation syntax.
use crate::catalog::{
    contracts,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, associated_type_id};
use kagari_types::{
    declaration::{TraitDef, module::ModuleDecl, requirement::NativeCallableRequirement},
    ty::{Constraint, GenericBound, GenericParam, NominalTy, Ty},
};

fn projection(name: &str) -> Ty {
    let owner = key::identity(RegistrationTrait::Try);
    Ty::Projection {
        receiver: Box::new(Ty::SelfType(owner.clone())),
        interface: Box::new(key::applied(RegistrationTrait::Try, vec![])),
        member: associated_type_id(&owner, name),
        arguments: vec![],
    }
}

pub(super) fn traits() -> Vec<TraitDef> {
    let mut residual = contracts::contract(RegistrationTrait::FromResidual, &["R"]);
    let mut from_residual = contracts::method(
        "from_residual",
        vec![residual.generic_params[0].as_type()],
        contracts::receiver(RegistrationTrait::FromResidual),
    );
    from_residual.params[0].name = "residual".into();
    residual.methods.push(from_residual);
    let mut carrier = contracts::contract(RegistrationTrait::Try, &[]);
    let owner = key::identity(RegistrationTrait::Try);
    carrier.associated_types.extend([
        contracts::associated(&owner, "Output", vec![]),
        contracts::associated(&owner, "Residual", vec![]),
    ]);
    carrier.supertraits.push(key::applied(
        RegistrationTrait::FromResidual,
        vec![projection("Residual")],
    ));
    let mut from_output = contracts::method(
        "from_output",
        vec![projection("Output")],
        contracts::receiver(RegistrationTrait::Try),
    );
    from_output.params[0].name = "output".into();
    carrier.methods.extend([
        from_output,
        contracts::method(
            "branch",
            vec![contracts::receiver(RegistrationTrait::Try)],
            contracts::enum_type(
                "ControlFlow",
                vec![projection("Residual"), projection("Output")],
            ),
        ),
    ]);
    vec![residual, carrier]
}

fn generic_parameters(module: &ModuleDecl, count: usize) -> Vec<GenericParam> {
    let owner = module.implementation_id(module.implementations.len());
    (0..count)
        .map(|position| GenericParam {
            owner: owner.clone(),
            position,
        })
        .collect()
}

pub(super) fn implementations(module: &mut ModuleDecl) {
    for name in ["Option", "Result", "ControlFlow"] {
        let parameters = generic_parameters(module, if name == "Option" { 1 } else { 2 });
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let output = arguments[if name == "ControlFlow" { 1 } else { 0 }].clone();
        let infallible = contracts::enum_type("Infallible", vec![]);
        let residual = match name {
            "Option" => contracts::enum_type(name, vec![infallible]),
            "Result" => contracts::enum_type(name, vec![infallible, arguments[1].clone()]),
            _ => contracts::enum_type(name, vec![arguments[0].clone(), infallible]),
        };
        let mut interface = key::applied(RegistrationTrait::Try, vec![]);
        interface
            .associated_types
            .insert(associated_type_id(&interface.declaration, "Output"), output);
        interface.associated_types.insert(
            associated_type_id(&interface.declaration, "Residual"),
            residual,
        );
        implement(
            module,
            RegistrationTrait::Try,
            interface,
            contracts::enum_type(name, arguments),
            parameters,
            name,
        );

        let parameters = generic_parameters(
            module,
            if name == "Result" {
                3
            } else if name == "Option" {
                1
            } else {
                2
            },
        );
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let infallible = contracts::enum_type("Infallible", vec![]);
        let source = match name {
            "Option" => contracts::enum_type(name, vec![infallible]),
            "Result" => contracts::enum_type(name, vec![infallible, arguments[2].clone()]),
            _ => contracts::enum_type(name, vec![arguments[0].clone(), infallible]),
        };
        let interface = key::applied(RegistrationTrait::FromResidual, vec![source]);
        let receiver = contracts::enum_type(
            name,
            arguments[..if name == "Option" { 1 } else { 2 }].to_vec(),
        );
        implement(
            module,
            RegistrationTrait::FromResidual,
            interface,
            receiver,
            parameters,
            name,
        );
        if name == "Result" {
            let interface = key::applied(RegistrationTrait::From, vec![arguments[2].clone()]);
            let implementation = module.implementations.last_mut().unwrap();
            let bound = GenericBound {
                ty: arguments[1].clone(),
                constraints: vec![Constraint::Trait(interface.clone())],
            };
            implementation.bounds.push(bound.clone());
            let member = ModuleDecl::method_id(
                &module.implementation_id(module.implementations.len() - 1),
                "from_residual",
            );
            module.callable_requirements.insert(
                member,
                vec![NativeCallableRequirement {
                    receiver: arguments[1].clone(),
                    member: ModuleDecl::method_id(&interface.declaration, "from"),
                    interface,
                    arguments: vec![],
                }],
            );
        }
    }
}

fn implement(
    module: &mut ModuleDecl,
    kind: RegistrationTrait,
    interface: NominalTy,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    name: &str,
) {
    let contract = module
        .traits
        .iter()
        .find(|contract| contract.name == kind.name())
        .expect("propagation contract")
        .clone();
    let bindings: Vec<_> = contract
        .methods
        .iter()
        .map(|method| {
            (
                method.name.as_str(),
                module.definition(
                    DefinitionKind::Function,
                    &format!("$foundation_propagation_{name}_{}", method.name),
                ),
            )
        })
        .collect();
    module
        .implement_trait(&contract, interface, receiver, parameters, &bindings)
        .expect("propagation implementation");
}
