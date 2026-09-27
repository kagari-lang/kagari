use crate::standard::StandardIntrinsic;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    pub reads_local: bool,
    pub writes_local: bool,
    pub reads_module: bool,
    pub writes_module: bool,
    pub reads_aggregate: bool,
    pub writes_aggregate: bool,
    pub reads_path: bool,
    pub writes_path: bool,
    pub allocates: bool,
    pub calls: bool,
    pub touches_runtime: bool,
    pub may_trap: bool,
}

impl EffectSet {
    pub fn union(self, other: Self) -> Self {
        Self {
            reads_local: self.reads_local || other.reads_local,
            writes_local: self.writes_local || other.writes_local,
            reads_module: self.reads_module || other.reads_module,
            writes_module: self.writes_module || other.writes_module,
            reads_aggregate: self.reads_aggregate || other.reads_aggregate,
            writes_aggregate: self.writes_aggregate || other.writes_aggregate,
            reads_path: self.reads_path || other.reads_path,
            writes_path: self.writes_path || other.writes_path,
            allocates: self.allocates || other.allocates,
            calls: self.calls || other.calls,
            touches_runtime: self.touches_runtime || other.touches_runtime,
            may_trap: self.may_trap || other.may_trap,
        }
    }

    pub fn local_read() -> Self {
        Self {
            reads_local: true,
            ..Self::default()
        }
    }

    pub fn local_write() -> Self {
        Self {
            writes_local: true,
            ..Self::default()
        }
    }

    pub fn aggregate_read() -> Self {
        Self {
            reads_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_read() -> Self {
        Self {
            reads_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn module_write() -> Self {
        Self {
            writes_module: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn aggregate_write() -> Self {
        Self {
            writes_aggregate: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_read() -> Self {
        Self {
            reads_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn path_write() -> Self {
        Self {
            writes_path: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn allocation() -> Self {
        Self {
            allocates: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn call() -> Self {
        Self {
            calls: true,
            may_trap: true,
            ..Self::default()
        }
    }

    pub fn runtime_call() -> Self {
        Self {
            calls: true,
            touches_runtime: true,
            may_trap: true,
            ..Self::default()
        }
    }
}

pub fn standard_intrinsic_effects(intrinsic: StandardIntrinsic) -> EffectSet {
    let runtime_read = EffectSet::runtime_call().union(EffectSet::aggregate_read());
    let mutating = matches!(
        intrinsic,
        StandardIntrinsic::KeyMapInsert
            | StandardIntrinsic::KeyMapRemove
            | StandardIntrinsic::KeySetInsert
            | StandardIntrinsic::KeySetRemove
            | StandardIntrinsic::ArrayPush
            | StandardIntrinsic::ArrayPop
            | StandardIntrinsic::ArrayInsert
            | StandardIntrinsic::ArrayReserve
            | StandardIntrinsic::MapReserve
            | StandardIntrinsic::SetReserve
            | StandardIntrinsic::ArraySwap
            | StandardIntrinsic::ArrayReverse
            | StandardIntrinsic::ArrayTruncate
            | StandardIntrinsic::ArrayExtend
            | StandardIntrinsic::ArrayExtendStorage
            | StandardIntrinsic::ArraySwapRemove
            | StandardIntrinsic::ArrayRemove
            | StandardIntrinsic::ArrayClear
            | StandardIntrinsic::ArrayFill
            | StandardIntrinsic::ArrayCopyFrom
            | StandardIntrinsic::ArrayCopyFromStorage
            | StandardIntrinsic::ArrayRemoveRange
            | StandardIntrinsic::ArrayCopyWithin
            | StandardIntrinsic::ArrayCopyWithinBounds
            | StandardIntrinsic::ArrayRetain
            | StandardIntrinsic::MapRetain
            | StandardIntrinsic::SetRetain
            | StandardIntrinsic::ArraySort
            | StandardIntrinsic::ArraySortBy
            | StandardIntrinsic::ArraySortByKey
            | StandardIntrinsic::ArrayDedup
            | StandardIntrinsic::ArrayReplaceStorage
            | StandardIntrinsic::CollectionRetainStorage
            | StandardIntrinsic::CollectionMutationBegin
            | StandardIntrinsic::CollectionMutationEnd
            | StandardIntrinsic::MapGetOrInsertWith
            | StandardIntrinsic::MapUpdate
            | StandardIntrinsic::MapInsert
            | StandardIntrinsic::MapRemove
            | StandardIntrinsic::MapClear
            | StandardIntrinsic::SetInsert
            | StandardIntrinsic::SetRemove
            | StandardIntrinsic::SetClear
    );
    let allocating = matches!(intrinsic, StandardIntrinsic::Integer(method, _) if method.allocates())
        || matches!(
            intrinsic,
            StandardIntrinsic::ArrayRemoveRangePrepare
                | StandardIntrinsic::KeyCandidates
                | StandardIntrinsic::KeyMapGet
                | StandardIntrinsic::KeyMapRemove
                | StandardIntrinsic::ArrayGet
                | StandardIntrinsic::ArrayPop
                | StandardIntrinsic::ArrayReserve
                | StandardIntrinsic::MapReserve
                | StandardIntrinsic::SetReserve
                | StandardIntrinsic::ArraySwap
                | StandardIntrinsic::ArrayReverse
                | StandardIntrinsic::ArrayTruncate
                | StandardIntrinsic::ArrayExtend
                | StandardIntrinsic::ArrayExtendStorage
                | StandardIntrinsic::ArraySwapRemove
                | StandardIntrinsic::ArrayRemove
                | StandardIntrinsic::ArrayWithCapacity
                | StandardIntrinsic::MapWithCapacity
                | StandardIntrinsic::SetWithCapacity
                | StandardIntrinsic::ArrayListNew
                | StandardIntrinsic::LinkedHashMapNew
                | StandardIntrinsic::LinkedHashSetNew
                | StandardIntrinsic::ArrayListFrom
                | StandardIntrinsic::LinkedHashMapFrom
                | StandardIntrinsic::LinkedHashSetFrom
                | StandardIntrinsic::MapGet
                | StandardIntrinsic::MapRemove
                | StandardIntrinsic::MapKeys
                | StandardIntrinsic::MapValues
                | StandardIntrinsic::MapEntries
                | StandardIntrinsic::MapKeysStorage
                | StandardIntrinsic::MapValuesStorage
                | StandardIntrinsic::MapEntriesStorage
                | StandardIntrinsic::SetToArray
                | StandardIntrinsic::ArrayJoin
                | StandardIntrinsic::StringParse
                | StandardIntrinsic::ParseNumber(_)
                | StandardIntrinsic::ParseRadix(_)
                | StandardIntrinsic::StringSlice
                | StandardIntrinsic::StringReplace
                | StandardIntrinsic::StringReplaceN
                | StandardIntrinsic::StringRepeat
                | StandardIntrinsic::StringToAsciiLowercase
                | StandardIntrinsic::StringToAsciiUppercase
                | StandardIntrinsic::StringToLowercase
                | StandardIntrinsic::StringToUppercase
                | StandardIntrinsic::StringBytes
                | StandardIntrinsic::StringCharIndices
                | StandardIntrinsic::StringSplit
                | StandardIntrinsic::StringSplitN
                | StandardIntrinsic::StringSplitOnce
                | StandardIntrinsic::StringRsplitOnce
                | StandardIntrinsic::StringSplitWhitespace
                | StandardIntrinsic::StringLines
                | StandardIntrinsic::StringTrim
                | StandardIntrinsic::StringTrimStart
                | StandardIntrinsic::StringTrimEnd
                | StandardIntrinsic::StringFind
                | StandardIntrinsic::StringRfind
                | StandardIntrinsic::StringStripPrefix
                | StandardIntrinsic::StringStripSuffix
                | StandardIntrinsic::OptionUnwrapOrElse
                | StandardIntrinsic::OptionOrElse
                | StandardIntrinsic::OptionMapOr
                | StandardIntrinsic::OptionMapOrElse
                | StandardIntrinsic::OptionFilter
                | StandardIntrinsic::OptionIsSomeAnd
                | StandardIntrinsic::OptionZip
                | StandardIntrinsic::OptionFlatten
                | StandardIntrinsic::OptionTranspose
                | StandardIntrinsic::ResultUnwrapOrElse
                | StandardIntrinsic::ResultOrElse
                | StandardIntrinsic::ResultMapOr
                | StandardIntrinsic::ResultMapOrElse
                | StandardIntrinsic::ResultOk
                | StandardIntrinsic::ResultErr
                | StandardIntrinsic::ResultIsOkAnd
                | StandardIntrinsic::ResultIsErrAnd
                | StandardIntrinsic::ResultFlatten
                | StandardIntrinsic::ResultTranspose
                | StandardIntrinsic::OptionMap
                | StandardIntrinsic::OptionAndThen
                | StandardIntrinsic::OptionOkOr
                | StandardIntrinsic::OptionOkOrElse
                | StandardIntrinsic::ResultMap
                | StandardIntrinsic::ResultMapErr
                | StandardIntrinsic::ResultAndThen
        );

    let mut effects = match intrinsic {
        StandardIntrinsic::MathMin
        | StandardIntrinsic::MathMax
        | StandardIntrinsic::MathClamp
        | StandardIntrinsic::MathAbs
        | StandardIntrinsic::MathFloor
        | StandardIntrinsic::MathCeil
        | StandardIntrinsic::MathRound
        | StandardIntrinsic::MathSqrt
        | StandardIntrinsic::MathSin
        | StandardIntrinsic::MathCos
        | StandardIntrinsic::MathTan => EffectSet::runtime_call(),
        StandardIntrinsic::DebugPrint
        | StandardIntrinsic::DebugAssert
        | StandardIntrinsic::DebugAssertEq
        | StandardIntrinsic::DebugPanic => EffectSet::runtime_call(),
        _ => runtime_read,
    };
    if mutating {
        effects = effects.union(EffectSet::aggregate_write());
    }
    if allocating {
        effects = effects.union(EffectSet::allocation());
    }
    effects
}
