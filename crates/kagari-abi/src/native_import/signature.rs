//! Guards for the storage shapes consumed by direct Rust implementations. These
//! compare a carried signature; they do not supply source declarations.
use crate::{
    callable::EngineNativeBinding,
    contracts::verify_intrinsic,
    native_import::{NativeSignature, keys},
    numeric::{self, method::IntegerMethodContract},
    scalar::BuiltinType,
    standard::{StandardIntrinsic, surface::StandardEnum},
    types::AbiType,
};

mod storage;

pub(super) fn validate(
    binding: EngineNativeBinding,
    signature: &NativeSignature,
) -> Option<StandardIntrinsic> {
    let operation = match binding {
        EngineNativeBinding::Intrinsic(operation) => operation,
        EngineNativeBinding::Integer(method) => {
            let [AbiType::Builtin(receiver), AbiType::Builtin(rhs)] = signature.params.as_slice()
            else {
                return None;
            };
            let contract = IntegerMethodContract::new(method, *receiver)?;
            if contract.rhs() != *rhs || contract.result() != signature.result {
                return None;
            }
            StandardIntrinsic::Integer(method, *receiver)
        }
        EngineNativeBinding::ParseRadix => {
            let AbiType::StandardEnum {
                kind: StandardEnum::Result,
                args,
            } = &signature.result
            else {
                return None;
            };
            let [AbiType::Builtin(scalar), error] = args.as_slice() else {
                return None;
            };
            scalar.integer_layout()?;
            if !enumeration(error, StandardEnum::ParseError, &[]) {
                return None;
            }
            StandardIntrinsic::ParseRadix(*scalar)
        }
        // Defaults and protocol methods use the continuation registry rather than
        // this direct storage-operation entrypoint.
        EngineNativeBinding::TraitDefault(_) | EngineNativeBinding::Protocol(_) => return None,
    };
    if keys::selected(binding) {
        return storage::valid(operation, &signature.params, &signature.result)
            .then_some(operation);
    }
    let representations: Vec<_> = signature
        .params
        .iter()
        .map(AbiType::representation)
        .collect();
    verify_intrinsic(
        Some(signature.result.representation()),
        operation,
        &representations,
    )
    .ok()?;
    let params = signature.params.as_slice();
    let result = &signature.result;
    let valid = match operation {
        StandardIntrinsic::Integer(method, scalar) => IntegerMethodContract::new(method, scalar)
            .is_some_and(|contract| {
                params == contract.parameters().map(AbiType::Builtin)
                    && result == &contract.result()
            }),
        StandardIntrinsic::ParseNumber(scalar) => {
            params == [AbiType::Builtin(BuiltinType::String)]
                && numeric::parsing_error(scalar)
                    .is_some_and(|error| parsing_result(result, scalar, error))
        }
        StandardIntrinsic::ParseRadix(scalar) => {
            params
                == [
                    AbiType::Builtin(BuiltinType::String),
                    AbiType::Builtin(BuiltinType::U32),
                ]
                && parsing_result(result, scalar, StandardEnum::ParseError)
        }
        StandardIntrinsic::MathMin | StandardIntrinsic::MathMax => matches!(params, [left, right]
            if left == right && left == result && number(left, false)),
        StandardIntrinsic::MathClamp => matches!(params, [value, min, max]
            if value == min && value == max && value == result && number(value, false)),
        StandardIntrinsic::MathAbs => {
            matches!(params, [value] if value == result && number(value, true))
        }
        StandardIntrinsic::MathFloor
        | StandardIntrinsic::MathCeil
        | StandardIntrinsic::MathRound
        | StandardIntrinsic::MathSqrt
        | StandardIntrinsic::MathSin
        | StandardIntrinsic::MathCos
        | StandardIntrinsic::MathTan => {
            params == [AbiType::Builtin(BuiltinType::F64)]
                && *result == AbiType::Builtin(BuiltinType::F64)
        }
        StandardIntrinsic::DebugPrint => {
            params == [AbiType::Builtin(BuiltinType::String)]
                && *result == AbiType::Builtin(BuiltinType::Unit)
        }
        StandardIntrinsic::DebugPanic => {
            params == [AbiType::Builtin(BuiltinType::String)]
                && *result == AbiType::Builtin(BuiltinType::Never)
        }
        StandardIntrinsic::DebugAssert => {
            params
                == [
                    AbiType::Builtin(BuiltinType::Bool),
                    AbiType::Builtin(BuiltinType::String),
                ]
                && *result == AbiType::Builtin(BuiltinType::Unit)
        }
        _ => storage::valid(operation, params, result),
    };
    valid.then_some(operation)
}

fn number(ty: &AbiType, signed: bool) -> bool {
    matches!(ty, AbiType::Builtin(scalar) if scalar.number_type().is_some()
        && (!signed || matches!(scalar, BuiltinType::F32 | BuiltinType::F64)
            || scalar.integer_layout().is_some_and(|(_, signed)| signed)))
}

pub(super) fn enumeration(ty: &AbiType, expected: StandardEnum, members: &[AbiType]) -> bool {
    matches!(ty, AbiType::StandardEnum { kind, args } if *kind == expected && args == members)
}

fn parsing_result(ty: &AbiType, scalar: BuiltinType, error: StandardEnum) -> bool {
    enumeration(
        ty,
        StandardEnum::Result,
        &[
            AbiType::Builtin(scalar),
            AbiType::StandardEnum {
                kind: error,
                args: vec![],
            },
        ],
    )
}
