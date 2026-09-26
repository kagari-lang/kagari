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
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Neg,
    Not,
    Index,
}
impl StandardTrait {
    pub const ALL: [Self; 15] = [
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
    if kind.operator() {
        return build_operator_contract(kind);
    }
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

fn build_operator_contract(kind: StandardTrait) -> TraitSignature {
    let id = identity(kind);
    let source = SourceFile::new("kagari://std/operators", "");
    let declaration = |id: DefinitionId, name: &str| Declaration {
        id: DeclarationId::Definition(id),
        name: name.into(),
        location: source.span(Span::new(0, 0)).unwrap(),
    };
    let generics = if kind.binary_operator() || kind == StandardTrait::Index {
        vec![crate::types::GenericParameterType {
            owner: id.clone(),
            position: 0,
            name: "Rhs".into(),
        }]
    } else {
        vec![]
    };
    let interface = NominalType {
        declaration: id.clone(),
        arguments: generics.iter().cloned().map(TypeId::Generic).collect(),
        associated_types: Default::default(),
    };
    let output = crate::types::associated_type_id(&id, "Output");
    let name = match kind {
        StandardTrait::Add => "add",
        StandardTrait::Sub => "sub",
        StandardTrait::Mul => "mul",
        StandardTrait::Div => "div",
        StandardTrait::Rem => "rem",
        StandardTrait::Neg => "neg",
        StandardTrait::Not => "not",
        StandardTrait::Index => "index",
        _ => unreachable!(),
    };
    let mut method_id = id.clone();
    method_id.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: name.into(),
        occurrence: 0,
    });
    TraitSignature {
        declaration: declaration(id.clone(), kind.name()),
        id: id.clone(),
        generic_params: generics.clone(),
        bounds: Default::default(),
        supertraits: vec![],
        methods: vec![MethodSignature {
            has_default: false,
            id: method_id.clone(),
            owner: id.clone(),
            slot: 0,
            name: name.into(),
            generic_params: generics.clone(),
            bounds: Default::default(),
            declaration: declaration(method_id, name),
            params: std::iter::once(MethodParameter {
                name: "self".into(),
                writeability: Writeability::Val,
                ty: TypeId::SelfType(id.clone()),
            })
            .chain(generics.first().map(|p| MethodParameter {
                name: "rhs".into(),
                writeability: Writeability::Val,
                ty: TypeId::Generic(p.clone()),
            }))
            .collect(),
            return_type: TypeId::Projection {
                receiver: Box::new(TypeId::SelfType(id.clone())),
                interface: Box::new(interface),
                member: output.clone(),
                arguments: vec![],
            },
        }],
        associated_types: [(output, vec![])].into_iter().collect(),
        associated_type_parameters: Default::default(),
        associated_consts: Default::default(),
    }
}

/// Builtin associated outputs are computed from the applied protocol, not its spelling.
pub fn intrinsic_output(interface: &NominalType, receiver: &TypeId) -> Option<TypeId> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if kind == StandardTrait::Index
        && let TypeId::Array(element) = receiver
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
    StandardTrait::ALL.into_iter().find(|kind| {
        kind.name() == name
            && super::surface::standard_modules().iter().any(|spec| {
                spec.kind == module && spec.path == format!("std::{}", kind.namespace())
            })
    })
}
