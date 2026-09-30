//! Closed operation identities for native trait implementations and defaults.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeProtocolMethod {
    CollectionFromIterator,
    CollectionIter,
    CollectionSet,
    IterNext,
    NumericFromStr,
    OptionFromIterator,
    RangeEndBound,
    RangeStartBound,
    ResultFromIterator,
    NumericSum,
    NumericProduct,
}

/// Native defaults retain ordinary trait identities and checked generic signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeDefaultMethod {
    #[serde(rename = "IteratorJoin")]
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
    #[serde(rename = "IteratorCollect")]
    Collect,
    #[serde(rename = "IteratorSum")]
    Sum,
    #[serde(rename = "IteratorProduct")]
    Product,
    #[serde(rename = "IteratorFlatMap")]
    FlatMap,
    #[serde(rename = "IteratorFlatten")]
    Flatten,
    #[serde(rename = "IteratorTakeWhile")]
    TakeWhile,
    #[serde(rename = "IteratorSkipWhile")]
    SkipWhile,
    #[serde(rename = "IteratorInspect")]
    Inspect,
    #[serde(rename = "IteratorFuse")]
    Fuse,
    #[serde(rename = "IteratorFindMap")]
    FindMap,
    #[serde(rename = "IteratorPosition")]
    Position,
    #[serde(rename = "IteratorNth")]
    Nth,
    #[serde(rename = "IteratorLast")]
    Last,
    #[serde(rename = "IteratorReduce")]
    Reduce,
    #[serde(rename = "IteratorMin")]
    Min,
    #[serde(rename = "IteratorMax")]
    Max,
    #[serde(rename = "IteratorMinByKey")]
    MinByKey,
    #[serde(rename = "IteratorMaxByKey")]
    MaxByKey,
    #[serde(rename = "IteratorMinBy")]
    MinBy,
    #[serde(rename = "IteratorMaxBy")]
    MaxBy,

    #[serde(rename = "IteratorMap")]
    Map,
    #[serde(rename = "IteratorFilter")]
    Filter,
    #[serde(rename = "IteratorFilterMap")]
    FilterMap,
    #[serde(rename = "IteratorTake")]
    Take,
    #[serde(rename = "IteratorSkip")]
    Skip,
    #[serde(rename = "IteratorEnumerate")]
    Enumerate,
    #[serde(rename = "IteratorZip")]
    Zip,
    #[serde(rename = "IteratorChain")]
    Chain,
    #[serde(rename = "IteratorFind")]
    Find,
    #[serde(rename = "IteratorAny")]
    Any,
    #[serde(rename = "IteratorAll")]
    All,
    #[serde(rename = "IteratorCount")]
    Count,
    #[serde(rename = "IteratorFold")]
    Fold,
    #[serde(rename = "IteratorForEach")]
    ForEach,
    #[serde(rename = "IteratorPartition")]
    Partition,
    #[serde(rename = "IteratorGroupBy")]
    GroupBy,
}
