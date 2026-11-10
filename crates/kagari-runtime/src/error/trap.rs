//! Script traps independent of diagnostic formatting and execution context.
use std::borrow::Cow;

/// A fixed Runtime trap or an application/library supplied diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrapReason {
    #[error("Future has already been awaited")]
    FutureHasAlreadyBeenAwaited,

    #[error("Task wait cycle")]
    TaskWaitCycle,

    #[error("arithmetic requires matching numeric operands")]
    ArithmeticRequiresMatchingNumericOperands,

    #[error("array copy requires equal lengths")]
    ArrayCopyRequiresEqualLengths,

    #[error("array.join expects a string array and separator")]
    ArrayJoinExpectsAStringArrayAndSeparator,

    #[error("array.join expects a valid array handle")]
    ArrayJoinExpectsAValidArrayHandle,

    #[error("array.join expects string elements")]
    ArrayJoinExpectsStringElements,

    #[error("async state retains an invalid or scoped value")]
    AsyncStateRetainsAnInvalidOrScopedValue,

    #[error("closure call contract")]
    ClosureCallContract,

    #[error("closure call contract mismatch")]
    ClosureCallContractMismatch,

    #[error("collection key protocol mismatch; custom keys require script protocol execution")]
    CollectionKeyProtocolMismatchCustomKeysRequireScriptProtocolExecution,

    #[error("comparison requires two operands")]
    ComparisonRequiresTwoOperands,

    #[error("container mutation during a guarded callback")]
    ContainerMutationDuringAGuardedCallback,

    #[error("container mutation during key comparison or hashing")]
    ContainerMutationDuringKeyComparisonOrHashing,

    #[error("debug.assert expects bool and string message")]
    DebugAssertExpectsBoolAndStringMessage,

    #[error("duplicate Future operands")]
    DuplicateFutureOperands,

    #[error("eq expects two operands")]
    EqExpectsTwoOperands,

    #[error("expected a builtin array object")]
    ExpectedABuiltinArrayObject,

    #[error("expected arithmetic operation")]
    ExpectedArithmeticOperation,

    #[error("expected capture cell")]
    ExpectedCaptureCell,

    #[error("expected closure")]
    ExpectedClosure,

    #[error("expected interface value")]
    ExpectedInterfaceValue,

    #[error("field is read-only or value has the wrong concrete type")]
    FieldIsReadOnlyOrValueHasTheWrongConcreteType,

    #[error("format expects one operand")]
    FormatExpectsOneOperand,

    #[error("hash expects one operand")]
    HashExpectsOneOperand,

    #[error("hash storage type or selected key protocol mismatch")]
    HashStorageTypeOrSelectedKeyProtocolMismatch,

    #[error("identity comparison requires object references")]
    IdentityComparisonRequiresObjectReferences,

    #[error("integer method requires two arguments")]
    IntegerMethodRequiresTwoArguments,

    #[error("integer receiver required")]
    IntegerReceiverRequired,

    #[error("interface method argument does not match its linked signature")]
    InterfaceMethodArgumentDoesNotMatchItsLinkedSignature,

    #[error("interface method result does not match its linked signature")]
    InterfaceMethodResultDoesNotMatchItsLinkedSignature,

    #[error("interface method unavailable")]
    InterfaceMethodUnavailable,

    #[error("invalid array target or index")]
    InvalidArrayTargetOrIndex,

    #[error("invalid array target or payload")]
    InvalidArrayTargetOrPayload,

    #[error("invalid assertion message")]
    InvalidAssertionMessage,

    #[error("invalid builtin ordering operands")]
    InvalidBuiltinOrderingOperands,

    #[error("invalid capacity receiver")]
    InvalidCapacityReceiver,

    #[error("invalid capture cell")]
    InvalidCaptureCell,

    #[error("invalid cell value")]
    InvalidCellValue,

    #[error("invalid closure capture")]
    InvalidClosureCapture,

    #[error("invalid closure handle")]
    InvalidClosureHandle,

    #[error("invalid custom key operation")]
    InvalidCustomKeyOperation,

    #[error("invalid field slot")]
    InvalidFieldSlot,

    #[error("invalid hash key")]
    InvalidHashKey,

    #[error("invalid heap handle in equality")]
    InvalidHeapHandleInEquality,

    #[error("invalid heap handle in identity comparison")]
    InvalidHeapHandleInIdentityComparison,

    #[error("invalid heap payload")]
    InvalidHeapPayload,

    #[error("invalid heap reference at safepoint")]
    InvalidHeapReferenceAtSafepoint,

    #[error("invalid heap reference in debug value")]
    InvalidHeapReferenceInDebugValue,

    #[error("invalid heap target")]
    InvalidHeapTarget,

    #[error("invalid heap target, index, or payload")]
    InvalidHeapTargetIndexOrPayload,

    #[error("invalid interface handle")]
    InvalidInterfaceHandle,

    #[error("invalid interface receiver")]
    InvalidInterfaceReceiver,

    #[error("invalid interface upcast")]
    InvalidInterfaceUpcast,

    #[error("invalid mutation root")]
    InvalidMutationRoot,

    #[error("invalid mutation target")]
    InvalidMutationTarget,

    #[error("invalid numeric conversion")]
    InvalidNumericConversion,

    #[error("invalid numeric operand")]
    InvalidNumericOperand,

    #[error("invalid or foreign heap payload")]
    InvalidOrForeignHeapPayload,

    #[error("invalid or structurally modified iterator")]
    InvalidOrStructurallyModifiedIterator,

    #[error("invalid prepared collection")]
    InvalidPreparedCollection,

    #[error("invalid range")]
    InvalidRange,

    #[error("invalid range value")]
    InvalidRangeValue,

    #[error("invalid sequence buffer or payload")]
    InvalidSequenceBufferOrPayload,

    #[error("invalid string element")]
    InvalidStringElement,

    #[error("invalid string separator")]
    InvalidStringSeparator,

    #[error("invalid struct")]
    InvalidStruct,

    #[error("invalid struct field")]
    InvalidStructField,

    #[error("invalid tuple")]
    InvalidTuple,

    #[error("invalid unary operand")]
    InvalidUnaryOperand,

    #[error("mutation guard allocation")]
    MutationGuardAllocation,

    #[error("mutation guard mismatch")]
    MutationGuardMismatch,

    #[error("reflect_get_field expects struct value")]
    ReflectGetFieldExpectsStructValue,

    #[error("reflect_set_field expects default-storable value")]
    ReflectSetFieldExpectsDefaultStorableValue,

    #[error("reflect_set_field expects struct value")]
    ReflectSetFieldExpectsStructValue,

    #[error("reflect_set_index expects array or tuple value")]
    ReflectSetIndexExpectsArrayOrTupleValue,

    #[error("reflect_set_index expects default-storable value")]
    ReflectSetIndexExpectsDefaultStorableValue,

    #[error("reflect_set_index expects non-negative integer index")]
    ReflectSetIndexExpectsNonNegativeIntegerIndex,

    #[error("runtime driver already active")]
    RuntimeDriverAlreadyActive,

    #[error("signed-offset wrapping requires an unsigned receiver")]
    SignedOffsetWrappingRequiresAnUnsignedReceiver,

    #[error("sort comparator does not define a consistent order")]
    SortComparatorDoesNotDefineAConsistentOrder,

    #[error("string interpolation requires [String] storage")]
    StringInterpolationRequiresStringStorage,

    #[error("struct layout mismatch")]
    StructLayoutMismatch,

    #[error("total comparison cannot be unordered")]
    TotalComparisonCannotBeUnordered,

    #[error("unsupported format value, invalid handle, or formatting limit exceeded")]
    UnsupportedFormatValueInvalidHandleOrFormattingLimitExceeded,

    #[error("value category does not support general equality")]
    ValueCategoryDoesNotSupportGeneralEquality,

    #[error("value has no hash semantics or exceeds key size limit")]
    ValueHasNoHashSemanticsOrExceedsKeySizeLimit,

    #[error("debug.assert failed: {message}")]
    DebugAssertion { message: String },

    #[error("invalid index `{index}`")]
    InvalidReflectiveIndex { index: u64 },

    #[error("missing field {name}")]
    MissingField { name: String },

    /// Custom text supplied by a Native provider, library or assertion.
    #[error("{0}")]
    Provider(Cow<'static, str>),
}

impl TrapReason {
    pub(crate) fn retained_bytes(&self) -> usize {
        match self {
            Self::Provider(Cow::Owned(text))
            | Self::DebugAssertion { message: text }
            | Self::MissingField { name: text } => text.capacity(),
            _ => 0,
        }
    }
}
