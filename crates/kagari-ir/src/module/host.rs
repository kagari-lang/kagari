//! Nominal host dependencies include signatures and layouts, even without a call.
use super::{EnumLayout, PublicAbiItem, StructLayout, abi::AbiType};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::DefinitionId,
};
use std::collections::BTreeSet;

/// Check host method tables against the defining module's executable trait
/// contracts, including declarations absent from its public ABI.
pub(crate) fn trait_bindings_match(
    interface: &kagari_common::host_interface::HostInterface,
    module: &kagari_common::identity::ModuleIdentity,
    items: &[PublicAbiItem],
    contracts: &[super::TraitContract],
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    use kagari_common::identity::DefinitionKind;
    for host in &interface.types {
        cancel.check()?;
        for implementation in &host.trait_implementations {
            cancel.check()?;
            let id = &implementation.trait_id;
            if &id.module != module {
                continue;
            }
            let Some(name) = id.path.last().map(|part| &part.name) else {
                return Ok(false);
            };
            let public = items.iter().find_map(|item| match item {
                PublicAbiItem::Trait(trait_abi) if &trait_abi.name == name => Some(trait_abi),
                _ => None,
            });
            let private = contracts.iter().find(|contract| &contract.abi.name == name);
            let Some(trait_abi) = public.or_else(|| private.map(|contract| &contract.abi)) else {
                return Ok(false);
            };
            if id.path.len() != 1
                || id.path[0].kind != DefinitionKind::Trait
                || id.path[0].occurrence != 0
                || private.is_some_and(|contract| contract.declaration != *id)
                || !host_trait_matches(implementation, host, trait_abi, cancel)?
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn host_trait_matches(
    implementation: &kagari_common::host_interface::HostTraitImplementationDeclaration,
    host: &kagari_common::host_interface::HostTypeDeclaration,
    trait_abi: &super::TraitAbi,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let args = implementation
        .trait_arguments
        .iter()
        .map(AbiType::from_host_type)
        .collect::<Vec<_>>();
    if args.len() != trait_abi.generic_params.len()
        || implementation.methods.len() != trait_abi.methods.len()
    {
        return Ok(false);
    }
    for method in &trait_abi.methods {
        cancel.check()?;
        if !method.generic_params.is_empty() || method.params.is_empty() {
            return Ok(false);
        }
        let Some(binding) = implementation.methods.iter().find(|binding| {
            binding.trait_method.path.last().is_some_and(|part| {
                part.name == method.name
                    && part.kind == kagari_common::identity::DefinitionKind::Method
                    && part.occurrence == 0
            })
        }) else {
            return Ok(false);
        };
        let Some(host_method) = host
            .methods
            .iter()
            .find(|candidate| candidate.id == binding.host_method)
        else {
            return Ok(false);
        };
        let receiver = AbiType::Host(host.id.clone());
        if method.params.len() != host_method.params.len() + 1
            || !matches_host_type(
                &method.params[0].ty,
                &receiver,
                &implementation.trait_id,
                &args,
                &receiver,
                cancel,
            )?
        {
            return Ok(false);
        }
        for (expected, actual) in method.params[1..].iter().zip(&host_method.params) {
            cancel.check()?;
            if !matches_host_type(
                &expected.ty,
                &AbiType::from_host_type(&actual.ty),
                &implementation.trait_id,
                &args,
                &receiver,
                cancel,
            )? {
                return Ok(false);
            }
        }
        if !matches_host_type(
            &method.return_type,
            &AbiType::from_host_type(&host_method.return_type),
            &implementation.trait_id,
            &args,
            &receiver,
            cancel,
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_host_type(
    expected: &AbiType,
    actual: &AbiType,
    owner: &DefinitionId,
    arguments: &[AbiType],
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<bool, Cancelled> {
    let mut pending = vec![(expected, actual)];
    while let Some((expected, actual)) = pending.pop() {
        cancel.check()?;
        match (expected, actual) {
            (AbiType::SelfType(id), actual) if id == owner => pending.push((receiver, actual)),
            (
                AbiType::Parameter {
                    owner: id,
                    position,
                },
                actual,
            ) if id == owner => {
                let Some(argument) = arguments.get(*position) else {
                    return Ok(false);
                };
                pending.push((argument, actual));
            }
            (AbiType::Builtin(a), AbiType::Builtin(b)) if a == b => {}
            (AbiType::Host(a), AbiType::Host(b)) if a == b => {}
            (AbiType::Tuple(a), AbiType::Tuple(b)) if a.len() == b.len() => {
                pending.extend(a.iter().zip(b));
            }
            (AbiType::Array(a), AbiType::Array(b)) | (AbiType::Set(a), AbiType::Set(b)) => {
                pending.push((a, b));
            }
            (AbiType::Map { key: ak, value: av }, AbiType::Map { key: bk, value: bv }) => {
                pending.extend([(ak.as_ref(), bk.as_ref()), (av.as_ref(), bv.as_ref())]);
            }
            (
                AbiType::StandardEnum { kind: ak, args: aa },
                AbiType::StandardEnum { kind: bk, args: ba },
            ) if ak == bk && aa.len() == ba.len() => pending.extend(aa.iter().zip(ba)),
            _ => return Ok(false),
        }
    }
    Ok(true)
}

pub(crate) fn references(
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<BTreeSet<DefinitionId>, Cancelled> {
    let mut pending = Vec::new();
    for item in items {
        cancel.check()?;
        let functions: &[super::FunctionAbi] = match item {
            PublicAbiItem::Function(function) => std::slice::from_ref(function),
            PublicAbiItem::Const(value) => {
                pending.push(&value.ty);
                &[]
            }
            PublicAbiItem::Type(ty) => {
                pending.extend(ty.fields.iter().map(|field| &field.ty));
                pending.extend(ty.variants.iter().flat_map(|variant| &variant.payload));
                &[]
            }
            PublicAbiItem::Trait(ty) => &ty.methods,
            PublicAbiItem::InterfaceTable(table) => {
                pending.extend([&table.trait_type, &table.for_type]);
                &table.methods
            }
        };
        for function in functions {
            pending.extend(function.params.iter().map(|param| &param.ty));
            pending.push(&function.return_type);
        }
    }
    for layout in structures {
        pending.extend(&layout.arguments);
        pending.extend(layout.fields.iter().map(|field| &field.ty));
    }
    for layout in enums {
        pending.extend(&layout.arguments);
        pending.extend(layout.variants.iter().flat_map(|variant| &variant.payload));
    }
    let mut result = BTreeSet::new();
    while let Some(ty) = pending.pop() {
        cancel.check()?;
        match ty {
            AbiType::Host(id) => {
                result.insert(id.clone());
            }
            AbiType::Tuple(types) | AbiType::StandardEnum { args: types, .. } => {
                pending.extend(types)
            }
            AbiType::Array(ty) | AbiType::Set(ty) => pending.push(ty),
            AbiType::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::Struct(ty) | AbiType::Enum(ty) | AbiType::Trait(ty) => {
                pending.extend(&ty.arguments)
            }
            _ => {}
        }
    }
    Ok(result)
}

pub(crate) fn validate(
    interface: &kagari_common::host_interface::HostInterface,
    items: &[PublicAbiItem],
    structures: &[StructLayout],
    enums: &[EnumLayout],
    cancel: &CancellationToken,
) -> Result<(), super::layout::LayoutValidationError> {
    use super::layout::LayoutValidationError as Error;
    cancel.check().map_err(|_| Error::Cancelled)?;
    interface.validate().map_err(|_| Error::Invalid)?;
    let ids: BTreeSet<_> = interface.types.iter().map(|ty| &ty.id).collect();
    if references(items, structures, enums, cancel)
        .map_err(|_| Error::Cancelled)?
        .iter()
        .all(|id| ids.contains(id))
    {
        Ok(())
    } else {
        Err(Error::Invalid)
    }
}
