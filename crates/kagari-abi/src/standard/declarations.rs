//! Portable declaration descriptors generated from the bundled standard sources.
use super::surface;
use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiItem {
    pub module: &'static str,
    pub uri: &'static str,
    pub path: &'static [(DefinitionKind, &'static str)],
    pub start: usize,
    pub end: usize,
    pub documentation: &'static str,
    pub signature: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiBound {
    pub name: &'static str,
    pub args: &'static [ApiType],
    pub bindings: &'static [(&'static str, ApiType)],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiAssociatedType {
    pub item: ApiItem,
    pub bounds: &'static [ApiBound],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiMethod {
    pub item: ApiItem,
    pub native_default: Option<NativeDefaultMethod>,
    pub generics: &'static [ApiGeneric],
    pub bounds: &'static [(ApiType, &'static [ApiBound])],
    pub params: &'static [ApiParameter],
    pub result: ApiType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiGeneric {
    pub name: &'static str,
    pub bounds: &'static [ApiBound],
    pub projection_key: &'static str,
}

/// Native defaults retain ordinary trait identities and checked generic signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeDefaultMethod {
    Join,
    ListJoin,
    ListWindows,
    ListChunks,
    ListFirst,
    ListLast,
    ListContains,
    ListStartsWith,
    ListEndsWith,
    ListBinarySearch,

    SetUnion,
    SetIntersection,
    SetDifference,
    SetSymmetricDifference,
    SetIsSubset,
    SetIsSuperset,
    SetIsDisjoint,
    MapKeysView,
    MapValuesView,
    MapEntriesView,
    Collect,
    Sum,
    Product,
    FlatMap,
    Flatten,
    TakeWhile,
    SkipWhile,
    Inspect,
    Fuse,
    FindMap,
    Position,
    Nth,
    Last,
    Reduce,
    Min,
    Max,
    MinByKey,
    MaxByKey,
    MinBy,
    MaxBy,

    Map,
    Filter,
    FilterMap,
    Take,
    Skip,
    Enumerate,
    Zip,
    Chain,
    Find,
    Any,
    All,
    Count,
    Fold,
    ForEach,
    Partition,
    GroupBy,
}

pub fn native_default_method(id: &DefinitionId) -> Option<NativeDefaultMethod> {
    surface::STANDARD_TRAITS
        .iter()
        .flat_map(|t| t.methods)
        .find(|m| m.item.identity() == *id)
        .and_then(|m| m.native_default)
}

pub fn native_trait_default(interface: &DefinitionId, name: &str) -> bool {
    surface::STANDARD_TRAITS
        .iter()
        .find(|t| t.item.identity() == *interface)
        .is_some_and(|t| {
            t.methods.iter().any(|m| {
                m.item.path.last().is_some_and(|p| p.1 == name) && m.native_default.is_some()
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiTrait {
    pub item: ApiItem,
    pub generics: &'static [&'static str],
    pub supertraits: &'static [ApiBound],
    pub associated_types: &'static [ApiAssociatedType],
    pub methods: &'static [ApiMethod],
}

/// An explicit native trait implementation read from the bundled source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiImplementation {
    pub interface: &'static str,
    pub trait_arguments: &'static [ApiType],
    pub bounds: &'static [(ApiType, &'static [ApiBound])],
    pub generics: &'static [&'static str],
    pub target: ApiType,
    pub associated_types: &'static [(ApiItem, ApiType)],
    pub methods: &'static [ApiMethod],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiType {
    Named(&'static str, &'static [ApiType]),
    Array(&'static ApiType),
    Tuple(&'static [ApiType]),
    Function(&'static [ApiType], &'static ApiType),
    Projection(&'static ApiType, &'static ApiBound, &'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiParameter {
    pub name: &'static str,
    pub ty: ApiType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiFunction {
    pub bounds: &'static [(ApiType, &'static [ApiBound])],
    pub qualified_name: &'static str,
    pub uri: &'static str,
    pub start: usize,
    pub end: usize,
    pub documentation: &'static str,
    pub signature: &'static str,
    pub params: &'static [ApiParameter],
    pub result: ApiType,
}
impl ApiItem {
    pub fn identity(&self) -> DefinitionId {
        DefinitionId {
            module: ModuleIdentity {
                package: PackageId("kagari-std".into()),
                path: vec![self.module.into()],
            },
            path: self
                .path
                .iter()
                .map(|(kind, name)| DefinitionPathSegment {
                    kind: *kind,
                    name: (*name).into(),
                    occurrence: 0,
                })
                .collect(),
        }
    }
}
impl ApiImplementation {
    pub fn trait_declaration(&self) -> &'static ApiTrait {
        surface::STANDARD_TRAITS
            .iter()
            .find(|t| t.item.path.last().unwrap().1 == self.interface)
            .expect("validated native trait declaration")
    }
}
