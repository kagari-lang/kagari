//! Complete syntax/value contracts, explicitly expressed in Kagari's type model.
use crate::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::ModuleDecl,
    language::{self, Protocol, catalog::core_traits, primitive},
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AssociatedTypeDef, Constraint, FnDecl, GenericParam, NominalTy, Param, TraitDef, Ty},
};
use kagari_common::identity::{DefinitionPath, associated_type_id};

pub(super) fn contract(kind: Protocol, parameters: &[&str]) -> TraitDef {
    let owner = language::identity(kind);
    TraitDef {
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

pub(super) fn receiver(kind: Protocol) -> Ty {
    Ty::SelfType(language::identity(kind))
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

pub(super) fn enum_type(kind: StandardEnum, args: Vec<Ty>) -> Ty {
    Ty::StandardEnum { kind, args }
}

pub(super) fn option(item: Ty) -> Ty {
    enum_type(StandardEnum::Option, vec![item])
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

pub(super) fn applied_item(kind: Protocol, item: Ty) -> NominalTy {
    let mut applied = primitive::applied(kind, vec![]);
    applied
        .associated_types
        .insert(associated_type_id(&applied.declaration, "Item"), item);
    applied
}

pub(super) fn declare(module: &mut ModuleDecl) {
    module.traits.extend(core_traits::declarations());
    module.documentation.insert(language::identity(Protocol::Iterator),
        "A shared cursor. Each next call advances it and returns Some(item), or None when exhausted.".into());
    module.documentation.insert(
        language::identity(Protocol::Iterable),
        "Produces an iterator whose Item matches this collection or sequence.".into(),
    );
    let mut bounds = contract(Protocol::RangeBounds, &["T"]);
    let bound = enum_type(
        StandardEnum::Bound,
        vec![bounds.generic_params[0].as_type()],
    );
    bounds.methods.extend([
        method(
            "start_bound",
            vec![receiver(Protocol::RangeBounds)],
            bound.clone(),
        ),
        method("end_bound", vec![receiver(Protocol::RangeBounds)], bound),
    ]);
    module.traits.push(bounds);
}
