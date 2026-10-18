//! Conversion authority and construction requirements remain explicit trait metadata.
use crate::foundation::{bound, enumeration, interface, protocols::Contract};
use kagari_runtime::native::{
    assembly::draft::ModuleDraft, binding::NativeResult, declarations::MethodDecl, types::Type,
};
use kagari_types::{
    declaration::conversion::ConversionAdapter, scalar::BuiltinType, ty::GenericParam,
};

pub(super) fn configure(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let receiver = contract.owner.receiver()?;
    match contract.name {
        "Default" => contract.method(
            module,
            MethodDecl::static_method("default").returns(receiver),
        ),
        "From" => contract.method(
            module,
            MethodDecl::static_method("from")
                .parameter("value", contract.owner.parameter(0)?)
                .returns(receiver),
        ),
        "FromIterator" | "Sum" | "Product" => aggregation(module, contract),
        _ => conversion(module, contract),
    }
}

fn conversion(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let (name, instance, error_name) = match contract.name {
        "Into" => ("into", true, None),
        "TryFrom" => ("try_from", false, Some("Error")),
        "TryInto" => ("try_into", true, Some("Error")),
        "FromStr" => ("from_str", false, Some("Err")),
        _ => unreachable!("conversion trait"),
    };
    let input = if contract.arity == 0 {
        Type::scalar(BuiltinType::String)
    } else {
        contract.owner.parameter(0)?
    };
    let result = if instance {
        input.clone()
    } else {
        contract.owner.receiver()?
    };
    let error = error_name
        .map(|name| contract.associated(module, name, vec![]))
        .transpose()?;
    let result_enum = enumeration(module, "Result", 2)?;
    let result = if let Some(error) = &error {
        result_enum.apply([result, contract.projection(error)?])?
    } else {
        result
    };
    let declaration = if instance {
        MethodDecl::instance(name)
    } else {
        MethodDecl::static_method(name)
            .parameter(if contract.arity == 0 { "text" } else { "value" }, input)
    };
    contract.method(module, declaration.returns(result))?;
    contract.conversion = match contract.name {
        "Into" | "TryInto" => {
            let fallible = contract.name == "TryInto";
            let forward = interface(module, if fallible { "TryFrom" } else { "From" }, 1)?;
            let method =
                module.reference_method(&forward, if fallible { "try_from" } else { "from" }, 0)?;
            let error = if fallible {
                Some((
                    error.expect("reverse error").id(),
                    module.reference_associated_type(&forward, "Error", 0)?.id(),
                ))
            } else {
                None
            };
            Some(ConversionAdapter::Reverse {
                origin: forward.id(),
                method: method.id(),
                error,
                result: fallible.then(|| result_enum.id()),
            })
        }
        "TryFrom" => Some(ConversionAdapter::Forward {
            method: module
                .reference_method(&contract.owner, "try_from", 0)?
                .id(),
            error: error.expect("forward error").id(),
            result: result_enum.id(),
        }),
        _ => None,
    };
    Ok(())
}

fn aggregation(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let name = match contract.name {
        "FromIterator" => "from_iter",
        "Sum" => "sum",
        "Product" => "product",
        _ => unreachable!("aggregation trait"),
    };
    let member = module.reference_method(&contract.owner, name, 1)?;
    let source = member.parameter(0)?;
    let iterable = interface(module, "Iterable", 0)?;
    let item = module.reference_associated_type(&iterable, "Item", 0)?;
    let applied = item.bind(iterable.apply([])?, contract.owner.parameter(0)?)?;
    let mut method = module.trait_method(
        contract.owner.id(),
        MethodDecl::static_method(name)
            .parameter("source", source.clone())
            .returns(contract.owner.receiver()?),
    )?;
    method.generic_params.push(GenericParam {
        owner: member.id(),
        position: 0,
    });
    method.bounds.push(bound(&source, applied));
    contract.methods.push(method);
    Ok(())
}
