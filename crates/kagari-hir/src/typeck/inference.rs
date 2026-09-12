use crate::types::{GenericParameterType, TypeId, TypeSubstitution};

/// Infer only the callee's parameters. Repeated occurrences are checked against
/// the resulting signature by the caller; no source spelling participates.
pub(super) fn infer(
    expected: &TypeId,
    actual: &TypeId,
    parameters: &[GenericParameterType],
    substitution: &mut TypeSubstitution,
) {
    if actual.is_unresolved() {
        return;
    }
    match (expected, actual) {
        (TypeId::Generic(parameter), _) if parameters.contains(parameter) => {
            substitution
                .entry(parameter.clone())
                .or_insert_with(|| actual.clone());
        }
        (TypeId::Tuple(expected), TypeId::Tuple(actual)) if expected.len() == actual.len() => {
            for (expected, actual) in expected.iter().zip(actual) {
                infer(expected, actual, parameters, substitution);
            }
        }
        (
            TypeId::StandardEnum {
                kind: expected_kind,
                args: expected,
            },
            TypeId::StandardEnum {
                kind: actual_kind,
                args: actual,
            },
        ) if expected_kind == actual_kind && expected.len() == actual.len() => {
            for (expected, actual) in expected.iter().zip(actual) {
                infer(expected, actual, parameters, substitution);
            }
        }
        (TypeId::Array(expected), TypeId::Array(actual))
        | (TypeId::Set(expected), TypeId::Set(actual)) => {
            infer(expected, actual, parameters, substitution)
        }
        (TypeId::Map { key: ek, value: ev }, TypeId::Map { key: ak, value: av }) => {
            infer(ek, ak, parameters, substitution);
            infer(ev, av, parameters, substitution);
        }
        _ => {}
    }
}
