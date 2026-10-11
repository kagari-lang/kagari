//! Temporary authoring boundary until Native builders retain their construction scope.
use crate::declaration::module::{DeclarationError, ModuleDecl};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        map::DefinitionContext,
        mapping::{DefinitionMapper, DefinitionRecord},
        metadata::DefinitionMetadata,
        table::DefinitionId,
    },
};

impl ModuleDecl {
    pub(super) fn adopt_declarations(
        &self,
    ) -> Result<DefinitionMetadata<ModuleDecl<DefinitionId>>, DeclarationError> {
        let invalid = |error| DeclarationError(format!("invalid native identities: {error}"));
        let context = DefinitionContext::new().map_err(invalid)?;
        let root = context.intern_root(&self.identity).map_err(invalid)?;
        for ty in &self.types {
            context
                .intern_child(root, ty.kind.definition_kind(), &ty.name, 0)
                .map_err(invalid)?;
        }
        for contract in &self.traits {
            let owner = context
                .intern_child(root, DefinitionKind::Trait, &contract.name, 0)
                .map_err(invalid)?;
            for method in &contract.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(invalid)?;
            }
        }
        for function in &self.functions {
            context
                .intern_child(root, DefinitionKind::Function, &function.name, 0)
                .map_err(invalid)?;
        }
        for (index, implementation) in self.implementations.iter().enumerate() {
            let occurrence = u32::try_from(index)
                .map_err(|_| DeclarationError("too many native implementations".into()))?;
            let owner = context
                .intern_child(root, DefinitionKind::Impl, "", occurrence)
                .map_err(invalid)?;
            for method in &implementation.methods {
                context
                    .intern_child(owner, DefinitionKind::Method, &method.name, 0)
                    .map_err(invalid)?;
            }
        }
        let cancel = CancellationToken::default();
        let records = self
            .map_identities(&mut DefinitionMapper::new(
                &mut |path| Ok(context.intern(path)?),
                &cancel,
            ))
            .map_err(|error| DeclarationError(format!("invalid native identities: {error}")))?;
        DefinitionMetadata::checked(context.snapshot(), records, &cancel)
            .map_err(|error| DeclarationError(format!("invalid native identities: {error}")))
    }
}
