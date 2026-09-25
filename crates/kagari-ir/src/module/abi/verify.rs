//! Validate serialized semantic types independently of display strings.
use super::*;
use crate::module::layout::LayoutValidationError;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity},
};
use std::collections::HashSet;

type Parameters = HashSet<(DefinitionId, usize)>;

pub(crate) fn concrete_type_valid(ty: &AbiType, cancel: &CancellationToken) -> bool {
    type_valid(ty, &Parameters::new(), None, cancel)
}

pub(crate) fn validate(
    items: &[PublicAbiItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let invalid = || LayoutValidationError::Invalid;
    let mut aggregate_names = HashSet::new();
    let mut trait_names = HashSet::new();
    let mut interface_identities = HashSet::new();
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
                aggregate_names.insert(&ty.name)
                    && aggregate_shape_valid(ty, cancel)
                    && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(
                        |params| {
                            bounds_valid(&ty.bounds, &params, cancel)
                                && ty
                                    .fields
                                    .iter()
                                    .all(|field| type_valid(&field.ty, &params, None, cancel))
                                && ty
                                    .variants
                                    .iter()
                                    .flat_map(|variant| &variant.payload)
                                    .all(|ty| type_valid(ty, &params, None, cancel))
                        },
                    )
            }
            PublicAbiItem::Trait(ty) => {
                trait_names.insert(&ty.name) && trait_valid(ty, module, cancel)
            }
            PublicAbiItem::InterfaceTable(table) => {
                let owner = &table.declaration;
                let params = (interface_identities.insert(owner)
                    && owner.module == *module
                    && owner.path.len() == 1
                    && owner.path[0].kind == DefinitionKind::Impl
                    && owner.path[0].name.is_empty()
                    && owner.within_path_limit())
                .then(|| parameters(&table.generic_params, owner, &Parameters::new()))
                .flatten();
                params.is_some_and(|params| {
                    let mut methods = HashSet::new();
                    bounds_valid(&table.bounds, &params, cancel)
                        && matches!(table.trait_type, AbiType::Trait(_))
                        && type_valid(&table.trait_type, &params, None, cancel)
                        && type_valid(&table.for_type, &params, None, cancel)
                        && table.methods.iter().all(|method| {
                            methods.insert(&method.name)
                                && function_valid(
                                    method,
                                    module,
                                    &owner.path,
                                    &params,
                                    None,
                                    cancel,
                                )
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
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(instance) = &table.trait_type else {
            return Err(invalid());
        };
        if instance.declaration.module != *module {
            continue;
        }
        if instance.declaration.path.len() != 1
            || instance.declaration.path[0].kind != DefinitionKind::Trait
            || instance.declaration.path[0].occurrence != 0
        {
            return Err(invalid());
        }
        let Some(trait_name) = instance.declaration.path.last().map(|part| &part.name) else {
            return Err(invalid());
        };
        let Some(PublicAbiItem::Trait(interface)) = items
            .iter()
            .find(|item| matches!(item, PublicAbiItem::Trait(ty) if &ty.name == trait_name))
        else {
            // Private traits are absent from the public ABI table.
            continue;
        };
        if !interface_contract_matches(table, interface, cancel) {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(invalid());
        }
    }
    Ok(())
}

pub(crate) fn validate_trait_contracts(
    contracts: &[TraitContract],
    items: &[PublicAbiItem],
    module: &ModuleIdentity,
    cancel: &CancellationToken,
) -> Result<(), LayoutValidationError> {
    let mut names = HashSet::new();
    for contract in contracts {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let id = &contract.declaration;
        if id.module != *module
            || id.path.len() != 1
            || id.path[0].kind != DefinitionKind::Trait
            || id.path[0].occurrence != 0
            || id.path[0].name != contract.abi.name
            || !id.within_path_limit()
            || !names.insert(&contract.abi.name)
            || !trait_valid(&contract.abi, module, cancel)
        {
            return Err(LayoutValidationError::Invalid);
        }
    }
    if items
        .iter()
        .any(|item| matches!(item, PublicAbiItem::Trait(public) if names.contains(&public.name)))
    {
        return Err(LayoutValidationError::Invalid);
    }
    for item in items {
        cancel
            .check()
            .map_err(|_| LayoutValidationError::Cancelled)?;
        let PublicAbiItem::InterfaceTable(table) = item else {
            continue;
        };
        let AbiType::Trait(instance) = &table.trait_type else {
            return Err(LayoutValidationError::Invalid);
        };
        if instance.declaration.module != *module
            || items.iter().any(|item| {
                matches!(item, PublicAbiItem::Trait(public) if instance.declaration.path.last().is_some_and(|part| part.name == public.name))
            })
        {
            continue;
        }
        let Some(contract) = contracts
            .iter()
            .find(|contract| contract.declaration == instance.declaration)
        else {
            return Err(LayoutValidationError::Invalid);
        };
        if !interface_contract_matches(table, &contract.abi, cancel) {
            cancel
                .check()
                .map_err(|_| LayoutValidationError::Cancelled)?;
            return Err(LayoutValidationError::Invalid);
        }
    }
    Ok(())
}

fn trait_valid(ty: &TraitAbi, module: &ModuleIdentity, cancel: &CancellationToken) -> bool {
    let owner = owner(module, &[], DefinitionKind::Trait, &ty.name);
    let mut methods = HashSet::new();
    !ty.name.is_empty()
        && parameters(&ty.generic_params, &owner, &Parameters::new()).is_some_and(|params| {
            bounds_valid(&ty.bounds, &params, cancel)
                && ty.methods.iter().all(|method| {
                    methods.insert(&method.name)
                        && function_valid(
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

pub(crate) fn interface_contract_matches(
    table: &InterfaceTableAbi,
    interface: &TraitAbi,
    cancel: &CancellationToken,
) -> bool {
    let AbiType::Trait(instance) = &table.trait_type else {
        return false;
    };
    let mut matched = HashSet::new();
    instance.arguments.len() == interface.generic_params.len()
        && table.methods.len() == interface.methods.len()
        && table.methods.iter().all(|method| {
            cancel.check().is_ok()
                && interface.methods.iter().any(|declared| {
                    declared.name == method.name
                        && same_method_contract(
                            declared,
                            method,
                            &instance.declaration,
                            &instance.arguments,
                            &table.declaration,
                            &table.for_type,
                            cancel,
                        )
                        && matched.insert(&declared.name)
                })
        })
}

fn same_method_contract(
    declared: &FunctionAbi,
    implemented: &FunctionAbi,
    trait_owner: &DefinitionId,
    trait_arguments: &[AbiType],
    impl_owner: &DefinitionId,
    for_type: &AbiType,
    cancel: &CancellationToken,
) -> bool {
    if declared.generic_params.len() != implemented.generic_params.len()
        || declared.params.len() != implemented.params.len()
        || declared.bounds.len() != implemented.bounds.len()
    {
        return false;
    }
    let trait_method = owner(
        &trait_owner.module,
        &trait_owner.path,
        DefinitionKind::Method,
        &declared.name,
    );
    let impl_method = owner(
        &impl_owner.module,
        &impl_owner.path,
        DefinitionKind::Method,
        &implemented.name,
    );
    let matches_type = |expected: &AbiType, actual: &AbiType| {
        let mut pending = vec![(expected, actual)];
        while let Some((expected, actual)) = pending.pop() {
            if cancel.check().is_err() {
                return false;
            }
            match (expected, actual) {
                (AbiType::SelfType(id), actual) if id == trait_owner => {
                    pending.push((for_type, actual));
                }
                (
                    AbiType::Parameter {
                        owner: id,
                        position,
                    },
                    actual,
                ) if id == trait_owner => {
                    let Some(argument) = trait_arguments.get(*position) else {
                        return false;
                    };
                    pending.push((argument, actual));
                }
                (
                    AbiType::Parameter {
                        owner: id,
                        position,
                    },
                    AbiType::Parameter {
                        owner: actual_owner,
                        position: actual_position,
                    },
                ) if id == &trait_method
                    && actual_owner == &impl_method
                    && position == actual_position => {}
                (
                    AbiType::Parameter {
                        owner: expected_owner,
                        position: expected_position,
                    },
                    AbiType::Parameter {
                        owner: actual_owner,
                        position: actual_position,
                    },
                ) if expected_owner == actual_owner && expected_position == actual_position => {}
                (AbiType::Builtin(a), AbiType::Builtin(b)) if a == b => {}
                (AbiType::Host(a), AbiType::Host(b)) if a == b => {}
                (AbiType::Tuple(a), AbiType::Tuple(b)) if a.len() == b.len() => {
                    pending.extend(a.iter().zip(b));
                }
                (AbiType::Array(a), AbiType::Array(b)) | (AbiType::Set(a), AbiType::Set(b)) => {
                    pending.push((a, b))
                }
                (AbiType::Map { key: ak, value: av }, AbiType::Map { key: bk, value: bv }) => {
                    pending.extend([(ak.as_ref(), bk.as_ref()), (av.as_ref(), bv.as_ref())])
                }
                (AbiType::Struct(a), AbiType::Struct(b))
                | (AbiType::Enum(a), AbiType::Enum(b))
                | (AbiType::Trait(a), AbiType::Trait(b))
                    if a.declaration == b.declaration && a.arguments.len() == b.arguments.len() =>
                {
                    pending.extend(a.arguments.iter().zip(&b.arguments));
                }
                (
                    AbiType::StandardEnum { kind: ak, args: aa },
                    AbiType::StandardEnum { kind: bk, args: ba },
                ) if ak == bk && aa.len() == ba.len() => pending.extend(aa.iter().zip(ba)),
                _ => return false,
            }
        }
        true
    };
    if !declared
        .bounds
        .iter()
        .zip(&implemented.bounds)
        .all(|(expected, actual)| {
            expected.position == actual.position
                && expected.constraints.len() == actual.constraints.len()
                && ((expected.owner == trait_method && actual.owner == impl_method)
                    || (expected.owner == *trait_owner && actual.owner == *impl_owner))
                && expected
                    .constraints
                    .iter()
                    .zip(&actual.constraints)
                    .all(|(expected, actual)| match (expected, actual) {
                        (ConstraintAbi::Standard(a), ConstraintAbi::Standard(b)) => a == b,
                        (ConstraintAbi::Trait(a), ConstraintAbi::Trait(b)) => {
                            matches_type(&AbiType::Trait(a.clone()), &AbiType::Trait(b.clone()))
                        }
                        _ => false,
                    })
        })
    {
        return false;
    }
    declared
        .params
        .iter()
        .zip(&implemented.params)
        .all(|(expected, actual)| {
            expected.mutable == actual.mutable && matches_type(&expected.ty, &actual.ty)
        })
        && matches_type(&declared.return_type, &implemented.return_type)
}

fn aggregate_shape_valid(ty: &TypeAbi, cancel: &CancellationToken) -> bool {
    if ty.name.is_empty()
        || match ty.kind {
            TypeAbiKind::Struct => !ty.variants.is_empty(),
            TypeAbiKind::Enum => !ty.fields.is_empty(),
        }
    {
        return false;
    }
    let mut names = HashSet::new();
    ty.fields
        .iter()
        .map(|field| &field.name)
        .chain(ty.variants.iter().map(|variant| &variant.name))
        .all(|name| cancel.check().is_ok() && !name.is_empty() && names.insert(name))
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
fn bounds_valid(
    bounds: &[GenericBoundAbi],
    params: &Parameters,
    cancel: &CancellationToken,
) -> bool {
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
                ConstraintAbi::Trait(ty) => {
                    type_valid(&AbiType::Trait(ty.clone()), params, None, cancel)
                }
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
        bounds_valid(&function.bounds, &params, cancel)
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
    fn interface_method_contract_substitutes_self_and_method_binders_inside_containers() {
        let module = ModuleIdentity::single_file("interface.kgr");
        let trait_owner = owner(&module, &[], DefinitionKind::Trait, "Read");
        let impl_owner = owner(&module, &[], DefinitionKind::Impl, "");
        let trait_method = owner(&module, &trait_owner.path, DefinitionKind::Method, "read");
        let impl_method = owner(&module, &impl_owner.path, DefinitionKind::Method, "read");
        let for_type = AbiType::Struct(NominalAbiType {
            declaration: owner(&module, &[], DefinitionKind::Struct, "Player"),
            arguments: Vec::new(),
        });
        let mut declared = FunctionAbi {
            name: "read".into(),
            generic_params: vec![GenericParameterAbi {
                owner: trait_method.clone(),
                position: 0,
            }],
            bounds: Vec::new(),
            params: vec![ParameterAbi {
                name: "input".into(),
                ty: AbiType::Array(Box::new(AbiType::Tuple(vec![
                    AbiType::SelfType(trait_owner.clone()),
                    AbiType::Parameter {
                        owner: trait_method.clone(),
                        position: 0,
                    },
                ]))),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::I32),
        };
        let mut implemented = FunctionAbi {
            name: "read".into(),
            generic_params: vec![GenericParameterAbi {
                owner: impl_method.clone(),
                position: 0,
            }],
            bounds: Vec::new(),
            params: vec![ParameterAbi {
                name: "renamed".into(),
                ty: AbiType::Array(Box::new(AbiType::Tuple(vec![
                    for_type.clone(),
                    AbiType::Parameter {
                        owner: impl_method.clone(),
                        position: 0,
                    },
                ]))),
                mutable: false,
            }],
            return_type: AbiType::Builtin(BuiltinType::I32),
        };
        let cancel = CancellationToken::default();
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
        let original_param = implemented.params[0].ty.clone();
        implemented.params[0].ty = AbiType::Array(Box::new(AbiType::Tuple(vec![
            for_type.clone(),
            AbiType::Builtin(BuiltinType::Bool),
        ])));
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
        implemented.params[0].ty = original_param;
        declared.bounds.push(GenericBoundAbi {
            owner: trait_method.clone(),
            position: 0,
            constraints: vec![ConstraintAbi::Standard(
                kagari_hir::builtin::surface::StandardTypeConstraint::HashKey,
            )],
        });
        implemented.bounds.push(GenericBoundAbi {
            owner: impl_method.clone(),
            position: 0,
            constraints: declared.bounds[0].constraints.clone(),
        });
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
        implemented.bounds[0].constraints.clear();
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
        let marker = owner(&module, &[], DefinitionKind::Trait, "Marker");
        let applied = |parameter_owner| {
            ConstraintAbi::Trait(NominalAbiType {
                declaration: marker.clone(),
                arguments: vec![AbiType::Array(Box::new(AbiType::Parameter {
                    owner: parameter_owner,
                    position: 0,
                }))],
            })
        };
        declared.bounds[0].constraints = vec![applied(trait_method.clone())];
        implemented.bounds[0].constraints = vec![applied(impl_method)];
        assert!(same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
        let ConstraintAbi::Trait(instance) = &mut implemented.bounds[0].constraints[0] else {
            unreachable!()
        };
        instance.arguments[0] = AbiType::Array(Box::new(AbiType::Builtin(BuiltinType::Bool)));
        assert!(!same_method_contract(
            &declared,
            &implemented,
            &trait_owner,
            &[],
            &impl_owner,
            &for_type,
            &cancel,
        ));
    }

    #[test]
    fn interface_tables_require_distinct_local_impl_identities() {
        let original = crate::tests::common::bytecode_ok(
            "struct Player { val value: i32 } pub trait Display { fn show(self) -> i32; } impl Display for Player { fn show(self) -> i32 { self.value } } fn main() -> i32 { 1 }",
        );
        let table_index = original
            .public_items
            .iter()
            .position(|item| matches!(item, PublicAbiItem::InterfaceTable(_)))
            .expect("checked interface table");
        let PublicAbiItem::InterfaceTable(table) = &original.public_items[table_index] else {
            unreachable!()
        };
        assert_eq!(table.declaration.module, original.identity);
        assert_eq!(table.declaration.path.len(), 1);
        assert_eq!(table.declaration.path[0].kind, DefinitionKind::Impl);
        assert!(table.declaration.path[0].name.is_empty());
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.declaration.module.package.0 = "foreign".into(),
                1 => table.declaration.path[0].kind = DefinitionKind::Trait,
                _ => table.declaration.path[0].name = "fabricated".into(),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.methods[0].return_type = AbiType::Builtin(BuiltinType::Bool),
                1 => table.methods[0].params[0].mutable = true,
                _ => table.methods[0].params[0].ty = AbiType::Builtin(BuiltinType::I32),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
        let mut wrong_trait = original.clone();
        let PublicAbiItem::InterfaceTable(table) = &mut wrong_trait.public_items[table_index]
        else {
            unreachable!()
        };
        let AbiType::Trait(reference) = &mut table.trait_type else {
            unreachable!()
        };
        reference.declaration.path[0].occurrence = 1;
        assert!(matches!(
            verify_module(&wrong_trait),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
        let mut duplicate = original.clone();
        duplicate
            .public_items
            .push(duplicate.public_items[table_index].clone());
        assert!(matches!(
            verify_module(&duplicate),
            Err(BytecodeVerificationError::InvalidPublicAbi)
        ));
        for corruption in 0..3 {
            let mut module = original.clone();
            let PublicAbiItem::InterfaceTable(table) = &mut module.public_items[table_index] else {
                unreachable!()
            };
            match corruption {
                0 => table.methods.clear(),
                1 => table.methods[0].name = "other".into(),
                _ => table.methods.push(table.methods[0].clone()),
            }
            assert!(matches!(
                verify_module(&module),
                Err(BytecodeVerificationError::InvalidPublicAbi)
            ));
        }
    }

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
