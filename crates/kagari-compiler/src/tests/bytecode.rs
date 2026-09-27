use crate::tests::common;
use kagari_abi::ids::FunctionRef;
use kagari_abi::representation::ValueType;
use kagari_abi::standard::StandardIntrinsic;
use kagari_abi::types::PublicAbiItem;
use kagari_abi::types::TypeAbiKind;
use kagari_bytecode::ArtifactBuildOptions;
use kagari_bytecode::ArtifactCompatibility;
use kagari_bytecode::ArtifactFingerprint;
use kagari_bytecode::ArtifactSectionId;
use kagari_bytecode::ArtifactValidationError;
use kagari_bytecode::BinaryOp;
use kagari_bytecode::BytecodeFunction;
use kagari_bytecode::BytecodeInstruction;
use kagari_bytecode::BytecodeModule;
use kagari_bytecode::BytecodeVerificationError;
use kagari_bytecode::CallTarget;
use kagari_bytecode::DebugMetadata;
use kagari_bytecode::DependencyFingerprint;
use kagari_bytecode::FieldRef;
use kagari_bytecode::FunctionMetadata;
use kagari_bytecode::JumpTarget;
use kagari_bytecode::KBC_MAGIC;
use kagari_bytecode::KbcArtifact;
use kagari_bytecode::LocalSlot;
use kagari_bytecode::PathId;
use kagari_bytecode::PathRecord;
use kagari_bytecode::Register;
use kagari_bytecode::RuntimeHelper;
use kagari_bytecode::SafeDebugPointKind;
use kagari_bytecode::StructId;
use kagari_bytecode::UnaryOp;
use kagari_bytecode::verify_module;
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
