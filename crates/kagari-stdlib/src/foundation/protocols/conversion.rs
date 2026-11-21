//! Conversion contracts and the library's conditional Script forwarding implementations.
use crate::foundation::{bound, enumeration, interface, protocols::Contract};
use kagari_runtime::native::{
    assembly::{
        draft::{ModuleDraft, TraitImplementation},
        references::{DraftMember, DraftReference},
    },
    binding::NativeResult,
    declarations::MethodDecl,
    types::Type,
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
"Return the value chosen by the explicit implementation. This required static method has no receiver or arguments and no fallback body. Its result is the implementing Self type.",
),
        "From" => contract.method(
            module,
            MethodDecl::static_method("from")
                .parameter("value", contract.owner.parameter(0)?)
                .returns(receiver),
contract.overview.0,
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
        .map(|name| {
            contract.associated(
                module,
                name,
                vec![],
                "The error returned when this conversion or parse fails.",
            )
        })
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
    contract.method(module, declaration.returns(result), contract.overview.0)?;
    if instance {
        let reverse_method = module.reference_method(&contract.owner, name, 0)?;
        forwarding(module, &contract.owner, reverse_method, error)?;
    } else if contract.name == "TryFrom" {
        contract.conversion = Some(ConversionAdapter {
            method: module.reference_method(&contract.owner, name, 0)?.id(),
            error: error.expect("TryFrom declares Error").id(),
            result: result_enum.id(),
        });
    }
    Ok(())
}

fn forwarding(
    module: &mut ModuleDraft<'_>,
    reverse: &DraftReference,
    reverse_method: DraftMember,
    reverse_error: Option<DraftMember>,
) -> NativeResult<()> {
    let (trait_name, method_name) = if reverse_error.is_some() {
        ("TryFrom", "try_from")
    } else {
        ("From", "from")
    };
    let forward = interface(module, trait_name, 1)?;
    let forward_method = module.reference_method(&forward, method_name, 0)?;
    let errors = reverse_error
        .map(|reverse| {
            module
                .reference_associated_type(&forward, "Error", 0)
                .map(|forward| (reverse, forward))
        })
        .transpose()?;
    module.trait_implementation_with(2, |types| {
        let source = types[0].clone();
        let destination = types[1].clone();
        let from = forward.apply([source.clone()])?;
        let into = reverse.apply([destination.clone()])?;
        let interface = if let Some((reverse_error, forward_error)) = errors {
            reverse_error.bind(
                into,
                forward_error.projection(destination.clone(), from.clone(), [])?,
            )?
        } else {
            into
        };
        Ok(TraitImplementation {
            receiver: source,
            interface,
            bounds: vec![bound(&destination, from.clone())],
            bindings: vec![],
        }
        .with_script_method(reverse_method, move |body| {
            body.trait_call(&forward_method, destination, from, [], [body.parameter(0)?])
        }))
    })
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
    contract.submit(module, method, contract.overview.0)
}
