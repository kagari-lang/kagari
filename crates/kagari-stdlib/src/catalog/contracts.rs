//! Complete syntax/value contracts, explicitly expressed in Kagari's scoped type model.
use crate::catalog::{
    identities,
    key::{self, RegistrationTrait},
    roles,
};
use kagari_common::identity::{DefinitionKind, map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{
        AssociatedTypeDef, FnDecl, Param, TraitDef,
        module::{DeclarationError, ModuleDecl},
    },
    scalar::BuiltinType,
    ty::{GenericParam, NominalTy, Ty},
};

pub(super) fn contract(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
    parameters: &[&str],
) -> Result<TraitDef, DeclarationError> {
    let owner = key::identity(ids, kind)?;
    Ok(TraitDef {
        conversion_adapter: None,
        name: kind.name().into(),
        supertraits: vec![],
        generic_params: parameters
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParam { owner, position })
            .collect(),
        bounds: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        methods: vec![],
    })
}

pub(super) fn receiver(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
) -> Result<Ty, DeclarationError> {
    Ok(Ty::SelfType(key::identity(ids, kind)?))
}

pub(super) fn associated(
    ids: &DefinitionContext,
    owner: &DefinitionId,
    name: &str,
    bounds: Vec<NominalTy>,
) -> Result<AssociatedTypeDef, DeclarationError> {
    Ok(AssociatedTypeDef {
        declaration: identities::associated(ids, owner, name)?,
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds,
    })
}

pub(super) fn method(name: &str, params: Vec<Ty>, result: Ty) -> FnDecl {
    FnDecl {
        host_policy: None,
        name: name.into(),
        method_policy: MethodPolicy::default(),
        implementation: CallableImplementation::Required,
        generic_params: vec![],
        bounds: vec![],
        params: params
            .into_iter()
            .enumerate()
            .map(|(index, ty)| Param {
                name: if index == 0 {
                    "self".into()
                } else {
                    format!("arg{index}")
                },
                ty,
                mutable: false,
            })
            .collect(),
        return_type: result,
    }
}

pub(super) fn enum_type(
    ids: &DefinitionContext,
    name: &str,
    arguments: Vec<Ty>,
) -> Result<Ty, DeclarationError> {
    Ok(Ty::Enum(NominalTy {
        declaration: identities::definition(ids, DefinitionKind::Enum, name)?,
        arguments,
        associated_types: Default::default(),
    }))
}

pub(super) fn option(ids: &DefinitionContext, item: Ty) -> Result<Ty, DeclarationError> {
    enum_type(ids, "Option", vec![item])
}

pub(super) fn unit() -> Ty {
    Ty::Builtin(BuiltinType::Unit)
}

pub(super) fn usize_type() -> Ty {
    Ty::Builtin(BuiltinType::USize)
}

pub(super) fn boolean() -> Ty {
    Ty::Builtin(BuiltinType::Bool)
}

pub(super) fn applied_item(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
    item: Ty,
) -> Result<NominalTy, DeclarationError> {
    let mut applied = key::applied(ids, kind, vec![])?;
    applied.associated_types.insert(
        identities::associated(ids, &applied.declaration, "Item")?,
        item,
    );
    Ok(applied)
}

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    module.traits.extend(roles::declarations(ids)?);
    let mut bounds = contract(ids, RegistrationTrait::RangeBounds, &["T"])?;
    let bound = enum_type(ids, "Bound", vec![bounds.generic_params[0].as_type()])?;
    bounds.methods.extend([
        method(
            "start_bound",
            vec![receiver(ids, RegistrationTrait::RangeBounds)?],
            bound.clone(),
        ),
        method(
            "end_bound",
            vec![receiver(ids, RegistrationTrait::RangeBounds)?],
            bound,
        ),
    ]);
    module.traits.push(bounds);
    Ok(())
}

/// The concrete growable list type in the assembly; partitioning assigns its owner.
pub(super) fn vec_type(ids: &DefinitionContext, item: Ty) -> Result<Ty, DeclarationError> {
    Ok(Ty::NativeObject(NominalTy {
        declaration: identities::definition(ids, DefinitionKind::AssociatedType, "Vec")?,
        arguments: vec![item],
        associated_types: Default::default(),
    }))
}
