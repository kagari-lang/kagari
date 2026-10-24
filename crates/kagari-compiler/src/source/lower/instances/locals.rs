//! Named local placement keeps authored identity separate from receiving code.

use crate::source::lower::{
    MirLoweringError,
    instances::{Instance, InstancePlanner},
};
use kagari_common::{
    identity::{DefinitionKind, mapping::DefinitionMappingError},
    span::Span,
};
use kagari_contract::types::named::FunctionOrigin;
use kagari_hir::{
    AnalyzedModule, declarations::DeclarationId, hir::ids::FunctionId,
    resolver::resolved::ResolvedName, types::TypeId,
};
use kagari_mir::ids::InstanceId;

impl<'a> InstancePlanner<'a> {
    pub(in crate::source::lower) fn function_origins(&self) -> Vec<FunctionOrigin> {
        let mut origins = self
            .local_function_placements
            .iter()
            .map(|(authored, placement)| FunctionOrigin {
                placement: *placement,
                authored: *authored,
            })
            .collect::<Vec<_>>();
        let definitions = self.definitions();
        origins.sort_by(|left, right| {
            definitions
                .compare(left.placement, right.placement)
                .expect("checked local placement identities")
        });
        origins
    }

    /// Place a callable beside its receiving instance while retaining its authored owner.
    pub(in crate::source::lower) fn enqueue_local_function_value(
        &mut self,
        parent: &Instance,
        source: &'a AnalyzedModule,
        function: FunctionId,
        arguments: Vec<TypeId>,
        span: Span,
    ) -> Result<InstanceId, MirLoweringError> {
        let target = source
            .declarations
            .target(ResolvedName::Function(function))
            .ok_or(MirLoweringError::MissingBinding(
                "local function definition",
            ))?;
        let DeclarationId::Definition(declaration) = &target.id else {
            return Err(MirLoweringError::MissingBinding("local callable identity"));
        };
        let placement = if self.module_handle(*declaration) == self.owner_handle() {
            None
        } else if let Some(placement) = self.local_function_placements.get(declaration) {
            Some(*placement)
        } else {
            // Imported default bodies execute under the implementation identity.
            // The explicit origin record prevents this receiving slot from being
            // mistaken for a new authored named declaration.
            let placement = self
                .context
                .intern_child(
                    parent.key.declaration,
                    DefinitionKind::Function,
                    &format!("$local_{}", function.index()),
                    0,
                )
                .map_err(DefinitionMappingError::from)?;
            // Recursive references reuse placement. Locals inherit no caller
            // substitution, including when their default body was imported.
            self.local_function_placements
                .insert(*declaration, placement);
            Some(placement)
        };
        self.enqueue_script(source, function, arguments, placement, span)
    }
}
