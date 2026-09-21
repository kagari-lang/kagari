use kagari_common::identity::DefinitionId;

pub type TypeSubstitution = std::collections::HashMap<GenericParameterType, TypeId>;

#[cfg(test)]
mod nominal_tests;

/// Names are diagnostic metadata; owner and position determine equality.
#[derive(Debug, Clone)]
pub struct GenericParameterType {
    pub owner: DefinitionId,
    pub position: usize,
    pub name: String,
}

impl PartialEq for GenericParameterType {
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner && self.position == other.position
    }
}
impl Eq for GenericParameterType {}
impl std::hash::Hash for GenericParameterType {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.owner.hash(state);
        self.position.hash(state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BuiltinType {
    Unit,
    Bool,
    I8,
    I16,
    I32,
    I64,
    ISize,
    U8,
    U16,
    U32,
    U64,
    USize,
    F32,
    F64,
    String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NominalType {
    pub declaration: DefinitionId,
    pub arguments: Vec<TypeId>,
}

impl NominalType {
    fn map_arguments(&self, mut map: impl FnMut(&TypeId) -> TypeId) -> Self {
        Self {
            declaration: self.declaration.clone(),
            arguments: self.arguments.iter().map(&mut map).collect(),
        }
    }

    fn display_name(&self) -> String {
        let name = &self
            .declaration
            .path
            .last()
            .expect("type declaration path")
            .name;
        if self.arguments.is_empty() {
            name.clone()
        } else {
            format!(
                "{name}<{}>",
                self.arguments
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeId {
    Unknown,
    Error,
    Builtin(BuiltinType),
    Tuple(Vec<TypeId>),
    Array(Box<TypeId>),
    Map {
        key: Box<TypeId>,
        value: Box<TypeId>,
    },
    Set(Box<TypeId>),
    Struct(NominalType),
    Enum(NominalType),
    Trait(NominalType),
    Host(DefinitionId),
    Generic(GenericParameterType),
    SelfType(DefinitionId),
    StandardEnum {
        kind: crate::builtin::surface::StandardEnum,
        args: Vec<TypeId>,
    },
}

impl TypeId {
    /// Substitute one binder layer; replacements can contain the caller's parameters.
    pub fn instantiate(&self, substitution: &TypeSubstitution) -> TypeId {
        match self {
            Self::Struct(ty) => Self::Struct(ty.map_arguments(|arg| arg.instantiate(substitution))),
            Self::Enum(ty) => Self::Enum(ty.map_arguments(|arg| arg.instantiate(substitution))),
            Self::Trait(ty) => Self::Trait(ty.map_arguments(|arg| arg.instantiate(substitution))),
            Self::Generic(parameter) => substitution
                .get(parameter)
                .cloned()
                .unwrap_or_else(|| self.clone()),
            Self::Tuple(elements) => Self::Tuple(
                elements
                    .iter()
                    .map(|ty| ty.instantiate(substitution))
                    .collect(),
            ),
            Self::Array(element) => Self::Array(Box::new(element.instantiate(substitution))),
            Self::Map { key, value } => Self::Map {
                key: Box::new(key.instantiate(substitution)),
                value: Box::new(value.instantiate(substitution)),
            },
            Self::Set(element) => Self::Set(Box::new(element.instantiate(substitution))),
            Self::StandardEnum { kind, args } => Self::StandardEnum {
                kind: *kind,
                args: args.iter().map(|ty| ty.instantiate(substitution)).collect(),
            },
            _ => self.clone(),
        }
    }

    pub fn is_concrete(&self) -> bool {
        self.is_resolved_in(&[])
    }

    /// A caller-owned binder is known context even before monomorphization.
    pub(crate) fn is_resolved_in(&self, parameters: &[GenericParameterType]) -> bool {
        match self {
            Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                ty.arguments.iter().all(|ty| ty.is_resolved_in(parameters))
            }
            Self::Generic(parameter) => parameters.contains(parameter),
            Self::Unknown | Self::Error | Self::SelfType(_) => false,
            Self::Tuple(elements) | Self::StandardEnum { args: elements, .. } => {
                elements.iter().all(|ty| ty.is_resolved_in(parameters))
            }
            Self::Array(element) | Self::Set(element) => element.is_resolved_in(parameters),
            Self::Map { key, value } => {
                key.is_resolved_in(parameters) && value.is_resolved_in(parameters)
            }
            _ => true,
        }
    }
    pub(crate) fn with_self(&self, owner: &DefinitionId, replacement: &TypeId) -> TypeId {
        match self {
            Self::Struct(ty) => {
                Self::Struct(ty.map_arguments(|arg| arg.with_self(owner, replacement)))
            }
            Self::Enum(ty) => Self::Enum(ty.map_arguments(|arg| arg.with_self(owner, replacement))),
            Self::Trait(ty) => {
                Self::Trait(ty.map_arguments(|arg| arg.with_self(owner, replacement)))
            }
            Self::SelfType(id) if id == owner => replacement.clone(),
            Self::Tuple(elements) => Self::Tuple(
                elements
                    .iter()
                    .map(|ty| ty.with_self(owner, replacement))
                    .collect(),
            ),
            Self::Array(element) => Self::Array(Box::new(element.with_self(owner, replacement))),
            Self::Map { key, value } => Self::Map {
                key: Box::new(key.with_self(owner, replacement)),
                value: Box::new(value.with_self(owner, replacement)),
            },
            Self::Set(element) => Self::Set(Box::new(element.with_self(owner, replacement))),
            Self::StandardEnum { kind, args } => Self::StandardEnum {
                kind: *kind,
                args: args
                    .iter()
                    .map(|ty| ty.with_self(owner, replacement))
                    .collect(),
            },
            _ => self.clone(),
        }
    }
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Self::Builtin(
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
            )
        )
    }
    pub fn supports_equality(&self) -> bool {
        match self {
            Self::Unknown
            | Self::Error
            | Self::Trait(_)
            | Self::Host(_)
            | Self::Generic(_)
            | Self::SelfType(_) => false,
            Self::Tuple(members) | Self::StandardEnum { args: members, .. } => {
                members.iter().all(Self::supports_equality)
            }
            // The elements of mutable containers do not participate in identity equality.
            Self::Builtin(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Array(_)
            | Self::Map { .. }
            | Self::Set(_) => true,
        }
    }

    /// Recovery types suppress dependent diagnostics but never authorize codegen.
    pub fn is_unresolved(&self) -> bool {
        match self {
            Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => {
                ty.arguments.iter().any(Self::is_unresolved)
            }
            Self::Unknown | Self::Error => true,
            Self::Tuple(elements) | Self::StandardEnum { args: elements, .. } => {
                elements.iter().any(Self::is_unresolved)
            }
            Self::Array(element) | Self::Set(element) => element.is_unresolved(),
            Self::Map { key, value } => key.is_unresolved() || value.is_unresolved(),
            _ => false,
        }
    }

    /// Fill recovery holes from another checked expression, preserving known facts.
    pub(crate) fn recover_from(&mut self, other: &Self) {
        if matches!(other, Self::Unknown | Self::Error) || self.conflicts_with(other) {
            return;
        }
        match (self, other) {
            (left @ (Self::Unknown | Self::Error), right) => *left = right.clone(),
            (Self::Tuple(left), Self::Tuple(right)) => {
                for (left, right) in left.iter_mut().zip(right) {
                    left.recover_from(right);
                }
            }
            (Self::Array(left), Self::Array(right)) | (Self::Set(left), Self::Set(right)) => {
                left.recover_from(right);
            }
            (Self::Map { key: lk, value: lv }, Self::Map { key: rk, value: rv }) => {
                lk.recover_from(rk);
                lv.recover_from(rv);
            }
            (Self::Struct(left), Self::Struct(right))
            | (Self::Enum(left), Self::Enum(right))
            | (Self::Trait(left), Self::Trait(right)) => {
                for (left, right) in left.arguments.iter_mut().zip(&right.arguments) {
                    left.recover_from(right);
                }
            }
            (Self::StandardEnum { args: left, .. }, Self::StandardEnum { args: right, .. }) => {
                for (left, right) in left.iter_mut().zip(right) {
                    left.recover_from(right);
                }
            }
            _ => {}
        }
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        fn members_conflict(left: &[TypeId], right: &[TypeId]) -> bool {
            left.len() != right.len() || left.iter().zip(right).any(|(a, b)| a.conflicts_with(b))
        }
        match (self, other) {
            (Self::Unknown | Self::Error, _) | (_, Self::Unknown | Self::Error) => false,
            (Self::Tuple(left), Self::Tuple(right)) => members_conflict(left, right),
            (Self::Array(left), Self::Array(right)) | (Self::Set(left), Self::Set(right)) => {
                left.conflicts_with(right)
            }
            (Self::Map { key: lk, value: lv }, Self::Map { key: rk, value: rv }) => {
                lk.conflicts_with(rk) || lv.conflicts_with(rv)
            }
            (Self::Struct(left), Self::Struct(right))
            | (Self::Enum(left), Self::Enum(right))
            | (Self::Trait(left), Self::Trait(right)) => {
                left.declaration != right.declaration
                    || members_conflict(&left.arguments, &right.arguments)
            }
            (
                Self::StandardEnum { kind: lk, args: la },
                Self::StandardEnum { kind: rk, args: ra },
            ) => lk != rk || members_conflict(la, ra),
            _ => self != other,
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        crate::builtin::surface::builtin_type(name).map(Self::Builtin)
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Unknown => "<unknown>".to_owned(),
            Self::Error => "<error>".to_owned(),
            Self::Host(id) => id.path.last().expect("host type identity").name.clone(),
            Self::Builtin(ty) => crate::builtin::surface::builtin_type_spec(*ty)
                .map(|spec| spec.name.to_owned())
                .unwrap_or("<builtin>".to_owned()),
            Self::Tuple(elements) => {
                let inner = elements
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({inner})")
            }
            Self::Array(element) => format!("[{}]", element.display_name()),
            Self::Map { key, value } => {
                format!("Map<{}, {}>", key.display_name(), value.display_name())
            }
            Self::Set(element) => format!("Set<{}>", element.display_name()),
            Self::Struct(ty) | Self::Enum(ty) | Self::Trait(ty) => ty.display_name(),
            Self::Generic(parameter) => parameter.name.clone(),
            Self::SelfType(_) => "Self".to_owned(),
            Self::StandardEnum { kind, args } => {
                let inner = args
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{inner}>", kind.spec().name)
            }
        }
    }

    pub fn is_heap_backed(&self) -> bool {
        match self {
            Self::Unknown | Self::Error => false,
            Self::Builtin(ty) => {
                crate::builtin::surface::builtin_type_spec(*ty).is_some_and(|spec| spec.heap_backed)
            }
            Self::Tuple(_)
            | Self::Array(_)
            | Self::Map { .. }
            | Self::Set(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::Trait(_)
            | Self::Host(_)
            | Self::Generic(_)
            | Self::SelfType(_)
            | Self::StandardEnum { .. } => true,
        }
    }
}
