//! Standard protocols have declaration identities and ordinary trait contracts.
use crate::{
    aggregates::{AggregateCatalog, TraitSignature},
    typeck::{ConstraintTarget, GenericBounds},
    types::{BuiltinType, NominalType, TypeId},
};
use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardTrait {
    PartialEq,
    Eq,
    Hash,
    Debug,
    Display,
    PartialOrd,
    Ord,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
    Index,
    From,
    Into,
    TryFrom,
    TryInto,
    Iterator,
    IntoIterator,
}
impl StandardTrait {
    pub const ALL: [Self; 21] = [
        Self::PartialEq,
        Self::Eq,
        Self::Hash,
        Self::Debug,
        Self::Display,
        Self::PartialOrd,
        Self::Ord,
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Rem,
        Self::Neg,
        Self::Not,
        Self::Index,
        Self::From,
        Self::Into,
        Self::TryFrom,
        Self::TryInto,
        Self::Iterator,
        Self::IntoIterator,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::PartialEq => "PartialEq",
            Self::Eq => "Eq",
            Self::Hash => "Hash",
            Self::Debug => "Debug",
            Self::Display => "Display",
            Self::PartialOrd => "PartialOrd",
            Self::Ord => "Ord",
            Self::Add => "Add",
            Self::Sub => "Sub",
            Self::Mul => "Mul",
            Self::Div => "Div",
            Self::Rem => "Rem",
            Self::Neg => "Neg",
            Self::Not => "Not",
            Self::Index => "Index",
            Self::From => "From",
            Self::Into => "Into",
            Self::TryFrom => "TryFrom",
            Self::TryInto => "TryInto",
            Self::Iterator => "Iterator",
            Self::IntoIterator => "IntoIterator",
        }
    }
    pub fn namespace(self) -> &'static str {
        match self {
            Self::PartialEq | Self::Eq | Self::PartialOrd | Self::Ord => "cmp",
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::Neg
            | Self::Not
            | Self::Index => "ops",
            Self::From | Self::Into | Self::TryFrom | Self::TryInto => "convert",
            Self::Iterator | Self::IntoIterator => "iter",
            Self::Hash => "hash",
            Self::Debug | Self::Display => "fmt",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| {
            name == kind.name() || name == format!("std::{}::{}", kind.namespace(), kind.name())
        })
    }
    pub fn from_id(id: &DefinitionId) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| &kind.contract().id == id)
    }
    pub fn nominal(self) -> NominalType {
        NominalType {
            declaration: self.contract().id.clone(),
            arguments: vec![],
            associated_types: Default::default(),
        }
    }
    /// Declaration view used by type syntax; callers supply concrete arguments.
    pub fn declaration_type(self) -> NominalType {
        let mut ty = self.nominal();
        ty.arguments = self
            .contract()
            .generic_params
            .iter()
            .cloned()
            .map(TypeId::Generic)
            .collect();
        ty
    }
    pub fn iteration(self) -> bool {
        matches!(self, Self::Iterator | Self::IntoIterator)
    }
    pub fn conversion(self) -> bool {
        matches!(
            self,
            Self::From | Self::Into | Self::TryFrom | Self::TryInto
        )
    }
    pub fn reverse_conversion(self) -> bool {
        matches!(self, Self::Into | Self::TryInto)
    }
    pub fn fallible_conversion(self) -> bool {
        matches!(self, Self::TryFrom | Self::TryInto)
    }
    pub fn binary_operator(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Rem
        )
    }
    pub fn operator(self) -> bool {
        self.binary_operator() || matches!(self, Self::Neg | Self::Not | Self::Index)
    }
    pub fn intrinsic_view(self, receiver: &TypeId) -> NominalType {
        let mut view = self.nominal();
        if self.binary_operator() {
            view.arguments.push(receiver.clone());
        }
        if self == Self::Index {
            view.arguments.push(TypeId::Builtin(BuiltinType::I32));
        }
        if let Some(output) = intrinsic_output(&view, receiver) {
            view.associated_types.insert(
                crate::types::associated_type_id(&view.declaration, "Output"),
                output,
            );
        }
        view
    }
    pub fn host_implementable(self) -> bool {
        matches!(self, Self::Debug | Self::Display)
    }
    pub fn equality_protocol(self) -> bool {
        matches!(self, Self::PartialEq | Self::Eq | Self::Hash)
    }
    pub fn contract(self) -> &'static TraitSignature {
        static CONTRACTS: OnceLock<Vec<TraitSignature>> = OnceLock::new();
        &CONTRACTS.get_or_init(|| Self::ALL.into_iter().map(build_contract).collect())
            [self as usize]
    }
}
pub(crate) fn identity(kind: StandardTrait) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity {
            package: PackageId("kagari-std".into()),
            path: vec![kind.namespace().into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: kind.name().into(),
            occurrence: 0,
        }],
    }
}
fn build_contract(kind: StandardTrait) -> TraitSignature {
    super::surface::STANDARD_TRAITS
        .iter()
        .find(|spec| spec.item.identity() == identity(kind))
        .expect("standard trait declaration")
        .contract()
}

/// Builtin associated outputs are computed from the applied protocol, not its spelling.
pub fn intrinsic_output(interface: &NominalType, receiver: &TypeId) -> Option<TypeId> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if kind == StandardTrait::Index
        && let TypeId::Array(element, _) = receiver
        && matches!(
            interface.arguments.as_slice(),
            [TypeId::Builtin(
                BuiltinType::I8
                    | BuiltinType::I16
                    | BuiltinType::I32
                    | BuiltinType::I64
                    | BuiltinType::ISize
                    | BuiltinType::U8
                    | BuiltinType::U16
                    | BuiltinType::U32
                    | BuiltinType::U64
                    | BuiltinType::USize
            )]
        )
    {
        return Some((**element).clone());
    }
    if interface.arguments.is_empty()
        && (kind == StandardTrait::Not && *receiver == TypeId::Builtin(BuiltinType::Bool)
            || kind == StandardTrait::Neg
                && matches!(
                    receiver,
                    TypeId::Builtin(
                        BuiltinType::I8
                            | BuiltinType::I16
                            | BuiltinType::I32
                            | BuiltinType::I64
                            | BuiltinType::ISize
                            | BuiltinType::F32
                            | BuiltinType::F64
                    )
                ))
    {
        return Some(receiver.clone());
    }
    if kind.binary_operator()
        && interface.arguments.as_slice() == [receiver.clone()]
        && super::surface::supports_arithmetic(receiver, receiver)
    {
        Some(receiver.clone())
    } else {
        None
    }
}

pub fn intrinsic_applies(
    interface: &NominalType,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return false;
    };
    if kind.iteration() {
        return iteration_outputs(kind, receiver, catalog, bounds).is_some_and(|outputs| {
            interface.arguments.is_empty()
                && interface
                    .associated_types
                    .iter()
                    .all(|(member, ty)| outputs.get(member) == Some(ty))
        });
    }
    if kind.conversion() {
        return kind == StandardTrait::From
            && interface.arguments.as_slice() == [receiver.clone()]
            && interface.associated_types.is_empty();
    }
    if kind.operator() {
        let Some(output) = intrinsic_output(interface, receiver) else {
            return false;
        };
        interface.associated_types.iter().all(|(member, ty)| {
            *member == crate::types::associated_type_id(&interface.declaration, "Output")
                && *ty == output
        })
    } else {
        interface.arguments.is_empty()
            && interface.associated_types.is_empty()
            && intrinsic_holds(kind, receiver, catalog, bounds)
    }
}

pub fn ordering_type(optional: bool) -> TypeId {
    let ordering = TypeId::StandardEnum {
        kind: super::surface::StandardEnum::Ordering,
        args: vec![],
    };
    if optional {
        TypeId::StandardEnum {
            kind: super::surface::StandardEnum::Option,
            args: vec![ordering],
        }
    } else {
        ordering
    }
}

/// Intrinsic implementations preserve the language's value/identity contract.
/// Explicit Debug/Display implementations are selected before intrinsic fallbacks.
pub fn intrinsic_holds(
    protocol: StandardTrait,
    ty: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> bool {
    if protocol.iteration() {
        return iteration_outputs(protocol, ty, catalog, bounds).is_some();
    }
    if protocol.conversion() {
        return false;
    }
    if protocol.operator() {
        return intrinsic_output(&protocol.intrinsic_view(ty), ty).is_some();
    }
    if matches!(protocol, StandardTrait::PartialOrd | StandardTrait::Ord) {
        if bounds.get(ty).is_some_and(|constraints| constraints.iter().any(|c| matches!(c, ConstraintTarget::Trait(n) if n.declaration == protocol.contract().id || protocol == StandardTrait::PartialOrd && n.declaration == StandardTrait::Ord.contract().id))) {return true;}
        if let Some(catalog) = catalog
            && matches!(
                catalog.concrete_interface_implementation(
                    &protocol.nominal(),
                    ty,
                    bounds,
                    4096,
                    64,
                    &Default::default()
                ),
                Ok(Some(_))
            )
        {
            return true;
        }
        return match ty {
            TypeId::Unknown | TypeId::Error => true,
            TypeId::Builtin(BuiltinType::F32 | BuiltinType::F64) => {
                protocol == StandardTrait::PartialOrd
            }
            TypeId::Builtin(_) => true,
            TypeId::StandardEnum {
                kind: super::surface::StandardEnum::Ordering,
                ..
            } => true,
            _ => false,
        };
    }
    if protocol.equality_protocol()
        && let Some(catalog) = catalog
    {
        return catalog.standard_protocol_holds(protocol, ty, bounds);
    }
    let mut pending = vec![(ty.clone(), 0usize)];
    let mut seen = std::collections::HashSet::new();
    while let Some((ty, depth)) = pending.pop() {
        if seen.len() >= 4096 || depth > 64 {
            return false;
        }
        if !seen.insert(ty.clone()) {
            continue;
        }
        if bounds.get(&ty).is_some_and(|bounds| bounds.iter().any(|bound| matches!(bound, ConstraintTarget::Trait(n) if StandardTrait::from_id(&n.declaration).is_some_and(|p| p == protocol || p == StandardTrait::Eq && protocol == StandardTrait::PartialEq)))) { continue; }
        match ty {
            // Recovery holes cannot disprove a protocol; code generation rejects them.
            TypeId::Unknown | TypeId::Error => {}
            TypeId::Builtin(b) => {
                if matches!(protocol, StandardTrait::Eq | StandardTrait::Hash)
                    && matches!(b, BuiltinType::F32 | BuiltinType::F64)
                {
                    return false;
                }
            }
            TypeId::Enum(_) | TypeId::Host(_) if protocol == StandardTrait::Debug => {}
            TypeId::Struct(_) | TypeId::Array(_, _) | TypeId::Map { .. } | TypeId::Set(_, _)
                if protocol != StandardTrait::Display => {}
            TypeId::Tuple(elements) | TypeId::StandardEnum { args: elements, .. }
                if protocol != StandardTrait::Display =>
            {
                pending.extend(elements.into_iter().map(|ty| (ty, depth + 1)))
            }
            TypeId::Enum(instance) if protocol != StandardTrait::Display => {
                let Some(catalog) = catalog else {
                    continue;
                };
                let Some(contract) = catalog.enumeration(&instance.declaration) else {
                    if let Some(payload) = catalog.concrete_enum_payload(&instance) {
                        pending.extend(payload.iter().cloned().map(|ty| (ty, depth + 1)));
                        continue;
                    }
                    return false;
                };
                let substitution = contract
                    .generic_params
                    .iter()
                    .cloned()
                    .zip(instance.arguments)
                    .collect();
                for variant in &contract.variants {
                    pending.extend(
                        variant
                            .payload
                            .iter()
                            .map(|ty| (ty.instantiate(&substitution), depth + 1)),
                    );
                }
            }
            _ => return false,
        }
    }
    true
}

pub fn in_module(module: super::surface::StandardModule, name: &str) -> Option<StandardTrait> {
    StandardTrait::ALL.into_iter().find(|kind| {
        kind.name() == name
            && super::surface::standard_modules().iter().any(|spec| {
                spec.kind == module && spec.path == format!("std::{}", kind.namespace())
            })
    })
}

/// The reverse protocol is a proof of the corresponding destination conversion.
pub fn conversion_requirement(
    interface: &NominalType,
    receiver: &TypeId,
) -> Option<(NominalType, TypeId)> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if !kind.reverse_conversion() || interface.arguments.len() != 1 {
        return None;
    }
    let forward = if kind == StandardTrait::Into {
        StandardTrait::From
    } else {
        StandardTrait::TryFrom
    };
    let mut required = forward.nominal();
    required.arguments.push(receiver.clone());
    for (member, ty) in &interface.associated_types {
        if *member != crate::types::associated_type_id(&interface.declaration, "Error")
            || !kind.fallible_conversion()
        {
            return None;
        }
        required.associated_types.insert(
            crate::types::associated_type_id(&required.declaration, "Error"),
            ty.clone(),
        );
    }
    Some((required, interface.arguments[0].clone()))
}

pub fn iteration_outputs(
    kind: StandardTrait,
    receiver: &TypeId,
    catalog: Option<&AggregateCatalog>,
    bounds: &GenericBounds,
) -> Option<std::collections::BTreeMap<DefinitionId, TypeId>> {
    let native_item = match receiver {
        TypeId::Cursor(item) => Some((**item).clone()),
        TypeId::Array(item, _) | TypeId::Set(item, _) if kind == StandardTrait::IntoIterator => {
            Some((**item).clone())
        }
        TypeId::Map { key, value, .. } if kind == StandardTrait::IntoIterator => {
            Some(TypeId::Tuple(vec![(**key).clone(), (**value).clone()]))
        }
        TypeId::Builtin(BuiltinType::String) if kind == StandardTrait::IntoIterator => {
            Some(receiver.clone())
        }
        _ => None,
    };
    if let Some(item) = native_item {
        let id = identity(kind);
        let mut outputs = std::collections::BTreeMap::from([(
            crate::types::associated_type_id(&id, "Item"),
            item.clone(),
        )]);
        if kind == StandardTrait::IntoIterator {
            outputs.insert(
                crate::types::associated_type_id(&id, "IntoIter"),
                TypeId::Cursor(Box::new(item)),
            );
        }
        return Some(outputs);
    }
    if kind != StandardTrait::IntoIterator {
        return None;
    }
    let iterator = StandardTrait::Iterator.nominal();
    let available = bounds
        .get(receiver)
        .into_iter()
        .flatten()
        .any(|b| matches!(b,ConstraintTarget::Trait(n) if n.declaration==iterator.declaration))
        || catalog.is_some_and(|c| {
            c.concrete_interface_implementation(
                &iterator,
                receiver,
                bounds,
                4096,
                64,
                &Default::default(),
            )
            .is_ok_and(|i| i.is_some())
        });
    if !available {
        return None;
    }
    let member = crate::types::associated_type_id(&iterator.declaration, "Item");
    let item = bounds
        .get(receiver)
        .into_iter()
        .flatten()
        .find_map(|b| match b {
            ConstraintTarget::Trait(n) if n.declaration == iterator.declaration => {
                n.associated_types.get(&member).cloned()
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let projection = TypeId::Projection {
                receiver: Box::new(receiver.clone()),
                interface: Box::new(iterator),
                member,
                arguments: vec![],
            };
            catalog.map_or(projection.clone(), |c| c.normalize_type(&projection))
        });
    let id = identity(kind);
    Some(
        [
            (crate::types::associated_type_id(&id, "Item"), item),
            (
                crate::types::associated_type_id(&id, "IntoIter"),
                receiver.clone(),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

/// Identity IntoIterator's proof obligation, for recursive searches using one budget.
pub fn iterator_requirement(interface: &NominalType, receiver: &TypeId) -> Option<NominalType> {
    if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::IntoIterator)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let item = crate::types::associated_type_id(&interface.declaration, "Item");
    let iterator = crate::types::associated_type_id(&interface.declaration, "IntoIter");
    let mut required = StandardTrait::Iterator.nominal();
    for (member, ty) in &interface.associated_types {
        if *member == item {
            required.associated_types.insert(
                crate::types::associated_type_id(&required.declaration, "Item"),
                ty.clone(),
            );
        } else if *member != iterator || ty != receiver {
            return None;
        }
    }
    Some(required)
}
