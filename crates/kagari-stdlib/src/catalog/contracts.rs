//! Complete syntax/value contracts, explicitly expressed in Kagari's type model.
use crate::catalog::{assembly_identity, roles};
use crate::catalog::{key, key::RegistrationTrait};
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, associated_type_id,
    mapping::{DefinitionMapper, DefinitionRecord},
};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{AssociatedTypeDef, FnDecl, Param, TraitDef, module::ModuleDecl},
    scalar::BuiltinType,
    ty::{Constraint, GenericParam, NominalTy, Ty},
};

pub(super) fn contract(kind: RegistrationTrait, parameters: &[&str]) -> TraitDef {
    let owner = key::identity(kind);
    TraitDef {
        conversion_adapter: None,
        storage_access: None,
        name: kind.name().into(),
        supertraits: vec![],
        generic_params: parameters
            .iter()
            .enumerate()
            .map(|(position, _)| GenericParam {
                owner: owner.clone(),
                position,
            })
            .collect(),
        bounds: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        methods: vec![],
    }
}

pub(super) fn receiver(kind: RegistrationTrait) -> Ty {
    Ty::SelfType(key::identity(kind))
}

pub(super) fn associated(
    owner: &DefinitionPath,
    name: &str,
    bounds: Vec<Constraint>,
) -> AssociatedTypeDef {
    AssociatedTypeDef {
        declaration: associated_type_id(owner, name),
        generic_params: vec![],
        parameter_bounds: vec![],
        bounds,
    }
}

pub(super) fn method(name: &str, params: Vec<Ty>, result: Ty) -> FnDecl {
    FnDecl {
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

pub(super) fn enum_type(name: &str, arguments: Vec<Ty>) -> Ty {
    Ty::Enum(NominalTy {
        declaration: ModuleDecl::new(assembly_identity()).definition(DefinitionKind::Enum, name),
        arguments,
        associated_types: Default::default(),
    })
}

pub(super) fn option(item: Ty) -> Ty {
    enum_type("Option", vec![item])
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

pub(super) fn applied_item(kind: RegistrationTrait, item: Ty) -> NominalTy {
    let mut applied = key::applied(kind, vec![]);
    applied
        .associated_types
        .insert(associated_type_id(&applied.declaration, "Item"), item);
    applied
}

pub(super) fn declare(module: &mut ModuleDecl) {
    module.traits.extend(
        roles::declarations()
            .map_identities(&mut DefinitionMapper::new(
                &mut |id: &DefinitionPath| {
                    let mut id = id.clone();
                    id.module = module.identity.clone();
                    Ok(id)
                },
                &Default::default(),
            ))
            .expect("foundation assembly identities"),
    );
    let mut bounds = contract(RegistrationTrait::RangeBounds, &["T"]);
    let bound = enum_type("Bound", vec![bounds.generic_params[0].as_type()]);
    bounds.methods.extend([
        method(
            "start_bound",
            vec![receiver(RegistrationTrait::RangeBounds)],
            bound.clone(),
        ),
        method(
            "end_bound",
            vec![receiver(RegistrationTrait::RangeBounds)],
            bound,
        ),
    ]);
    module.traits.push(bounds);
}
