//! Select native concrete-result interface construction while source proofs exist.
use crate::source::{
    lower::{MirLoweringError, instances::InstancePlanner},
    types::{raise_nominal_type, raise_type},
};
use kagari_common::span::Span;
use kagari_contract::{
    native_import::{NativeImport, result::NativeResultAdapter},
    types::ConcreteFunctionIdentity,
};
use kagari_hir::{
    typeck::{GenericBounds, table::ConstraintTarget},
    types::semantic::lower_type,
};
use kagari_types::ty::{Constraint, Ty, substitution::TypeSubstitution};

impl InstancePlanner<'_> {
    pub(crate) fn native_result_adapter(
        &mut self,
        import: &NativeImport,
        span: Span,
    ) -> Result<Option<NativeResultAdapter>, MirLoweringError> {
        let Some(declaration) = self.registered_native_declaration(&import.instance.declaration)
        else {
            return Ok(None);
        };
        let Some(receiver) = &declaration.concrete_result else {
            return Ok(None);
        };
        let invalid = || MirLoweringError::MissingBinding("native result interface application");
        let Ty::Trait(interface) = &import.signature.result else {
            return Err(invalid());
        };
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in declaration
            .function
            .generic_params
            .iter()
            .zip(&import.instance.arguments)
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let receiver = self.catalog.normalize_type(&raise_type(
            &substitution
                .apply(receiver, &self.options.cancel)
                .map_err(|_| invalid())?,
        ));
        let assumptions: GenericBounds = import
            .requirements
            .iter()
            .map(|bound| {
                (
                    raise_type(&bound.ty),
                    bound
                        .constraints
                        .iter()
                        .map(|constraint| match constraint {
                            Constraint::Standard(kind) => ConstraintTarget::Standard(*kind),
                            Constraint::Trait(interface) => {
                                ConstraintTarget::Trait(raise_nominal_type(interface))
                            }
                        })
                        .collect(),
                )
            })
            .collect();
        let (implementation, arguments) = self
            .catalog
            .concrete_interface_implementation(
                &raise_nominal_type(interface),
                &receiver,
                &assumptions,
                100_000,
                64,
                &self.options.cancel,
            )
            .map_err(|_| invalid())?
            .ok_or_else(invalid)?;
        self.record_interface(&implementation, &arguments, span)?;
        self.require_parent_interfaces(&receiver, &raise_nominal_type(interface), span)?;
        self.record_layout_root(&receiver, &Default::default(), span)?;
        Ok(Some(NativeResultAdapter {
            receiver: lower_type(&receiver),
            implementation: ConcreteFunctionIdentity {
                declaration: implementation,
                arguments: arguments.iter().map(lower_type).collect(),
            },
        }))
    }
}
