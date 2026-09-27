use crate::{
    bytecode::{
        ArtifactBuildOptions, ArtifactCompatibility, ArtifactFingerprint, ArtifactSectionId,
        ArtifactValidationError, BinaryOp, BytecodeFunction, BytecodeInstruction, BytecodeModule,
        BytecodeVerificationError, CallTarget, DebugMetadata, DependencyFingerprint, FieldRef,
        FunctionMetadata, FunctionRef, JumpTarget, KBC_MAGIC, KbcArtifact, LocalSlot, PathId,
        PathRecord, Register, RuntimeHelper, SafeDebugPointKind, StandardIntrinsic, StructId,
        UnaryOp, verify_module,
    },
    module::{PublicAbiItem, TypeAbiKind, ValueType},
    tests::common,
};
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::{ModuleIdentity, PackageId};

fn host_trait_test_module(source: &str) -> BytecodeModule {
    let mut module = common::bytecode_ok(source);
    add_readable_host(&mut module);
    module
}

fn add_readable_host(module: &mut BytecodeModule) {
    use kagari_common::{
        host_interface::{
            HostMethodDeclaration, HostTraitImplementationDeclaration, HostTraitMethodBinding,
            HostTypeDeclaration, HostValueType,
        },
        identity::{DefinitionId, DefinitionKind, DefinitionPathSegment},
    };

    let trait_id = DefinitionId {
        module: module.identity.clone(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: "Readable".into(),
            occurrence: 0,
        }],
    };
    let mut trait_method = trait_id.clone();
    trait_method.path.push(DefinitionPathSegment {
        kind: DefinitionKind::Method,
        name: "get".into(),
        occurrence: 0,
    });
    let mut host = HostTypeDeclaration::new("demo.Counter");
    let method = HostMethodDeclaration::new(&host.id, "read", vec![], HostValueType::I32);
    host.methods.push(method.clone());
    host.trait_implementations
        .push(HostTraitImplementationDeclaration::new(
            trait_id,
            vec![HostValueType::I32],
            vec![HostTraitMethodBinding {
                trait_method,
                host_method: method.id.clone(),
            }],
        ));
    module
        .host_interface
        .functions
        .push(host.method_contract(&method.id).unwrap());
    module.host_interface.types.push(host);
}

mod artifacts;
mod host_contracts;
mod identities;
mod interfaces;
mod lowering;
mod validation;
