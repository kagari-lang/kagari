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
            contract.method(
                module,
                MethodDecl::instance("len").returns(Type::u64()),
                "Return the number of entries.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("is_empty").returns(Type::bool()),
                "Return whether the collection has no entries.",
            )?;
            match name {
                "List" => contract.method(
                    module,
                    MethodDecl::instance("get")
                        .parameter("arg1", Type::u64())
                        .returns(option.apply([key])?),
                    "Return the requested value, or None if the index or key is absent.",
                )?,
                "Set" => contract.method(
                    module,
                    MethodDecl::instance("contains")
                        .parameter("arg1", key)
                        .returns(Type::bool()),
                    "Test membership without changing collection storage.",
                )?,
                _ => {
                    contract.method(
                        module,
                        MethodDecl::instance("contains_key")
                            .parameter("arg1", key.clone())
                            .returns(Type::bool()),
                        "Test membership without changing collection storage.",
                    )?;
                    contract.method(
                        module,
                        MethodDecl::instance("get")
                            .parameter("arg1", key)
                            .returns(option.apply([value])?),
                        "Return the requested value, or None if the index or key is absent.",
                    )?;
                }
            }
        }
        "MutableList" => {
            contract.parent(interface(module, "List", 1)?.apply([key.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("push").parameter("arg1", key.clone()),
                "Append one item. A guarded structural edit traps before committing the edit.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("pop").returns(option.apply([key.clone()])?),
                "Remove and return the last item, or None for an empty sequence.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("insert")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", key.clone()),
"Insert at the given position and shift later elements. An index beyond the length traps.",
)?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", Type::u64())
                    .returns(option.apply([key.clone()])?),
                "Remove and return the entry, or None if it is absent.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("clear"),
                "Remove all entries after validating structural mutation guards.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("set")
                    .parameter("arg1", Type::u64())
                    .parameter("arg2", key),
                "Replace the item at an existing index. An out-of-bounds index traps.",
            )?;
        }
        "MutableMap" => {
            contract.parent(interface(module, "Map", 2)?.apply([key.clone(), value.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("insert")
                    .parameter("arg1", key.clone())
                    .parameter("arg2", value.clone()),
                "Insert a key/value entry, replacing the value of an equal key.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", key)
                    .returns(option.apply([value])?),
                "Remove and return the entry, or None if it is absent.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("clear"),
                "Remove all entries after validating structural mutation guards.",
            )?;
        }
        "MutableSet" => {
            contract.parent(interface(module, "Set", 1)?.apply([key.clone()])?);
            contract.method(
                module,
                MethodDecl::instance("insert").parameter("arg1", key.clone()),
                "Insert a member, preserving the uniqueness of equal members.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("remove")
                    .parameter("arg1", key)
                    .returns(Type::bool()),
                "Remove a member and report whether it was present.",
            )?;
            contract.method(
                module,
                MethodDecl::instance("clear"),
                "Remove all entries after validating structural mutation guards.",
            )?;
        }
        _ => unreachable!("collection capability"),
    }
    Ok(())
}
