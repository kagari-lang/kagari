//! Physical value categories; these never prove a script nominal type.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKind {
    String,
    Tuple,
    Range,
    HostRoot,
    HostPath,
    Native,

    Array,

    Enum,
    Struct,
    Interface,
    Closure,
    Cell,
}
