//! Owner-scoped inherent groups share lowering with ordinary native builders.
use crate::native::{
    assembly::{
        conflict,
        draft::{MethodDraft, ModuleDraft},
        references::{DraftReference, ReferenceKind},
    },
    binding::NativeResult,
    builder::inherent::InherentMethodsBuilder,
    catalog::DeclarationCatalog,
    types::Type,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::{GenericBound, GenericParam, Ty};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

impl ModuleDraft<'_> {
    /// Collect a concrete inherent group, retaining transactional submission.
    pub fn inherent<T>(
        &mut self,
        receiver: Type,
        configure: impl FnOnce(&mut MethodDraft) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let owner = self.next_implementation_id()?;
        self.inherent_group(owner, receiver, &[], configure)
    }

    /// Collect `impl<T, ...> Owner<T, ...>` under its final implementation binder.
    /// Parameters follow the referenced type's declared order; the callback runs
    /// once and submits ordinary fluent MethodDecl definitions. Each invocation
    /// owns distinct parameters, even when it uses the same type reference.
    ///
    /// The reference must belong to this module and assembly scope. Its complete
    /// category/arity contract may be forward-declared and is checked at sealing.
    /// Foreign handles reject before the callback; callback/lowering errors publish
    /// no partial group. Declaration-only sealing never prepares its adapters.
    pub fn inherent_generic<T>(
        &mut self,
        reference: &DraftReference,
        configure: impl FnOnce(&[Type], &mut MethodDraft) -> NativeResult<T>,
    ) -> NativeResult<T> {
        self.check_local(reference.id)?;
        if !matches!(reference.kind, ReferenceKind::Type(_)) {
            return Err(conflict(
                "generic inherent receiver requires a type reference",
            ));
        }
        let owner = self.next_implementation_id()?;
        let parameters = (0..reference.arity)
            .map(|position| GenericParam { owner, position })
            .collect::<Vec<_>>();
        let arguments = parameters
            .iter()
            .map(|parameter| Type::from_semantic(parameter.as_type(), &self.definitions()))
            .collect::<NativeResult<Vec<_>>>()?;
        let receiver = reference.apply(arguments.clone())?;
        let result = self.inherent_group(owner, receiver, &parameters, |methods| {
            configure(&arguments, methods)
        })?;
        self.state.references.push(reference.clone());
        Ok(result)
    }

    fn inherent_group<T>(
        &mut self,
        owner: DefinitionId,
        receiver: Type,
        parameters: &[GenericParam],
        configure: impl FnOnce(&mut MethodDraft) -> NativeResult<T>,
    ) -> NativeResult<T> {
        let receiver = Type::from_semantic(receiver.adopt(self.context)?, &self.definitions())?;
        let mut methods = MethodDraft {
            builder: InherentMethodsBuilder {
                context: self.context.clone(),
                catalog: DeclarationCatalog::in_context(self.context.clone()),
                owner,
                receiver,
                receiver_codec: None,
                methods: BTreeMap::new(),
                documentation: BTreeMap::new(),
                method_parameters: BTreeMap::new(),
                requirements: BTreeMap::new(),
                concrete_results: BTreeMap::new(),
                bindings: BTreeMap::new(),
            },
            bounds: vec![],
            bindings: BTreeMap::new(),
        };
        let result = configure(&mut methods)?;
        let lowered = methods.builder.lower(parameters, &methods.bounds)?;
        self.state
            .declaration
            .implementations
            .push(lowered.declaration);
        self.state
            .declaration
            .documentation
            .extend(lowered.documentation);
        self.state
            .declaration
            .concrete_results
            .extend(lowered.concrete_results);
        self.state
            .declaration
            .callable_requirements
            .extend(lowered.requirements);
        self.state.bindings.extend(methods.bindings);
        Ok(result)
    }
}

impl MethodDraft {
    /// The receiver specialized with this group's own generic parameters.
    pub fn receiver(&self) -> Type {
        self.builder.receiver.clone()
    }

    /// Add an obligation inherited by every method, including its concrete-result
    /// and selected-call proofs. A forward trait application is checked at sealing.
    pub fn bound(&mut self, ty: Type, interface: Type) -> NativeResult<()> {
        let ty = ty.adopt(&self.builder.context)?;
        let Ty::Trait(interface) = interface.adopt(&self.builder.context)? else {
            return Err(conflict("inherent bound requires a trait application"));
        };
        self.bounds.push(GenericBound {
            ty,
            constraints: vec![interface],
        });
        Ok(())
    }
}
