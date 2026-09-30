//! Validate installed bindings after ordinary source types and bounds resolve.
use crate::{
    DiagnosticBuffer,
    aggregates::AggregateCatalog,
    lower::LoweredModule,
    native::NativeBinding,
    typeck::{ConstraintTarget, FunctionImplementation, ModuleSignatures, TypedFunction},
    types::abi::{lower_nominal_type, lower_type},
};
use kagari_abi::{
    native_import::{NativeSignature, contract::binding_signature_valid},
    types::{ConstraintAbi, GenericBoundAbi},
};
use kagari_common::{Diagnostic, DiagnosticKind, cancellation::CancellationToken};

pub(crate) fn validate(
    lowered: &LoweredModule,
    signatures: &ModuleSignatures,
    aggregates: &AggregateCatalog,
    diagnostics: &mut DiagnosticBuffer,
    cancel: &CancellationToken,
) {
    for function in signatures.functions() {
        if cancel.check().is_err() {
            break;
        }
        if let FunctionImplementation::Native(NativeBinding::Engine(binding)) =
            function.implementation
            && !valid(function, aggregates)
        {
            diagnostics.push(
                Diagnostic::error(DiagnosticKind::InvalidNativeSignature {
                    function: function.name.clone(),
                    binding: format!("{binding:?}"),
                })
                .with_span(lowered.source_map.function_span(function.id)),
            );
        }
    }
}

fn valid(function: &TypedFunction, aggregates: &AggregateCatalog) -> bool {
    let FunctionImplementation::Native(NativeBinding::Engine(binding)) = function.implementation
    else {
        return true;
    };
    if function.params.iter().any(|p| p.ty.is_unresolved()) || function.return_type.is_unresolved()
    {
        // Ordinary type diagnostics already reject this declaration.
        return true;
    }
    let signature = NativeSignature {
        params: function
            .params
            .iter()
            .map(|p| lower_type(&aggregates.normalize_type(&p.ty)))
            .collect(),
        result: lower_type(&aggregates.normalize_type(&function.return_type)),
    };
    let bounds: Vec<_> = function
        .bounds
        .iter()
        .map(|(ty, constraints)| GenericBoundAbi {
            ty: lower_type(&aggregates.normalize_type(ty)),
            constraints: constraints
                .iter()
                .map(|constraint| match constraint {
                    ConstraintTarget::Standard(kind) => ConstraintAbi::Standard(*kind),
                    ConstraintTarget::Trait(interface) => {
                        ConstraintAbi::Trait(lower_nominal_type(interface))
                    }
                })
                .collect(),
        })
        .collect();
    binding_signature_valid(binding, &signature, &bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::AnalysisDatabase;
    use kagari_abi::scalar::BuiltinType;
    use kagari_common::source_database::SourceDatabase;

    #[test]
    fn installed_signatures_check_arguments_and_results_after_resolution() {
        let snapshot = AnalysisDatabase::default()
            .snapshot(
                SourceDatabase::default().snapshot(),
                Default::default(),
                &Default::default(),
            )
            .unwrap();
        let file = snapshot
            .declaration_snapshot()
            .files()
            .find(|file| file.source().name() == "kagari://std/debug.kgr")
            .unwrap();
        let analysis = snapshot.file(file.source().id()).unwrap();
        let facts = analysis.result().facts();
        let mut function = facts
            .typed
            .functions
            .iter()
            .find(|function| function.name == "print")
            .unwrap()
            .clone();
        assert!(valid(&function, &facts.aggregates));
        function.params.clear();
        assert!(!valid(&function, &facts.aggregates));
        function = facts
            .typed
            .functions
            .iter()
            .find(|function| function.name == "print")
            .unwrap()
            .clone();
        function.return_type = crate::types::TypeId::Builtin(BuiltinType::Bool);
        assert!(!valid(&function, &facts.aggregates));
    }
}
