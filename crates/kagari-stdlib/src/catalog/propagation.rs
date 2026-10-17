//! Ordinary library declarations and implementations behind propagation syntax.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    declaration::{
        TraitDef,
        module::{DeclarationError, ImplDecl, ModuleDecl},
        requirement::NativeCallableRequirement,
    },
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};

fn projection(ids: &DefinitionContext, name: &str) -> Result<Ty, DeclarationError> {
    let owner = key::identity(ids, RegistrationTrait::Try)?;
    Ok(Ty::Projection {
        receiver: Box::new(Ty::SelfType(owner)),
        interface: Box::new(key::applied(ids, RegistrationTrait::Try, vec![])?),
        member: identities::associated(ids, &owner, name)?,
        arguments: vec![],
    })
}

pub(super) fn traits(ids: &DefinitionContext) -> Result<Vec<TraitDef>, DeclarationError> {
    let mut residual = contracts::contract(ids, RegistrationTrait::FromResidual, &["R"])?;
    let mut from_residual = contracts::method(
        "from_residual",
        vec![residual.generic_params[0].as_type()],
        contracts::receiver(ids, RegistrationTrait::FromResidual)?,
    );
    from_residual.params[0].name = "residual".into();
    residual.methods.push(from_residual);
    let mut carrier = contracts::contract(ids, RegistrationTrait::Try, &[])?;
    let owner = key::identity(ids, RegistrationTrait::Try)?;
    carrier.associated_types.extend([
        contracts::associated(ids, &owner, "Output", vec![])?,
        contracts::associated(ids, &owner, "Residual", vec![])?,
    ]);
    carrier.supertraits.push(key::applied(
        ids,
        RegistrationTrait::FromResidual,
        vec![projection(ids, "Residual")?],
    )?);
    let mut from_output = contracts::method(
        "from_output",
        vec![projection(ids, "Output")?],
        contracts::receiver(ids, RegistrationTrait::Try)?,
    );
    from_output.params[0].name = "output".into();
    carrier.methods.extend([
        from_output,
        contracts::method(
            "branch",
            vec![contracts::receiver(ids, RegistrationTrait::Try)?],
            contracts::enum_type(
                ids,
                "ControlFlow",
                vec![projection(ids, "Residual")?, projection(ids, "Output")?],
            )?,
        ),
    ]);
    Ok(vec![residual, carrier])
}

fn generic_parameters(
    ids: &DefinitionContext,
    module: &ModuleDecl,
    count: usize,
) -> Result<Vec<GenericParam>, DeclarationError> {
    let owner = identities::implementation(ids, module.implementations.len())?;
    Ok((0..count)
        .map(|position| GenericParam { owner, position })
        .collect())
}

pub(super) fn implementations(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    for name in ["Option", "Result", "ControlFlow"] {
        let parameters = generic_parameters(ids, module, if name == "Option" { 1 } else { 2 })?;
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let output = arguments[if name == "ControlFlow" { 1 } else { 0 }].clone();
        let infallible = contracts::enum_type(ids, "Infallible", vec![])?;
        let residual = match name {
            "Option" => contracts::enum_type(ids, name, vec![infallible])?,
            "Result" => contracts::enum_type(ids, name, vec![infallible, arguments[1].clone()])?,
            _ => contracts::enum_type(ids, name, vec![arguments[0].clone(), infallible])?,
        };
        let mut interface = key::applied(ids, RegistrationTrait::Try, vec![])?;
        interface.associated_types.insert(
            identities::associated(ids, &interface.declaration, "Output")?,
            output,
        );
        interface.associated_types.insert(
            identities::associated(ids, &interface.declaration, "Residual")?,
            residual,
        );
        implement(
            ids,
            module,
            RegistrationTrait::Try,
            interface,
            contracts::enum_type(ids, name, arguments)?,
            parameters,
            name,
        )?;

        let parameters = generic_parameters(
            ids,
            module,
            if name == "Result" {
                3
            } else if name == "Option" {
                1
            } else {
                2
            },
        )?;
        let arguments: Vec<_> = parameters.iter().map(GenericParam::as_type).collect();
        let infallible = contracts::enum_type(ids, "Infallible", vec![])?;
        let source = match name {
            "Option" => contracts::enum_type(ids, name, vec![infallible])?,
            "Result" => contracts::enum_type(ids, name, vec![infallible, arguments[2].clone()])?,
            _ => contracts::enum_type(ids, name, vec![arguments[0].clone(), infallible])?,
        };
        let interface = key::applied(ids, RegistrationTrait::FromResidual, vec![source])?;
        let receiver = contracts::enum_type(
            ids,
            name,
            arguments[..if name == "Option" { 1 } else { 2 }].to_vec(),
        )?;
        implement(
            ids,
            module,
            RegistrationTrait::FromResidual,
            interface,
            receiver,
            parameters,
            name,
        )?;
        if name == "Result" {
            let interface = key::applied(ids, RegistrationTrait::From, vec![arguments[2].clone()])?;
            let implementation = module.implementations.last_mut().unwrap();
            let bound = GenericBound {
                ty: arguments[1].clone(),
                constraints: vec![interface.clone()],
            };
            implementation.bounds.push(bound.clone());
            let member = identities::method(
                ids,
                &identities::implementation(ids, module.implementations.len() - 1)?,
                "from_residual",
            )?;
            module.callable_requirements.insert(
                member,
                vec![NativeCallableRequirement {
                    receiver: arguments[1].clone(),
                    member: identities::method(ids, &interface.declaration, "from")?,
                    interface,
                    arguments: vec![],
                }],
            );
        }
    }

    Ok(())
}

fn implement(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
    kind: RegistrationTrait,
    interface: NominalTy,
    receiver: Ty,
    parameters: Vec<GenericParam>,
    name: &str,
) -> Result<(), DeclarationError> {
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
            Ok((
                method.name.as_str(),
                identities::definition(
                    ids,
                    DefinitionKind::Function,
                    &format!("$foundation_propagation_{name}_{}", method.name),
                )?,
            ))
        })
        .collect::<Result<_, DeclarationError>>()?;
    module.implement_trait(
        &contract,
        ImplDecl {
            generic_params: parameters,
            bounds: vec![],
            trait_type: Some(interface),
            for_type: receiver,
            methods: vec![],
        },
        &bindings,
        ids,
    )?;

    Ok(())
}
