//! Convert checked HIR types to source-independent semantic facts in the same scope.
use crate::types::{GenericParameterType, NominalType, TypeId};
use kagari_common::identity::{
    ModuleIdentity,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::ownership::trait_impl_orphan_error,
    ty::{NominalTy, Ty, matching::impl_headers_overlap},
};

/// The input patterns of one resolved implementation, lowered once per coherence pass.
/// Bounds and associated outputs do not distinguish implementations.
pub(crate) struct ImplHeader {
    receiver: Ty,
    arguments: Vec<Ty>,
}

impl ImplHeader {
    pub(crate) fn lower(receiver: &TypeId, arguments: &[TypeId]) -> Self {
        Self {
            receiver: lower_type(receiver),
            arguments: arguments.iter().map(lower_type).collect(),
        }
    }

    pub(crate) fn orphan_error(
        &self,
        module: &ModuleIdentity,
        definitions: &DefinitionTable,
        interface: DefinitionId,
    ) -> Option<&'static str> {
        trait_impl_orphan_error(
            module,
            definitions,
            interface,
            &self.receiver,
            &self.arguments,
        )
    }

    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        impl_headers_overlap(
            &self.receiver,
            &self.arguments,
            &other.receiver,
            &other.arguments,
        )
    }
}

/// Converts a nominal application to source-independent semantic metadata; child recovery types panic as in `lower_type`.
pub fn lower_nominal_type(ty: &NominalType) -> NominalTy {
    NominalTy {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (*id, lower_type(ty)))
            .collect(),
        declaration: ty.declaration,
        arguments: ty.arguments.iter().map(lower_type).collect(),
    }
}

/// Converts HIR semantic types to the shared source-independent type representation.
///
/// Generic parameters and projections are preserved; this does not monomorphize.
///
/// # Panics
///
/// Panics if any inference variable, unknown or error type reaches this checked boundary.
pub fn lower_type(ty: &TypeId) -> Ty {
    match ty {
        TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => Ty::Projection {
            arguments: arguments.iter().map(lower_type).collect(),
            receiver: Box::new(lower_type(receiver)),
            interface: Box::new(lower_nominal_type(interface)),
            member: *member,
        },
        TypeId::Host(id) => Ty::Host(*id),
        TypeId::Builtin(ty) => Ty::Builtin(*ty),
        TypeId::Tuple(elements) => Ty::Tuple(elements.iter().map(lower_type).collect()),
        TypeId::Function { params, result } => Ty::Function {
            params: params.iter().map(lower_type).collect(),
            result: Box::new(lower_type(result)),
        },
        TypeId::Range(element, kind) => Ty::Range(Box::new(lower_type(element)), *kind),

        TypeId::Array(element) => Ty::Array(Box::new(lower_type(element))),

        TypeId::NativeObject(ty) => Ty::NativeObject(lower_nominal_type(ty)),
        TypeId::Struct(ty) => Ty::Struct(lower_nominal_type(ty)),
        TypeId::Enum(ty) => Ty::Enum(lower_nominal_type(ty)),
        TypeId::Trait(ty) => Ty::Trait(lower_nominal_type(ty)),

        TypeId::Generic(parameter) => Ty::Parameter {
            owner: parameter.owner,
            position: parameter.position,
        },
        TypeId::SelfType(owner) => Ty::SelfType(*owner),
        TypeId::Inference(_) | TypeId::Unknown | TypeId::Error => {
            unreachable!("non-concrete type reached concrete ABI encoding")
        }
    }
}

/// Reconstructs a HIR nominal application from scoped semantic metadata.
pub fn raise_nominal_type(ty: &NominalTy) -> NominalType {
    NominalType {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (*id, raise_type(ty)))
            .collect(),
        declaration: ty.declaration,
        arguments: ty.arguments.iter().map(raise_type).collect(),
    }
}

/// Reconstructs a HIR semantic type from scoped metadata, synthesizing generic display names where needed.
pub fn raise_type(ty: &Ty) -> TypeId {
    match ty {
        Ty::Projection {
            receiver,
            interface,
            member,
            arguments,
        } => TypeId::Projection {
            arguments: arguments.iter().map(raise_type).collect(),
            receiver: Box::new(raise_type(receiver)),
            interface: Box::new(raise_nominal_type(interface)),
            member: *member,
        },
        Ty::Host(id) => TypeId::Host(*id),
        Ty::SelfType(id) => TypeId::SelfType(*id),
        Ty::Parameter { owner, position } => TypeId::Generic(GenericParameterType {
            owner: *owner,
            position: *position,
            name: String::new(),
        }),
        Ty::Builtin(ty) => TypeId::Builtin(*ty),
        Ty::Tuple(types) => TypeId::Tuple(types.iter().map(raise_type).collect()),
        Ty::Function { params, result } => TypeId::Function {
            params: params.iter().map(raise_type).collect(),
            result: Box::new(raise_type(result)),
        },
        Ty::Range(ty, kind) => TypeId::Range(Box::new(raise_type(ty)), *kind),

        Ty::Array(ty) => TypeId::Array(Box::new(raise_type(ty))),

        Ty::NativeObject(ty) => TypeId::NativeObject(raise_nominal_type(ty)),
        Ty::Struct(ty) => TypeId::Struct(raise_nominal_type(ty)),
        Ty::Enum(ty) => TypeId::Enum(raise_nominal_type(ty)),
        Ty::Trait(ty) => TypeId::Trait(raise_nominal_type(ty)),
    }
}
