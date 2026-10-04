//! Encode checked source types at the compiler boundary.
use kagari_contract::numeric::NumericOperation;
use kagari_hir::{
    hir::expr::ops::BinaryOp,
    native::NativeTypeKind,
    types::{GenericParameterType, NominalType, TypeId},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeTypeConstructor},
    integer::IntegerOp,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

pub(crate) fn lower_native_kind(kind: NativeTypeKind) -> TypeDefKind {
    match kind {
        NativeTypeKind::Storage { layout, .. } => TypeDefKind::NativeStorage(layout),
        NativeTypeKind::String => TypeDefKind::Native(NativeTypeConstructor::String),
        NativeTypeKind::Vec => TypeDefKind::Native(NativeTypeConstructor::Array),
        NativeTypeKind::HashMap => TypeDefKind::Native(NativeTypeConstructor::Map),
        NativeTypeKind::HashSet => TypeDefKind::Native(NativeTypeConstructor::Set),
        NativeTypeKind::Iter => TypeDefKind::Native(NativeTypeConstructor::Iter),
        NativeTypeKind::Range(kind) => TypeDefKind::Native(NativeTypeConstructor::Range(kind)),
        NativeTypeKind::Enum(kind) => TypeDefKind::Native(NativeTypeConstructor::Enum(kind)),
    }
}

pub(crate) fn lower_numeric_operation(
    op: BinaryOp,
    input: BuiltinType,
    rhs: BuiltinType,
) -> Option<NumericOperation> {
    input.integer_layout()?;
    let op = match op {
        BinaryOp::Add => IntegerOp::CheckedAdd,
        BinaryOp::Sub => IntegerOp::CheckedSub,
        BinaryOp::Mul => IntegerOp::CheckedMul,
        BinaryOp::Div => IntegerOp::CheckedDiv,
        BinaryOp::Rem => IntegerOp::CheckedRem,
        BinaryOp::BitAnd => IntegerOp::BitAnd,
        BinaryOp::BitOr => IntegerOp::BitOr,
        BinaryOp::BitXor => IntegerOp::BitXor,
        BinaryOp::Shl => IntegerOp::Shl,
        BinaryOp::Shr => IntegerOp::Shr,
        _ => return None,
    };
    Some(NumericOperation {
        op,
        input,
        rhs: Some(rhs),
    })
}

pub(crate) fn raise_nominal_type(ty: &NominalTy) -> NominalType {
    NominalType {
        associated_types: ty
            .associated_types
            .iter()
            .map(|(id, ty)| (id.clone(), raise_type(ty)))
            .collect(),
        declaration: ty.declaration.clone(),
        arguments: ty.arguments.iter().map(raise_type).collect(),
    }
}

pub(crate) fn raise_type(ty: &Ty) -> TypeId {
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
            member: member.clone(),
        },
        Ty::Host(id) => TypeId::Host(id.clone()),
        Ty::SelfType(id) => TypeId::SelfType(id.clone()),
        Ty::Parameter { owner, position } => TypeId::Generic(GenericParameterType {
            owner: owner.clone(),
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
        Ty::Iter(ty) => TypeId::Iter(Box::new(raise_type(ty))),
        Ty::Array(ty, access) => TypeId::Array(Box::new(raise_type(ty)), *access),
        Ty::Map { key, value, access } => TypeId::Map {
            key: Box::new(raise_type(key)),
            value: Box::new(raise_type(value)),
            access: *access,
        },
        Ty::Set(ty, access) => TypeId::Set(Box::new(raise_type(ty)), *access),
        Ty::NativeObject(ty) => TypeId::NativeObject(raise_nominal_type(ty)),
        Ty::Struct(ty) => TypeId::Struct(raise_nominal_type(ty)),
        Ty::Enum(ty) => TypeId::Enum(raise_nominal_type(ty)),
        Ty::Trait(ty) => TypeId::Trait(raise_nominal_type(ty)),
        Ty::StandardEnum { kind, args } => TypeId::StandardEnum {
            kind: *kind,
            args: args.iter().map(raise_type).collect(),
        },
    }
}
