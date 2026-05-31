#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleAbi {
    pub public_items: PublicAbiItemBuffer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicAbiItem {
    Function(FunctionAbi),
    Const(ConstAbi),
    Type(TypeAbi),
    Trait(TraitAbi),
    InterfaceTable(InterfaceTableAbi),
}

impl PublicAbiItem {
    pub fn name(&self) -> &str {
        match self {
            Self::Function(item) => &item.name,
            Self::Const(item) => &item.name,
            Self::Type(item) => &item.name,
            Self::Trait(item) => &item.name,
            Self::InterfaceTable(item) => &item.name,
        }
    }

    pub fn category(&self) -> &'static str {
        match self {
            Self::Function(_) => "function",
            Self::Const(_) => "const",
            Self::Type(_) => "type",
            Self::Trait(_) => "trait",
            Self::InterfaceTable(_) => "interface_table",
        }
    }

    pub fn fingerprint_name(&self) -> String {
        format!("{}:{}", self.category(), self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionAbi {
    pub name: String,
    pub generic_params: Vec<String>,
    pub bounds: Vec<String>,
    pub params: Vec<ParameterAbi>,
    pub return_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterAbi {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstAbi {
    pub name: String,
    pub ty: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeAbi {
    pub name: String,
    pub kind: TypeAbiKind,
    pub fields: Vec<FieldAbi>,
    pub variants: Vec<VariantAbi>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeAbiKind {
    Struct,
    Enum,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldAbi {
    pub name: String,
    pub ty: String,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantAbi {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitAbi {
    pub name: String,
    pub generic_params: Vec<String>,
    pub methods: Vec<FunctionAbi>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceTableAbi {
    pub name: String,
    pub trait_name: String,
    pub for_type: String,
    pub methods: Vec<FunctionAbi>,
}

pub type PublicAbiItemBuffer = Vec<PublicAbiItem>;
