//! Carrier owners bind propagation bodies directly; trait signatures remain authoritative.
use crate::{
    bindings::{self, propagation},
    foundation::{bound, enumeration, interface, override_entry, raw},
};
use kagari_runtime::native::{
    assembly::{
        authoring::PendingBinding,
        draft::{ModuleDraft, TraitImplementation},
    },
    binding::{Codec, NativeBinding, NativeResult},
};

pub(super) fn declare(module: &mut ModuleDraft<'_>, name: &str) -> NativeResult<()> {
    let (arity, branch, output, residual) = match name {
        "Option" => (
            1,
            propagation::option_branch as bindings::Entry,
            propagation::option_from_output as bindings::Entry,
            propagation::option_from_residual as bindings::Entry,
        ),
        "Result" => (
            2,
            propagation::result_branch as bindings::Entry,
            propagation::result_from_output as bindings::Entry,
            propagation::result_from_residual as bindings::Entry,
        ),
        "ControlFlow" => (
            2,
            propagation::control_flow_branch as bindings::Entry,
            propagation::control_flow_from_output as bindings::Entry,
            propagation::control_flow_from_residual as bindings::Entry,
        ),
        _ => unreachable!("explicit carrier contributor"),
    };
    let carrier = enumeration(module, name, arity)?;
    let infallible = enumeration(module, "Infallible", 0)?.apply([])?;
    let protocol = interface(module, "Try", 0)?;
    let output_member = module.reference_associated_type(&protocol, "Output", 0)?;
    let residual_member = module.reference_associated_type(&protocol, "Residual", 0)?;
    let branch = override_entry(module, &protocol, "branch", 0, raw(1, branch))?;
    let output = override_entry(module, &protocol, "from_output", 0, raw(1, output))?;
    module.trait_implementation_with(arity, |arguments| {
        let output_type = arguments[usize::from(name == "ControlFlow")].clone();
        let residual = carrier.apply(match name {
            "Option" => vec![infallible.clone()],
            "Result" => vec![infallible.clone(), arguments[1].clone()],
            _ => vec![arguments[0].clone(), infallible.clone()],
        })?;
        Ok(TraitImplementation {
            receiver: carrier.apply(arguments.to_vec())?,
            interface: residual_member.bind(
                output_member.bind(protocol.apply([])?, output_type)?,
                residual,
            )?,
            bounds: vec![],
            bindings: vec![],
        }
        .with_overrides([branch, output]))
    })?;
    let from_residual = interface(module, "FromResidual", 1)?;
    let mut entry = override_entry(module, &from_residual, "from_residual", 0, raw(1, residual))?;
    let from = interface(module, "From", 1)?;
    let from_method = module.reference_method(&from, "from", 0)?;
    module.trait_implementation_with(if name == "Result" { 3 } else { arity }, |arguments| {
        let source = carrier.apply(match name {
            "Option" => vec![infallible.clone()],
            "Result" => vec![infallible.clone(), arguments[2].clone()],
            _ => vec![arguments[0].clone(), infallible.clone()],
        })?;
        let bounds = if name == "Result" {
            let from = from.apply([arguments[2].clone()])?;
            entry =
                entry.requires(from_method.requirement(arguments[1].clone(), from.clone(), [])?);
            vec![bound(&arguments[1], from)]
        } else {
            vec![]
        };
        Ok(TraitImplementation {
            receiver: carrier.apply(arguments[..arity].to_vec())?,
            interface: from_residual.apply([source])?,
            bounds,
            bindings: vec![],
        }
        .with_overrides([entry]))
    })?;
    if name == "Option" {
        let default = interface(module, "Default", 0)?;
        let entry = override_entry(module, &default, "default", 0, PendingBinding::implementation_send(NativeBinding::new(vec![], Codec::Value, |cx| bindings::option(cx, None))))?
            .documentation("Return Option<T>::None using the checked Option enum identity. T need not implement Default.\n\nSee `core::default::Default` for the construction contract.");
        module.trait_implementation_with(1, |arguments| {
            Ok(TraitImplementation {
                receiver: carrier.apply(arguments.to_vec())?,
                interface: default.apply([])?,
                bounds: vec![],
                bindings: vec![],
            }
            .with_overrides([entry]))
        })?;
    }
    Ok(())
}
