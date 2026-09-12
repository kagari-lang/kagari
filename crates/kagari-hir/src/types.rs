#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    Struct(String),
    Enum(String),
    Trait(String),
    Generic(String),
    StandardEnum {
        name: String,
        args: Vec<TypeId>,
    },
}

impl TypeId {
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
            Self::Unknown | Self::Error | Self::Trait(_) | Self::Generic(_) => false,
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
            Self::Unknown | Self::Error => true,
            Self::Tuple(elements) | Self::StandardEnum { args: elements, .. } => {
                elements.iter().any(Self::is_unresolved)
            }
            Self::Array(element) | Self::Set(element) => element.is_unresolved(),
            Self::Map { key, value } => key.is_unresolved() || value.is_unresolved(),
            _ => false,
        }
    }

    pub fn conflicts_with(&self, other: &Self) -> bool {
        !self.is_unresolved() && !other.is_unresolved() && self != other
    }

    pub fn from_name(name: &str) -> Option<Self> {
        crate::builtin::surface::builtin_type(name).map(Self::Builtin)
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Unknown => "<unknown>".to_owned(),
            Self::Error => "<error>".to_owned(),
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
            Self::Struct(name) | Self::Enum(name) | Self::Trait(name) | Self::Generic(name) => {
                name.clone()
            }
            Self::StandardEnum { name, args } => {
                let inner = args
                    .iter()
                    .map(TypeId::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{inner}>")
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
            | Self::Generic(_)
            | Self::StandardEnum { .. } => true,
        }
    }
}
