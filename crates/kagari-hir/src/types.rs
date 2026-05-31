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
    Builtin(BuiltinType),
    Tuple(Vec<TypeId>),
    Array(Box<TypeId>),
    Struct(String),
    Enum(String),
    StandardEnum { name: String, args: Vec<TypeId> },
}

impl TypeId {
    pub fn from_name(name: &str) -> Option<Self> {
        crate::builtin::surface::builtin_type(name).map(Self::Builtin)
    }

    pub fn display_name(&self) -> String {
        match self {
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
            Self::Struct(name) | Self::Enum(name) => name.clone(),
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
            Self::Builtin(ty) => {
                crate::builtin::surface::builtin_type_spec(*ty).is_some_and(|spec| spec.heap_backed)
            }
            Self::Tuple(_)
            | Self::Array(_)
            | Self::Struct(_)
            | Self::Enum(_)
            | Self::StandardEnum { .. } => true,
        }
    }
}
