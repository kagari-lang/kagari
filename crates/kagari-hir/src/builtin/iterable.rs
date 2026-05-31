use super::{BuiltinMethod, BuiltinMethodResult, BuiltinMethodSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Method {
    Len,
    Get,
}

const LEN_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "len",
    arity: 0,
    result: BuiltinMethodResult::Builtin(crate::types::BuiltinType::USize),
    mutates_receiver: false,
};

const GET_SPEC: BuiltinMethodSpec = BuiltinMethodSpec {
    name: "get",
    arity: 1,
    result: BuiltinMethodResult::IterableElement,
    mutates_receiver: false,
};

pub fn method_spec(method: Method) -> &'static BuiltinMethodSpec {
    match method {
        Method::Len => &LEN_SPEC,
        Method::Get => &GET_SPEC,
    }
}

pub fn builtin_method(method: Method) -> BuiltinMethod {
    BuiltinMethod::Iterable(method)
}
