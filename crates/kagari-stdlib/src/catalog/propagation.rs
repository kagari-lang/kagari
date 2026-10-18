//! Ordinary library declarations and implementations behind propagation syntax.
use crate::catalog::{
    contracts, identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    declaration::{TraitDef, module::DeclarationError},
    ty::Ty,
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
