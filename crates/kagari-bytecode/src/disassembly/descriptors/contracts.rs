//! Borrowed declaration spelling and shared witness rendering for descriptors.

use crate::disassembly::{
    presentation::DisplayContext,
    resolution::{Resolution, contracts},
};
use kagari_contract::{callable::witness::OperationWitness, types::ConcreteFunctionIdentity};
use kagari_types::{
    callable::{CallableImplementation, NativeDefaultApplication, Signature},
    declaration::requirement::NativeCallableRequirement,
    reference::ReferenceAccess,
    ty::{GenericBound, GenericParam, NominalTy, Ty},
};
use std::io::{self, Write};

pub(super) fn arguments(context: &DisplayContext<'_>, types: &[Ty]) -> String {
    format!(
        "[{}]",
        types
            .iter()
            .map(|ty| context.ty(ty))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn instance(
    context: &DisplayContext<'_>,
    identity: &ConcreteFunctionIdentity,
) -> String {
    let ConcreteFunctionIdentity {
        declaration,
        arguments: types,
    } = identity;
    format!(
        "{} arguments={}",
        context.definition(*declaration),
        arguments(context, types)
    )
}

pub(super) fn nominal(context: &DisplayContext<'_>, ty: &NominalTy) -> String {
    let NominalTy {
        declaration,
        arguments: types,
        associated_types,
    } = ty;
    let mut bindings: Vec<_> = associated_types.iter().collect();
    bindings.sort_by(|(left, _), (right, _)| {
        context
            .definitions
            .compare(**left, **right)
            .expect("verified associated identities")
    });
    let bindings = bindings
        .into_iter()
        .map(|(member, ty)| format!("{} = {}", context.definition(*member), context.ty(ty)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{} arguments={} associated=[{bindings}]",
        context.definition(*declaration),
        arguments(context, types)
    )
}

pub(super) fn signature(context: &DisplayContext<'_>, signature: &Signature) -> String {
    let Signature { params, result } = signature;
    format!(
        "params={} -> {}",
        arguments(context, params),
        context.ty(result)
    )
}

pub(super) fn access(access: Option<ReferenceAccess>) -> &'static str {
    match access {
        None => "none",
        Some(ReferenceAccess::Shared) => "shared",
        Some(ReferenceAccess::Mutable) => "mutable",
    }
}

pub(super) fn implementation(
    context: &DisplayContext<'_>,
    value: &CallableImplementation,
) -> String {
    match value {
        CallableImplementation::Required => "required".into(),
        CallableImplementation::Script => "script".into(),
        CallableImplementation::Native(binding) => {
            format!("native {}", context.definition(*binding))
        }
        CallableImplementation::NativeDefault(application) => {
            let NativeDefaultApplication {
                declaration,
                arguments: types,
            } = application;
            format!(
                "native_default {} arguments={}",
                context.definition(*declaration),
                arguments(context, types)
            )
        }
    }
}

pub(super) fn requirement(
    context: &DisplayContext<'_>,
    required: &NativeCallableRequirement,
) -> String {
    let NativeCallableRequirement {
        receiver,
        interface,
        member,
        arguments: types,
    } = required;
    format!(
        "receiver={} interface=({}) member={} arguments={}",
        context.ty(receiver),
        nominal(context, interface),
        context.definition(*member),
        arguments(context, types)
    )
}

pub(super) fn parameters(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    prefix: &str,
    parameters: &[GenericParam],
) -> io::Result<()> {
    for (index, parameter) in parameters.iter().enumerate() {
        let GenericParam { owner, position } = parameter;
        writeln!(
            writer,
            "{prefix}parameter#{index} owner={} position={position}",
            context.definition(*owner)
        )?;
    }
    Ok(())
}

pub(super) fn bounds(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    prefix: &str,
    bounds: &[GenericBound],
) -> io::Result<()> {
    for (index, bound) in bounds.iter().enumerate() {
        let GenericBound { ty, constraints } = bound;
        writeln!(
            writer,
            "{prefix}requirement#{index} type={}",
            context.ty(ty)
        )?;
        for (index, constraint) in constraints.iter().enumerate() {
            writeln!(
                writer,
                "{prefix}  constraint#{index} {}",
                nominal(context, constraint)
            )?;
        }
    }
    Ok(())
}

pub(super) fn table_reference(
    resolution: &Resolution<'_>,
    identity: &ConcreteFunctionIdentity,
) -> String {
    let (owner, slot) = resolution.interface(identity);
    format!("@{owner}:interface#{slot}")
}

pub(super) fn operations(
    writer: &mut impl Write,
    context: &DisplayContext<'_>,
    resolution: &Resolution<'_>,
    prefix: &str,
    operations: &[OperationWitness],
) -> io::Result<()> {
    for (index, operation) in operations.iter().enumerate() {
        write!(writer, "{prefix}operation#{index} ")?;
        contracts::witness(writer, context, resolution, operation)?;
        writeln!(writer)?;
    }
    Ok(())
}
