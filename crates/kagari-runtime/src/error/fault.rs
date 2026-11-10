//! Invariant failures retained by a quarantined Runtime.

/// Invariant failures retained by a quarantined Runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EngineFault {
    #[error("Task waiter wake failed")]
    TaskWaiterWakeFailed,

    #[error("active execution observer is missing")]
    ActiveExecutionObserverIsMissing,

    #[error("argument window borrowed during call entry")]
    ArgumentWindowBorrowedDuringCallEntry,

    #[error("backing charge underflow during cleanup")]
    BackingChargeUnderflowDuringCleanup,

    #[error("branch publication before retirement")]
    BranchPublicationBeforeRetirement,

    #[error("call depth underflow during frame cleanup")]
    CallDepthUnderflowDuringFrameCleanup,

    #[error("candidate session ended with nested execution scopes")]
    CandidateSessionEndedWithNestedExecutionScopes,

    #[error("compiled function returned an invalid value")]
    CompiledFunctionReturnedAnInvalidValue,

    #[error("compiled function returned an unknown status")]
    CompiledFunctionReturnedAnUnknownStatus,

    #[error("compiled function returned the wrong representation")]
    CompiledFunctionReturnedTheWrongRepresentation,

    #[error("constant initialized during exclusive execution")]
    ConstantInitializedDuringExclusiveExecution,

    #[error("constant storage borrowed across execution")]
    ConstantStorageBorrowedAcrossExecution,

    #[error("environment metadata destruction panicked")]
    EnvironmentMetadataDestructionPanicked,

    #[error("execution attempted during garbage collection")]
    ExecutionAttemptedDuringGarbageCollection,

    #[error("execution attempted during host path commit")]
    ExecutionAttemptedDuringHostPathCommit,

    #[error("execution observer borrowed across execution")]
    ExecutionObserverBorrowedAcrossExecution,

    #[error("execution observer encountered an engine fault")]
    ExecutionObserverEncounteredAnEngineFault,

    #[error("execution scope count overflow")]
    ExecutionScopeCountOverflow,

    #[error("execution session ended with active resources")]
    ExecutionSessionEndedWithActiveResources,

    #[error("execution slots borrowed across instruction")]
    ExecutionSlotsBorrowedAcrossInstruction,

    #[error("execution slots borrowed during constant publication")]
    ExecutionSlotsBorrowedDuringConstantPublication,

    #[error("execution slots borrowed during frame entry")]
    ExecutionSlotsBorrowedDuringFrameEntry,

    #[error("execution slots remained borrowed during frame cleanup")]
    ExecutionSlotsRemainedBorrowedDuringFrameCleanup,

    #[error("execution used a suspended frame scope")]
    ExecutionUsedASuspendedFrameScope,

    #[error("execution used a suspended session")]
    ExecutionUsedASuspendedSession,

    #[error("execution windows borrowed during await")]
    ExecutionWindowsBorrowedDuringAwait,

    #[error("execution windows borrowed during suspension")]
    ExecutionWindowsBorrowedDuringSuspension,

    #[error("expired frame storage")]
    ExpiredFrameStorage,

    #[error("field access contract mismatch")]
    FieldAccessContractMismatch,

    #[error("field operands borrowed across transition")]
    FieldOperandsBorrowedAcrossTransition,

    #[error("field operands borrowed during publication")]
    FieldOperandsBorrowedDuringPublication,

    #[error("frame scope attempted to pop its caller")]
    FrameScopeAttemptedToPopItsCaller,

    #[error("frame scope identity exhausted")]
    FrameScopeIdentityExhausted,

    #[error("frame scopes dropped out of order")]
    FrameScopesDroppedOutOfOrder,

    #[error("frame stack is borrowed across execution")]
    FrameStackIsBorrowedAcrossExecution,

    #[error("frame stack is mutably borrowed")]
    FrameStackIsMutablyBorrowed,

    #[error("frame stack remained borrowed during cleanup")]
    FrameStackRemainedBorrowedDuringCleanup,

    #[error("frame storage borrowed during inspection")]
    FrameStorageBorrowedDuringInspection,

    #[error("garbage collection callback panicked")]
    GarbageCollectionCallbackPanicked,

    #[error("heap charge exceeds committed backing")]
    HeapChargeExceedsCommittedBacking,

    #[error("heap charge exceeds committed occupancy")]
    HeapChargeExceedsCommittedOccupancy,

    #[error("heap charge underflow during cleanup")]
    HeapChargeUnderflowDuringCleanup,

    #[error("host borrow epoch exhausted")]
    HostBorrowEpochExhausted,

    #[error("host borrow frame disappeared during cleanup")]
    HostBorrowFrameDisappearedDuringCleanup,

    #[error("host borrow frame epoch changed during cleanup")]
    HostBorrowFrameEpochChangedDuringCleanup,

    #[error("host frame identity exhausted")]
    HostFrameIdentityExhausted,

    #[error("host object borrow disappeared during cleanup")]
    HostObjectBorrowDisappearedDuringCleanup,

    #[error("host path borrow count underflow")]
    HostPathBorrowCountUnderflow,

    #[error("host path borrow disappeared during cleanup")]
    HostPathBorrowDisappearedDuringCleanup,

    #[error("host path commit panicked")]
    HostPathCommitPanicked,

    #[error("host shared borrow count exhausted")]
    HostSharedBorrowCountExhausted,

    #[error("inconsistent scalar bank lengths")]
    InconsistentScalarBankLengths,

    #[error("initializer return lost its caller")]
    InitializerReturnLostItsCaller,

    #[error("initializer return stack is borrowed")]
    InitializerReturnStackIsBorrowed,

    #[error("initializer return window was not retired")]
    InitializerReturnWindowWasNotRetired,

    #[error("interface metadata destruction panicked")]
    InterfaceMetadataDestructionPanicked,

    #[error("invalid call argument register")]
    InvalidCallArgumentRegister,

    #[error("invalid call argument window")]
    InvalidCallArgumentWindow,

    #[error("invalid constant destination")]
    InvalidConstantDestination,

    #[error("invalid constant value")]
    InvalidConstantValue,

    #[error("invalid converter state")]
    InvalidConverterState,

    #[error("invalid execution frame window")]
    InvalidExecutionFrameWindow,

    #[error("invalid execution function")]
    InvalidExecutionFunction,

    #[error("invalid execution operand slot")]
    InvalidExecutionOperandSlot,

    #[error("invalid field destination")]
    InvalidFieldDestination,

    #[error("invalid field operand")]
    InvalidFieldOperand,

    #[error("invalid field value")]
    InvalidFieldValue,

    #[error("invalid frame jump target")]
    InvalidFrameJumpTarget,

    #[error("invalid frame module")]
    InvalidFrameModule,

    #[error("invalid frame register")]
    InvalidFrameRegister,

    #[error("invalid frame register type")]
    InvalidFrameRegisterType,

    #[error("invalid native return destination")]
    InvalidNativeReturnDestination,

    #[error("invalid native return slot")]
    InvalidNativeReturnSlot,

    #[error("invalid native semantic origin")]
    InvalidNativeSemanticOrigin,

    #[error("invalid or borrowed storage in collection graph")]
    InvalidOrBorrowedStorageInCollectionGraph,

    #[error("invalid parked frame window")]
    InvalidParkedFrameWindow,

    #[error("invalid prepared callee layout")]
    InvalidPreparedCalleeLayout,

    #[error("invalid prepared field ordinal")]
    InvalidPreparedFieldOrdinal,

    #[error("invalid prepared interface upcast")]
    InvalidPreparedInterfaceUpcast,

    #[error("invalid return destination")]
    InvalidReturnDestination,

    #[error("invalid return value")]
    InvalidReturnValue,

    #[error("invalid scalar field operand")]
    InvalidScalarFieldOperand,

    #[error("invalid semantic value in static cell")]
    InvalidSemanticValueInStaticCell,

    #[error("invalid static initializer binding")]
    InvalidStaticInitializerBinding,

    #[error("invalid static return binding")]
    InvalidStaticReturnBinding,

    #[error("invalid tuple destination")]
    InvalidTupleDestination,

    #[error("invalid tuple transition")]
    InvalidTupleTransition,

    #[error("invalid tuple value")]
    InvalidTupleValue,

    #[error("invalid verified public object export")]
    InvalidVerifiedPublicObjectExport,

    #[error("loaded static binding disappeared")]
    LoadedStaticBindingDisappeared,

    #[error("method application destruction panicked")]
    MethodApplicationDestructionPanicked,

    #[error("missing constant execution link")]
    MissingConstantExecutionLink,

    #[error("missing execution frame")]
    MissingExecutionFrame,

    #[error("missing linked direct route")]
    MissingLinkedDirectRoute,

    #[error("missing linked shared contract")]
    MissingLinkedSharedContract,

    #[error("missing prepared call")]
    MissingPreparedCall,

    #[error("missing prepared native call")]
    MissingPreparedNativeCall,

    #[error("missing return frame")]
    MissingReturnFrame,

    #[error("missing scheduled execution")]
    MissingScheduledExecution,

    #[error("module metadata destruction panicked")]
    ModuleMetadataDestructionPanicked,

    #[error("module store is borrowed across execution")]
    ModuleStoreIsBorrowedAcrossExecution,

    #[error("native completion payload type")]
    NativeCompletionPayloadType,

    #[error("native completion registry is faulted")]
    NativeCompletionRegistryIsFaulted,

    #[error("native execution reported an engine fault")]
    NativeExecutionReportedAnEngineFault,

    #[error("native frame resumed before callback completion")]
    NativeFrameResumedBeforeCallbackCompletion,

    #[error("native operation cleanup failed")]
    NativeOperationCleanupFailed,

    #[error("native payload destruction panicked")]
    NativePayloadDestructionPanicked,

    #[error("native program point while frames are borrowed")]
    NativeProgramPointWhileFramesAreBorrowed,

    #[error("native program point without an execution frame")]
    NativeProgramPointWithoutAnExecutionFrame,

    #[error("native program point without an execution session")]
    NativeProgramPointWithoutAnExecutionSession,

    #[error("native return before completion")]
    NativeReturnBeforeCompletion,

    #[error("observer encountered a borrowed execution stack")]
    ObserverEncounteredABorrowedExecutionStack,

    #[error("observer initialization lost its session")]
    ObserverInitializationLostItsSession,

    #[error("observer installation encountered a borrowed stack")]
    ObserverInstallationEncounteredABorrowedStack,

    #[error("operation group destruction panicked")]
    OperationGroupDestructionPanicked,

    #[error("owned activation ended with active scopes")]
    OwnedActivationEndedWithActiveScopes,

    #[error("owned execution cleanup failed")]
    OwnedExecutionCleanupFailed,

    #[error("parked frame stack borrowed")]
    ParkedFrameStackBorrowed,

    #[error("prepared call reached canonical dispatch")]
    PreparedCallReachedCanonicalDispatch,

    #[error("prepared interface upcast reached canonical dispatch")]
    PreparedInterfaceUpcastReachedCanonicalDispatch,

    #[error("publication before invocation retirement")]
    PublicationBeforeInvocationRetirement,

    #[error("ready wait retained a pending activation")]
    ReadyWaitRetainedAPendingActivation,

    #[error("resumed frame stack borrowed")]
    ResumedFrameStackBorrowed,

    #[error("return stack is borrowed")]
    ReturnStackIsBorrowed,

    #[error("runtime shutdown cleanup failed")]
    RuntimeShutdownCleanupFailed,

    #[error("scalar region requires script frame")]
    ScalarRegionRequiresScriptFrame,

    #[error("scheduled Task failed")]
    ScheduledTaskFailed,

    #[error("scheduled execution failed")]
    ScheduledExecutionFailed,

    #[error("scheduled invocation failed")]
    ScheduledInvocationFailed,

    #[error("scheduler callback failed")]
    SchedulerCallbackFailed,

    #[error("script call requires a script caller")]
    ScriptCallRequiresAScriptCaller,

    #[error("sealed native body reached callback transition")]
    SealedNativeBodyReachedCallbackTransition,

    #[error("sequence lease restoration failed")]
    SequenceLeaseRestorationFailed,

    #[error("session records remained borrowed during cleanup")]
    SessionRecordsRemainedBorrowedDuringCleanup,

    #[error("shutdown fault test")]
    ShutdownFaultTest,

    #[error("static return lost attempt authority")]
    StaticReturnLostAttemptAuthority,

    #[error("static store borrowed during cycle detection")]
    StaticStoreBorrowedDuringCycleDetection,

    #[error("static store borrowed during initialization")]
    StaticStoreBorrowedDuringInitialization,

    #[error("static store borrowed during initializer return")]
    StaticStoreBorrowedDuringInitializerReturn,

    #[error("struct storage does not match its layout")]
    StructStorageDoesNotMatchItsLayout,

    #[error("structured branch failed")]
    StructuredBranchFailed,

    #[error("synchronous activation count overflow")]
    SynchronousActivationCountOverflow,

    #[error("synchronous activation count underflow")]
    SynchronousActivationCountUnderflow,

    #[error("task contract fault")]
    TaskContractFault,

    #[error("tuple operands borrowed during publication")]
    TupleOperandsBorrowedDuringPublication,

    #[error("verified branch condition is not bool")]
    VerifiedBranchConditionIsNotBool,

    #[error("verified call target is unsupported")]
    VerifiedCallTargetIsUnsupported,

    #[error("verified function fell through")]
    VerifiedFunctionFellThrough,

    #[error("verified function reference is missing")]
    VerifiedFunctionReferenceIsMissing,

    /// An invariant reported by a Host implementation or execution backend.
    #[error("{0}")]
    HostInvariant(&'static str),
}
