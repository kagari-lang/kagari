use crate::tests::common;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    artifact::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactSectionId,
        ArtifactValidationError, DebugMetadata, DependencyFingerprint, KBC_MAGIC, KbcArtifact,
    },
    instruction::{
        BinaryOp, BytecodeInstruction, CallTarget, FieldRef, JumpTarget, PathId, RuntimeHelper,
        Slot, StructId, UnaryOp,
    },
    module::{BytecodeFunction, BytecodeModule, FunctionMetadata, SafeDebugPointKind},
    verifier::{BytecodeVerificationError, verify_module},
};
use kagari_common::identity::{DefinitionKind, ModuleIdentity, PackageId, map::DefinitionContext};
use kagari_contract::{ids::FunctionRef, types::PublicItem};
use kagari_types::declaration::TypeDefKind;

mod artifacts;
mod byte_literals;
mod host_contracts;
mod identities;
mod interfaces;
mod lowering;
mod path_fixtures;
mod validation;
