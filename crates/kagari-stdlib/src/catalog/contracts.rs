//! Signature helpers for the remaining SR04 task producer only.
use crate::catalog::identities;
use kagari_common::identity::{DefinitionKind, map::DefinitionContext};
use kagari_types::{
    callable::{CallableImplementation, MethodPolicy},
    declaration::{FnDecl, Param, module::DeclarationError},
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

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

pub(super) fn unit() -> Ty {
    Ty::Builtin(BuiltinType::Unit)
}
