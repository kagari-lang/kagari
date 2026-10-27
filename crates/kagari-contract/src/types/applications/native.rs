//! Bounded native nominal admission from carrier-scoped executable contracts.
use crate::{
    callable::{generic::GenericBody, shared::SharedCall},
    native_import::NativeImport,
    types::{applications::dynamic, proofs::ProofCatalog},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};
use kagari_types::{
    declaration::{TypeDef, TypeDefKind, native::NativeStorageLayout, verify::types_in_scope},
    ty::{
        GenericBound, GenericParam, NominalTy, Ty,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};

/// Checks native occurrences using declarations and proofs from the same dependency closure.
/// Returned nominals are not certificates: the executable owner issues evidence only
/// after declaration, call-target and complete graph validation have also succeeded.
pub struct NativeApplications<'a, 'declaration, F> {
    catalog: &'a ProofCatalog<'declaration>,
    lookup: F,
    cancel: &'a CancellationToken,
}

impl<'a, 'declaration, F> NativeApplications<'a, 'declaration, F>
where
    F: Fn(&DefinitionId) -> Option<&'declaration TypeDef>,
{
    pub fn new(
        catalog: &'a ProofCatalog<'declaration>,
        lookup: F,
        cancel: &'a CancellationToken,
    ) -> Self {
        Self {
            catalog,
            lookup,
            cancel,
        }
    }

    /// Check a declaration-matched native import under its authored body bounds.
    /// Closed imports never assume their requirements; those are obligations.
    pub fn import(&self, import: &NativeImport) -> Result<Vec<NominalTy>, TypeTransformError> {
        self.types(
            import
                .instance
                .arguments
                .iter()
                .chain(&import.signature.params)
                .chain([&import.signature.result])
                .chain(
                    import
                        .result_adapter
                        .iter()
                        .map(|adapter| &adapter.receiver),
                ),
            import.generic.as_ref(),
        )
    }

    /// Check the actual native signature in the caller's binder, not the template's.
    /// The caller must first validate the SharedCall against this exact import.
    pub fn shared(
        &self,
        import: &NativeImport,
        call: &SharedCall,
        caller: Option<&GenericBody>,
    ) -> Result<Vec<NominalTy>, TypeTransformError> {
        let body = import
            .generic
            .as_ref()
            .ok_or(TypeTransformError::InvalidContract)?;
        if body.parameters.len() != call.arguments.len() {
            return Err(TypeTransformError::InvalidContract);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in body.parameters.iter().zip(&call.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        let actual = import
            .signature
            .params
            .iter()
            .chain([&import.signature.result])
            .chain(
                import
                    .result_adapter
                    .iter()
                    .map(|adapter| &adapter.receiver),
            )
            .map(|ty| {
                self.catalog
                    .normalize(&substitution.apply(ty, self.cancel)?, self.cancel)
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.types(
            actual
                .iter()
                .chain(&call.arguments)
                .chain(&call.signature.params)
                .chain([&call.signature.result]),
            caller,
        )
    }

    /// Validate nested applications without treating their own bounds as assumptions.
    /// Only opaque applications are returned for payload admission. Existing unbounded
    /// nonopaque native layouts remain valid but cannot grant an opaque payload witness.
    pub fn types<'ty>(
        &self,
        types: impl IntoIterator<Item = &'ty Ty>,
        body: Option<&GenericBody>,
    ) -> Result<Vec<NominalTy>, TypeTransformError> {
        let parameters = body.map_or(&[][..], |body| body.parameters.as_slice());
        let assumptions = body.map_or(&[][..], |body| body.bounds.as_slice());
        let mut admitted = Vec::new();
        let mut remaining = MAX_TYPE_NODES;
        for root in types {
            self.cancel
                .check()
                .map_err(|_| TypeTransformError::Cancelled)?;
            if !types_in_scope([root], parameters, self.cancel, self.catalog.definitions())
                || !root.reference_position_valid()
            {
                self.cancel
                    .check()
                    .map_err(|_| TypeTransformError::Cancelled)?;
                return Err(TypeTransformError::InvalidContract);
            }
            dynamic::validate(
                [root],
                &|id| self.catalog.trait_contract(id),
                self.catalog.definitions(),
                self.cancel,
            )?;
            let mut pending = vec![root];
            while let Some(ty) = pending.pop() {
                self.cancel
                    .check()
                    .map_err(|_| TypeTransformError::Cancelled)?;
                remaining = remaining
                    .checked_sub(1)
                    .ok_or(TypeTransformError::LimitExceeded)?;
                match ty {
                    Ty::NativeObject(nominal) => {
                        let declaration = (self.lookup)(&nominal.declaration)
                            .ok_or(TypeTransformError::InvalidContract)?;
                        self.application(declaration, nominal, parameters, assumptions)?;
                        if declaration.kind
                            == TypeDefKind::NativeStorage(NativeStorageLayout::Opaque)
                            && !admitted.contains(nominal)
                        {
                            admitted.push(nominal.clone());
                        }
                        pending.extend(&nominal.arguments);
                    }
                    Ty::Struct(nominal) | Ty::Enum(nominal) | Ty::Trait(nominal) => {
                        pending.extend(&nominal.arguments);
                        pending.extend(nominal.associated_types.values());
                    }
                    Ty::Projection {
                        receiver,
                        interface,
                        arguments,
                        ..
                    } => {
                        pending.push(receiver);
                        pending.extend(&interface.arguments);
                        pending.extend(interface.associated_types.values());
                        pending.extend(arguments);
                    }
                    Ty::Reference { target, .. }
                    | Ty::Array(target)
                    | Ty::Iter(target)
                    | Ty::Range(target, _) => pending.push(target),
                    Ty::Tuple(items) => pending.extend(items),
                    Ty::Function { params, result } => {
                        pending.extend(params);
                        pending.push(result);
                    }

                    Ty::Builtin(_) | Ty::Host(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
                }
                if pending.len() > remaining {
                    return Err(TypeTransformError::LimitExceeded);
                }
            }
        }
        Ok(admitted)
    }

    fn application(
        &self,
        declaration: &TypeDef,
        nominal: &NominalTy,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
    ) -> Result<(), TypeTransformError> {
        let TypeDefKind::NativeStorage(layout) = declaration.kind else {
            return Err(TypeTransformError::InvalidContract);
        };
        if (!declaration.bounds.is_empty() && layout != NativeStorageLayout::Opaque)
            || declaration.generic_params.len() != nominal.arguments.len()
            || !layout.valid_parameters(nominal.arguments.len())
            || !nominal.associated_types.is_empty()
            || !types_in_scope(
                &nominal.arguments,
                parameters,
                self.cancel,
                self.catalog.definitions(),
            )
            || nominal
                .arguments
                .iter()
                .any(|ty| ty.contains_stored_reference() || !ty.reference_position_valid())
        {
            return Err(TypeTransformError::InvalidContract);
        }
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in declaration.generic_params.iter().zip(&nominal.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        for bound in substitution.apply_bounds(
            &declaration.bounds,
            self.catalog.definitions(),
            self.cancel,
        )? {
            if !self.catalog.constraints_hold(
                &bound.ty,
                &bound.constraints,
                assumptions,
                self.cancel,
            )? {
                return Err(TypeTransformError::InvalidContract);
            }
        }
        Ok(())
    }
}
