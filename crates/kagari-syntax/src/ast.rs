#[derive(Debug, Clone, Default)]
pub struct Module {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Function(Function),
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Parameter>,
    pub return_type: Option<TypeRef>,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub ty: TypeRef,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub name: String,
}
