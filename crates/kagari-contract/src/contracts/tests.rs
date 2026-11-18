use super::{ContractError, verify_native_call};
use crate::{native_import::NativeImport, types::ConcreteFunctionIdentity};
use kagari_abi::representation::ValueType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};
use kagari_types::{
    callable::Signature,
    reference::ReferenceAccess,
    scalar::BuiltinType,
    ty::{NominalTy, Ty},
};

fn import(context: &DefinitionContext, parameter: Ty) -> NativeImport {
    let root = context
        .intern_root(&ModuleIdentity::single_file("native_call.kgr"))
        .unwrap();
    let declaration = context
        .intern_child(root, DefinitionKind::Function, "call", 0)
        .unwrap();
    NativeImport {
        receiver_access: None,
        result_adapter: None,
        generic: None,
        instance: ConcreteFunctionIdentity {
            declaration,
            arguments: vec![],
        },
        binding: declaration,
        signature: Signature {
            params: vec![parameter],
            result: Ty::Builtin(BuiltinType::Unit),
        },
        requirements: vec![],
        callables: vec![],
    }
}

#[test]
fn native_receiver_loan_requires_an_owned_native_operand() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("native_call.kgr"))
        .unwrap();
    let nominal = |kind| NominalTy {
        declaration: context.intern_child(root, kind, "Payload", 0).unwrap(),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut call = import(
        &context,
        Ty::NativeObject(nominal(DefinitionKind::AssociatedType)),
    );
    call.receiver_access = Some(ReferenceAccess::Mutable);
    let definitions = context.snapshot();
    assert_eq!(
        verify_native_call(None, &call, &[ValueType::HeapObject], &definitions),
        Ok(())
    );
    assert_eq!(
        verify_native_call(None, &call, &[ValueType::HostHandle], &definitions),
        Err(ContractError::TypeMismatch {
            context: "native call argument",
            expected: ValueType::HeapObject,
            found: ValueType::HostHandle,
        }),
    );
    call.signature.params[0] = Ty::Struct(nominal(DefinitionKind::Struct));
    let definitions = context.snapshot();
    assert!(call.structurally_valid(&definitions));
    assert_eq!(
        verify_native_call(None, &call, &[ValueType::HeapObject], &definitions),
        Err(ContractError::InvalidOperation {
            reason: "native receiver loan requires runtime-owned native storage",
        }),
    );
}

#[test]
fn native_manual_host_argument_uses_its_declared_representation() {
    let context = DefinitionContext::new().unwrap();
    let root = context
        .intern_root(&ModuleIdentity::single_file("manual_host.kgr"))
        .unwrap();
    let host = context
        .intern_child(root, DefinitionKind::Struct, "Host", 0)
        .unwrap();
    let call = import(&context, Ty::Host(host));
    let definitions = context.snapshot();
    assert_eq!(
        verify_native_call(None, &call, &[ValueType::HostHandle], &definitions),
        Ok(())
    );
    assert_eq!(
        verify_native_call(None, &call, &[ValueType::HeapObject], &definitions),
        Err(ContractError::TypeMismatch {
            context: "native call argument",
            expected: ValueType::HostHandle,
            found: ValueType::HeapObject,
        }),
    );
}
