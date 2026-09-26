//! Standard protocols have declaration identities and ordinary trait contracts.
use crate::{
    aggregates::{AggregateCatalog, MethodParameter, MethodSignature, TraitSignature},
    declarations::{Declaration, DeclarationId},
    hir::Writeability,
    typeck::{ConstraintTarget, GenericBounds},
    types::{BuiltinType, NominalType, TypeId},
};
use kagari_common::{
    SourceFile, Span,
    identity::{DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId},
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
}
impl StandardTrait {
    pub const ALL: [Self; 7] = [
        Self::PartialEq,
        Self::Eq,
        Self::Hash,
        Self::Debug,
        Self::Display,
        Self::PartialOrd,
        Self::Ord,
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
        }
    }
    pub fn namespace(self) -> &'static str {
        match self {
            Self::PartialEq | Self::Eq | Self::PartialOrd | Self::Ord => "cmp",
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
    pub fn equality_protocol(self) -> bool {
        matches!(self, Self::PartialEq | Self::Eq | Self::Hash)
    }
    pub fn contract(self) -> &'static TraitSignature {
        static CONTRACTS: OnceLock<Vec<TraitSignature>> = OnceLock::new();
        &CONTRACTS.get_or_init(|| Self::ALL.into_iter().map(build_contract).collect())
            [self as usize]
    }
}
fn identity(kind: StandardTrait) -> DefinitionId {
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
    static SOURCE: OnceLock<SourceFile> = OnceLock::new();
    let source = SOURCE.get_or_init(|| SourceFile::new("kagari://std/protocols", ""));
    let id = identity(kind);
    let declaration = |id: DefinitionId, name: &str| Declaration {
        id: DeclarationId::Definition(id),
        name: name.into(),
        location: source.span(Span::new(0, 0)).expect("standard source"),
    };
    let mut methods = vec![];
    if kind != StandardTrait::Eq {
        let (name, result) = match kind {
            StandardTrait::PartialOrd => ("partial_cmp", ordering_type(true)),
            StandardTrait::Ord => ("cmp", ordering_type(false)),
            StandardTrait::PartialEq => ("eq", TypeId::Builtin(BuiltinType::Bool)),
            StandardTrait::Hash => ("hash", TypeId::Builtin(BuiltinType::I64)),
            StandardTrait::Debug => ("debug", TypeId::Builtin(BuiltinType::String)),
            StandardTrait::Display => ("display", TypeId::Builtin(BuiltinType::String)),
            _ => unreachable!(),
        };
        let mut method_id = id.clone();
        method_id.path.push(DefinitionPathSegment {
            kind: DefinitionKind::Method,
            name: name.into(),
            occurrence: 0,
        });
        let mut params = vec![MethodParameter {
            name: "self".into(),
            writeability: Writeability::Val,
            ty: TypeId::SelfType(id.clone()),
        }];
        if matches!(
            kind,
            StandardTrait::PartialEq | StandardTrait::PartialOrd | StandardTrait::Ord
        ) {
            params.push(MethodParameter {
                name: "other".into(),
                writeability: Writeability::Val,
                ty: TypeId::SelfType(id.clone()),
            });
        }
        methods.push(MethodSignature {
            has_default: false,
            declaration: declaration(method_id.clone(), name),
            id: method_id,
            owner: id.clone(),
            slot: 0,
            name: name.into(),
            generic_params: vec![],
            bounds: Default::default(),
            params,
            return_type: result,
        });
    }
    TraitSignature {
        declaration: declaration(id.clone(), kind.name()),
        id,
        generic_params: vec![],
        bounds: Default::default(),
        supertraits: match kind {
            StandardTrait::Eq | StandardTrait::PartialOrd => vec![StandardTrait::PartialEq],
            StandardTrait::Ord => vec![StandardTrait::Eq, StandardTrait::PartialOrd],
            _ => vec![],
        }
        .into_iter()
        .map(|kind| NominalType {
            declaration: identity(kind),
            arguments: vec![],
            associated_types: Default::default(),
        })
        .collect(),
        methods,
        associated_types: Default::default(),
        associated_type_parameters: Default::default(),
        associated_consts: Default::default(),
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
            TypeId::Struct(_) | TypeId::Array(_) | TypeId::Map { .. } | TypeId::Set(_)
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
    use super::surface::StandardModule;
    StandardTrait::ALL.into_iter().find(|kind| {
        kind.name() == name
            && matches!(
                (module, kind),
                (
                    StandardModule::Cmp,
                    StandardTrait::Eq
                        | StandardTrait::PartialEq
                        | StandardTrait::PartialOrd
                        | StandardTrait::Ord
                ) | (StandardModule::Hash, StandardTrait::Hash)
                    | (
                        StandardModule::Fmt,
                        StandardTrait::Debug | StandardTrait::Display
                    )
            )
    })
}
