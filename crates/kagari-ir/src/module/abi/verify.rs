//! Validate serialized semantic types independently of display strings.
use super::*;
use crate::module::layout::LayoutValidationError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use std::collections::HashSet;

type Parameters = HashSet<(DefinitionId, usize)>;

pub(crate) fn validate(
    items: &[PublicAbiItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let invalid = || LayoutValidationError::Invalid;
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let valid = match item {
            PublicAbiItem::Function(function) => {
                function.generic_params.is_empty()
                    && function.bounds.is_empty()
                    && function_valid(function, module, &[], &Parameters::new(), None, cancel)
            }
            PublicAbiItem::Const(value) => type_valid(&value.ty, &Parameters::new(), None, cancel),
            PublicAbiItem::Type(ty) => {
                let kind = match ty.kind {
                    TypeAbiKind::Struct => DefinitionKind::Struct,
                    TypeAbiKind::Enum => DefinitionKind::Enum,
                };
                let owner = owner(module, &[], kind, &ty.name);
                parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
                    bounds_valid(&ty.bounds, &params)
                        && ty
                            .fields
                            .iter()
                            .all(|field| type_valid(&field.ty, &params, None, cancel))
                        && ty
                            .variants
                            .iter()
                            .flat_map(|variant| &variant.payload)
                            .all(|ty| type_valid(ty, &params, None, cancel))
                })
            }
            PublicAbiItem::Trait(ty) => {
                let owner = owner(module, &[], DefinitionKind::Trait, &ty.name);
                parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
                    bounds_valid(&ty.bounds, &params)
                        && ty.methods.iter().all(|method| {
                            function_valid(
                                method,
                                module,
                                &owner.path,
                                &params,
                                Some(&owner),
                                cancel,
                            )
                        })
                })
            }
            PublicAbiItem::InterfaceTable(table) => {
                let template_owner = table.generic_params.first().map(|param| &param.owner);
                let params = match template_owner {
                    Some(owner)
                        if owner.module == *module
                            && owner.path.len() == 1
                            && owner.path[0].kind == DefinitionKind::Impl =>
                    {
                        parameters(&table.generic_params, owner, &Parameters::new())
                    }
                    Some(_) => None,
                    None => Some(Parameters::new()),
                };
                params.is_some_and(|params| {
                    bounds_valid(&table.bounds, &params)
                        && matches!(table.trait_type, AbiType::Trait(_))
                        && type_valid(&table.trait_type, &params, None, cancel)
                        && type_valid(&table.for_type, &params, None, cancel)
                        && table.methods.iter().all(|method| {
                            // Concrete impls need no template owner; method binders carry
                            // their declaring impl path even when no outer parameter exists.
                            let parent =
                                template_owner.map(|id| id.path.as_slice()).or_else(|| {
                                    method.generic_params.first().map(|param| {
                                        &param.owner.path
                                            [..param.owner.path.len().saturating_sub(1)]
                                    })
                                });
                            if let Some(parent) = parent {
                                parent.len() == 1
                                    && parent[0].kind == DefinitionKind::Impl
                                    && function_valid(method, module, parent, &params, None, cancel)
                            } else {
                                method.generic_params.is_empty()
                                    && bounds_valid(&method.bounds, &params)
                                    && signature_valid(method, &params, None, cancel)
                            }
                        })
                })
            }
        };
        if !valid {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(invalid());
        }
    }
    Ok(())
}

fn owner(
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    kind: DefinitionKind,
    name: &str,
) -> DefinitionId {
    let mut path = parent.to_vec();
    path.push(DefinitionPathSegment {
        kind,
        name: name.into(),
        occurrence: 0,
    });
    DefinitionId {
        module: module.clone(),
        path,
    }
}
fn parameters(
    declared: &[GenericParameterAbi],
    owner: &DefinitionId,
    outer: &Parameters,
) -> Option<Parameters> {
    let mut params = outer.clone();
    for (position, param) in declared.iter().enumerate() {
        if &param.owner != owner
            || param.position != position
            || !params.insert((owner.clone(), position))
        {
            return None;
        }
    }
    Some(params)
}
fn bounds_valid(bounds: &[GenericBoundAbi], params: &Parameters) -> bool {
    if !bounds
        .windows(2)
        .all(|pair| (&pair[0].owner, pair[0].position) < (&pair[1].owner, pair[1].position))
    {
        return false;
    }
    let mut seen = HashSet::new();
    bounds.iter().all(|bound| {
        params.contains(&(bound.owner.clone(), bound.position))
            && seen.insert((&bound.owner, bound.position))
            && !bound.constraints.is_empty()
            && bound.constraints.windows(2).all(|pair| pair[0] < pair[1])
            && bound.constraints.iter().all(|constraint| match constraint {
                ConstraintAbi::Standard(_) => true,
                ConstraintAbi::Trait(id) => nominal_valid(id, DefinitionKind::Trait),
            })
    })
}

fn function_valid(
    function: &FunctionAbi,
    module: &ModuleIdentity,
    parent: &[DefinitionPathSegment],
    outer: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    let kind = if parent.is_empty() {
        DefinitionKind::Function
    } else {
        DefinitionKind::Method
    };
    let owner = owner(module, parent, kind, &function.name);
    parameters(&function.generic_params, &owner, outer).is_some_and(|params| {
        bounds_valid(&function.bounds, &params)
            && signature_valid(function, &params, self_owner, cancel)
    })
}
fn signature_valid(
    function: &FunctionAbi,
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    function
        .params
        .iter()
        .map(|param| &param.ty)
        .chain(std::iter::once(&function.return_type))
        .all(|ty| type_valid(ty, params, self_owner, cancel))
}
fn nominal_valid(id: &DefinitionId, kind: DefinitionKind) -> bool {
    !id.module.package.0.is_empty()
        && !id.module.path.is_empty()
        && !id.module.path.iter().any(String::is_empty)
        && id
            .path
            .last()
            .is_some_and(|part| part.kind == kind && !part.name.is_empty())
}
fn type_valid(
    ty: &AbiType,
    params: &Parameters,
    self_owner: Option<&DefinitionId>,
    cancel: &CancellationToken,
) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() {
            return false;
        }
        match ty {
            AbiType::Parameter { owner, position } => {
                if !params.contains(&(owner.clone(), *position)) {
                    return false;
                }
            }
            AbiType::SelfType(owner) => {
                if Some(owner) != self_owner {
                    return false;
                }
            }
            AbiType::Builtin(_) => {}
            AbiType::Host(id) => {
                if kagari_common::host_interface::validate_host_type_identity(id).is_err() {
                    return false;
                }
            }
            AbiType::Tuple(types) => pending.extend(types),
            AbiType::Array(ty) | AbiType::Set(ty) => pending.push(ty),
            AbiType::Map { key, value } => pending.extend([key.as_ref(), value.as_ref()]),
            AbiType::StandardEnum { kind, args } => {
                let count = match kind {
                    StandardEnumKind::Option => 1,
                    StandardEnumKind::Result => 2,
                };
                if args.len() != count {
                    return false;
                }
                pending.extend(args);
            }
            AbiType::Struct(ty) | AbiType::Enum(ty) | AbiType::Trait(ty) => {
                pending.extend(&ty.arguments);
            }
        }
        let nominal = match ty {
            AbiType::Struct(n) => Some((n, DefinitionKind::Struct)),
            AbiType::Enum(n) => Some((n, DefinitionKind::Enum)),
            AbiType::Trait(n) => Some((n, DefinitionKind::Trait)),
            _ => None,
        };
        if let Some((nominal, kind)) = nominal
            && !nominal_valid(&nominal.declaration, kind)
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{BytecodeVerificationError, verify_module};

    #[test]
    fn public_signatures_reject_foreign_parameters_invalid_arity_and_escaped_self() {
        let original = crate::tests::common::bytecode_ok(
            "pub fn plain() -> i32 { 1 } pub trait Identity { fn same<T: HashKey + Comparable>(self, value: T) -> T; }",
        );
        for corruption in 0..8 {
            let mut module = original.clone();
            let (functions, traits) = module.public_items.split_at_mut(1);
            let PublicAbiItem::Function(function) = &mut functions[0] else {
                panic!("public function")
            };
            let PublicAbiItem::Trait(interface) = &mut traits[0] else {
                panic!("public trait")
            };
            match corruption {
                0 => {
                    interface.methods[0].generic_params[0]
                        .owner
                        .module
                        .package
                        .0 = "foreign".into()
                }
                1 => {
                    if let AbiType::Parameter { position, .. } =
                        &mut interface.methods[0].params[1].ty
                    {
                        *position = 99;
                    }
                }
                2 => {
                    function.return_type = AbiType::SelfType(owner(
                        &module.identity,
                        &[],
                        DefinitionKind::Trait,
                        "Identity",
                    ))
                }
                3 => {
                    function.return_type = AbiType::StandardEnum {
                        kind: StandardEnumKind::Result,
                        args: vec![AbiType::Builtin(BuiltinType::I32)],
                    }
                }
                4 => {
                    function.return_type = AbiType::Struct(NominalAbiType {
                        declaration: owner(
                            &module.identity,
                            &[],
                            DefinitionKind::Trait,
                            "Identity",
                        ),
                        arguments: vec![],
                    })
                }
                5 => function.generic_params.push(GenericParameterAbi {
                    owner: owner(&module.identity, &[], DefinitionKind::Function, "plain"),
                    position: 0,
                }),
                6 => interface.methods[0].bounds[0].constraints.reverse(),
                _ => {
                    let constraint = interface.methods[0].bounds[0].constraints[0].clone();
                    interface.methods[0].bounds[0]
                        .constraints
                        .insert(0, constraint);
                }
            }
            assert!(
                matches!(
                    verify_module(&module),
                    Err(BytecodeVerificationError::InvalidPublicAbi)
                ),
                "corruption {corruption}"
            );
        }
        let mut module = original;
        let PublicAbiItem::Trait(interface) = &mut module.public_items[1] else {
            panic!("public trait")
        };
        interface.methods[0].return_type =
            AbiType::SelfType(owner(&module.identity, &[], DefinitionKind::Trait, "Other"));
        assert!(matches!(
            verify_module(&module),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
    }
}
