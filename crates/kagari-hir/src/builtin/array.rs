use super::{BuiltinMethod, BuiltinMethodSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    Len,
    Push,
    Pop,
}

const LEN_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "len",
    arity: 0,
};

const PUSH_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "push",
    arity: 1,
};

const POP_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "pop",
    arity: 0,
};

pub fn lookup_method(name: &str) -> Option<BuiltinMethod> {
    match name {
        "len" => Some(BuiltinMethod::Array(Method::Len)),
        "push" => Some(BuiltinMethod::Array(Method::Push)),
        "pop" => Some(BuiltinMethod::Array(Method::Pop)),
        _ => None,
    }
}

pub fn method_spec(method: Method) -> &'static BuiltinMethodSpec {
    match method {
        Method::Len => &LEN_SPEC,
        Method::Push => &PUSH_SPEC,
        Method::Pop => &POP_SPEC,
    }
}
