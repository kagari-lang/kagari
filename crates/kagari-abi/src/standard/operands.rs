//! Operand counts of the engine operation registry, including internal helpers.
//! Public declarations are checked by HIR; this is an executable shape check.

use crate::standard::StandardIntrinsic;

impl StandardIntrinsic {
    pub fn operand_count(self) -> usize {
        match self {
            Self::ArrayListNew | Self::LinkedHashMapNew | Self::LinkedHashSetNew => 0,
            Self::ArrayCapacity
            | Self::ArrayClear
            | Self::ArrayDedup
            | Self::ArrayIsEmpty
            | Self::ArrayLen
            | Self::ArrayListFrom
            | Self::ArrayPop
            | Self::ArrayReverse
            | Self::ArraySort
            | Self::ArrayWithCapacity
            | Self::DebugPanic
            | Self::DebugPrint
            | Self::LinkedHashMapFrom
            | Self::LinkedHashSetFrom
            | Self::MapCapacity
            | Self::MapClear
            | Self::MapEntries
            | Self::MapIsEmpty
            | Self::MapKeys
            | Self::MapLen
            | Self::MapValues
            | Self::MapWithCapacity
            | Self::MathAbs
            | Self::MathCeil
            | Self::MathCos
            | Self::MathFloor
            | Self::MathRound
            | Self::MathSin
            | Self::MathSqrt
            | Self::MathTan
            | Self::OptionFlatten
            | Self::OptionIsNone
            | Self::OptionIsSome
            | Self::OptionTranspose
            | Self::ResultErr
            | Self::ResultFlatten
            | Self::ResultIsErr
            | Self::ResultIsOk
            | Self::ResultOk
            | Self::ResultTranspose
            | Self::SetCapacity
            | Self::SetClear
            | Self::SetIsEmpty
            | Self::SetLen
            | Self::SetToArray
            | Self::SetWithCapacity
            | Self::StringBytes
            | Self::StringCharIndices
            | Self::StringIsAscii
            | Self::StringIsEmpty
            | Self::StringLenBytes
            | Self::StringLenChars
            | Self::StringLines
            | Self::StringParse
            | Self::StringSplitWhitespace
            | Self::StringToAsciiLowercase
            | Self::StringToAsciiUppercase
            | Self::StringToLowercase
            | Self::StringToUppercase
            | Self::StringTrim
            | Self::StringTrimEnd
            | Self::StringTrimStart
            | Self::ParseNumber(_)
            | Self::MapKeysStorage
            | Self::MapValuesStorage
            | Self::MapEntriesStorage
            | Self::KeyLookupBegin
            | Self::CollectionMutationBegin
            | Self::CollectionMutationEnd
            | Self::IterResume
            | Self::ValueHash
            | Self::ValueDebug
            | Self::ValueDisplay => 1,
            Self::ArrayCopyFrom
            | Self::ArrayExtend
            | Self::ArrayFill
            | Self::ArrayGet
            | Self::ArrayJoin
            | Self::ArrayListFromFn
            | Self::ArrayPush
            | Self::ArrayRemove
            | Self::ArrayRemoveRange
            | Self::ArrayReserve
            | Self::ArrayRetain
            | Self::ArraySortBy
            | Self::ArraySortByKey
            | Self::ArraySwapRemove
            | Self::ArrayTruncate
            | Self::DebugAssert
            | Self::Integer(_, _)
            | Self::MapContainsKey
            | Self::MapGet
            | Self::MapRemove
            | Self::MapReserve
            | Self::MapRetain
            | Self::MathMax
            | Self::MathMin
            | Self::OptionAndThen
            | Self::OptionFilter
            | Self::OptionIsSomeAnd
            | Self::OptionMap
            | Self::OptionOkOr
            | Self::OptionOkOrElse
            | Self::OptionOrElse
            | Self::OptionUnwrapOr
            | Self::OptionUnwrapOrElse
            | Self::OptionZip
            | Self::ParseRadix(_)
            | Self::ResultAndThen
            | Self::ResultIsErrAnd
            | Self::ResultIsOkAnd
            | Self::ResultMap
            | Self::ResultMapErr
            | Self::ResultOrElse
            | Self::ResultUnwrapOr
            | Self::ResultUnwrapOrElse
            | Self::SetContains
            | Self::SetInsert
            | Self::SetRemove
            | Self::SetReserve
            | Self::SetRetain
            | Self::StringConcat
            | Self::StringContains
            | Self::StringEndsWith
            | Self::StringEqIgnoreAsciiCase
            | Self::StringFind
            | Self::StringIsCharBoundary
            | Self::StringRepeat
            | Self::StringRfind
            | Self::StringRsplitOnce
            | Self::StringSplit
            | Self::StringSplitOnce
            | Self::StringStartsWith
            | Self::StringStripPrefix
            | Self::StringStripSuffix
            | Self::ArrayCopyFromStorage
            | Self::ArrayExtendStorage
            | Self::ArrayReplaceStorage
            | Self::CollectionRetainStorage
            | Self::KeyCandidates
            | Self::ValueEq
            | Self::ValuePartialCmp
            | Self::ValueCmp => 2,
            Self::ArrayCopyWithin
            | Self::ArrayInsert
            | Self::ArraySwap
            | Self::DebugAssertEq
            | Self::MapGetOrInsertWith
            | Self::MapInsert
            | Self::MapUpdate
            | Self::MathClamp
            | Self::OptionMapOr
            | Self::OptionMapOrElse
            | Self::ResultMapOr
            | Self::ResultMapOrElse
            | Self::StringReplace
            | Self::StringSlice
            | Self::StringSplitN
            | Self::ArrayRemoveRangePrepare
            | Self::KeyMapGet
            | Self::KeyMapRemove
            | Self::KeySetContains
            | Self::KeySetRemove => 3,
            Self::StringReplaceN | Self::ArrayCopyWithinBounds | Self::KeySetInsert => 4,
            Self::KeyMapInsert => 5,
        }
    }
}
