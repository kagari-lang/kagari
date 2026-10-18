//! Collection interfaces declare capabilities without choosing storage or algorithms.
use crate::foundation::{enumeration, interface, protocols::Contract};
use kagari_runtime::native::{
    assembly::draft::ModuleDraft, binding::NativeResult, declarations::MethodDecl, types::Type,
};

pub(super) fn configure(module: &mut ModuleDraft<'_>, contract: &mut Contract) -> NativeResult<()> {
    let key = contract.owner.parameter(0)?;
    let option = enumeration(module, "Option", 1)?;
    let name = contract.name;
    let value = if contract.arity == 2 {
        contract.owner.parameter(1)?
    } else {
        key.clone()
    };
    match name {
        "List" | "Map" | "Set" => {
            let iterable = interface(module, "Iterable", 0)?;
            let item = module.reference_associated_type(&iterable, "Item", 0)?;
            let element = if name == "Map" {
                Type::tuple([key.clone(), value.clone()])?
            } else {
                key.clone()
            };
            if name == "List" {
                let index = interface(module, "Index", 1)?;
                let output = module.reference_associated_type(&index, "Output", 0)?;
                contract.parent(output.bind(index.apply([Type::u64()])?, key.clone())?);
            }
            contract.parent(item.bind(iterable.apply([])?, element)?);
            contract.method(module, MethodDecl::instance("len").returns(Type::u64()))?;
            contract.method(
                module,
                MethodDecl::instance("is_empty").returns(Type::bool()),
            )?;
            match name {
                "List" => contract.method(
                    module,
                    MethodDecl::instance("get")
                        .parameter("arg1", Type::u64())
                        .returns(option.apply([key])?),
                )?,
                "Set" => contract.method(
                    module,
                    MethodDecl::instance("contains")
                        .parameter("arg1", key)
                        .returns(Type::bool()),
                )?,
                _ => {
                    contract.method(
                        module,
                        MethodDecl::instance("contains_key")
                            .parameter("arg1", key.clone())
                            .returns(Type::bool()),
                    )?;
                    contract.method(
                        module,
                        MethodDecl::instance("get")
                            .parameter("arg1", key)
                            .returns(option.apply([value])?),
                    )?;
                }
            }
        }
        "MutableList" => {
            contract.parent(interface(module, "List", 1)?.apply([key.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("push").parameter("arg1", key.clone()),
            )?;
            contract.method(
                module,
                MethodDecl::instance("pop").returns(option.apply([key.clone()])?),
            )?;
            contract.method(
                module,
                MethodDecl::instance("insert")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", key.clone()),
            )?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", Type::u64())
                    .returns(option.apply([key.clone()])?),
            )?;
            contract.method(module, MethodDecl::instance("clear"))?;
            contract.method(
                module,
                MethodDecl::instance("set")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", key),
            )?;
        }
        "MutableMap" => {
            contract.parent(interface(module, "Map", 2)?.apply([key.clone(), value.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("insert")
                    .parameter("arg1", key.clone())
                    .parameter("arg2", value.clone()),
            )?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", key)
                    .returns(option.apply([value])?),
            )?;
            contract.method(module, MethodDecl::instance("clear"))?;
        }
        "MutableSet" => {
            contract.parent(interface(module, "Set", 1)?.apply([key.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("insert").parameter("arg1", key.clone()),
            )?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", key)
                    .returns(Type::bool()),
            )?;
            contract.method(module, MethodDecl::instance("clear"))?;
        }
        _ => unreachable!("collection capability"),
    }
    Ok(())
}
