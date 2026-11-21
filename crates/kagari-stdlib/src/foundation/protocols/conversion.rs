//! Native parsing and iterator aggregation contracts.
use crate::foundation::{bound, enumeration, interface, protocols::Contract};
use kagari_runtime::native::{
    assembly::draft::ModuleDraft, binding::NativeResult, declarations::MethodDecl, types::Type,
};
use kagari_types::{scalar::BuiltinType, ty::GenericParam};

pub(super) fn from_str(module: &mut ModuleDraft, contract: &mut Contract) -> NativeResult<()> {
    let error = contract.associated(
        module,
        "Err",
        vec![],
        "The error returned when this parse fails.",
    )?;
    let result = enumeration(module, "Result", 2)?
        .apply([contract.owner.receiver()?, contract.projection(&error)?])?;
    contract.method(
        module,
        MethodDecl::static_method("from_str")
            .parameter("text", Type::scalar(BuiltinType::String))
            .returns(result),
        contract.overview.0,
    )
}

pub(super) fn aggregation(
    module: &mut ModuleDraft<'_>,
    contract: &mut Contract,
) -> NativeResult<()> {
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
