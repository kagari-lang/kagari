//! Executable field access links portable identities to the reserved root's proof.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::roots::RootSet,
    host::{HostBorrowTable, scoped::ScopedHostView},
    module::{ExecutableModule, StructLayoutRef},
    native::{
        binding::NativeResult, conversion::context::ConversionContext, projection::GeneratedField,
    },
    value::{EphemeralValue, Value},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{reference::ReferenceAccess, ty::Ty};

pub(super) struct ScopedField {
    pub(super) parent: TypeArgument,
    pub(super) owner: ExecutableModule,
    pub(super) declaration: DefinitionId,
    pub(super) projector: GeneratedField,
    pub(super) target: TypeArgument,
}

impl HostBorrowTable {
    fn field(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        layout: &StructLayoutRef,
        slot: usize,
        access: ReferenceAccess,
    ) -> NativeResult<ScopedField> {
        if !view.access.permits(access) {
            return Err(RuntimeError::host_borrow_conflict(
                "shared field receiver cannot grant mutable access",
            ));
        }
        self.with_root(view, |root| {
            let parent = &root.places[view.place];
            if !layout.matches_view(parent.target.view(&root.owner)) {
                return Err(super::invalid());
            }
            let field = layout
                .layout()
                .fields
                .get(slot)
                .ok_or_else(super::invalid)?;
            let projector = root
                .catalog
                .rust_types
                .get(&parent.rust_type)
                .and_then(|mapping| mapping.fields.as_ref())
                .and_then(|fields| fields.iter().find(|proof| proof.name == field.name))
                .cloned()
                .ok_or_else(|| {
                    RuntimeError::module_validation("missing generated inline field capability")
                })?;
            let ty = (projector.target_type)(&root.catalog)?;
            let target = runtime.native_type_argument(&root.owner, &ty)?;
            let (expected, environment) = layout.field_type(slot).ok_or_else(super::invalid)?;
            if projector.root != parent.rust_type
                || !target.view(&root.owner).compatible(TypeView::new(
                    expected,
                    layout.module(),
                    environment,
                ))
            {
                return Err(super::invalid());
            }
            Ok(ScopedField {
                parent: parent.target.clone(),
                owner: root.owner.clone(),
                declaration: field.declaration,
                projector,
                target,
            })
        })
    }

    pub(crate) fn read_field(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        layout: &StructLayoutRef,
        slot: usize,
        access: ReferenceAccess,
    ) -> NativeResult<RootSet> {
        let field = self.field(runtime, view, layout, slot, access)?;
        let mut child = self.project_field(runtime, view, &field)?;
        child.access = access.field_access(layout.layout().fields[slot].mutable);
        if matches!(field.target.ty(), Ty::Struct(_)) {
            let value = runtime
                .gc()
                .alloc_ephemeral(EphemeralValue::HostView(child))?;
            return runtime
                .gc()
                .root_execution_values(vec![value])
                .ok_or_else(super::invalid);
        }
        if !matches!(field.target.ty(), Ty::Builtin(_) | Ty::NativeObject(_)) {
            return Err(RuntimeError::module_validation(
                "inline field read requires a struct view or owned scalar/handle",
            ));
        }
        let mut cx = ConversionContext::new(runtime, &field.owner)?;
        self.with_scoped_loans(
            runtime,
            [(child, field.projector.target, ReferenceAccess::Shared)],
            |[pointer]| {
                // SAFETY: The entire path's exact Rust identity and shared loan precede
                // this generated reader. Its output owns no borrowed Rust reference.
                let retained =
                    unsafe { (field.projector.snapshot)(pointer, &mut cx, &field.target) }?;
                let value = retained.call_argument(runtime, &field.target, &field.owner)?;
                cx.check_value(&field.target, &value)?;
                // Keep the snapshot alive until the independent execution pack is published.
                runtime
                    .gc()
                    .root_execution_values(vec![value])
                    .ok_or_else(super::invalid)
            },
        )
    }

    pub(crate) fn write_field(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        layout: &StructLayoutRef,
        slot: usize,
        value: Value,
    ) -> NativeResult<()> {
        let field = self.field(runtime, view, layout, slot, ReferenceAccess::Mutable)?;
        if !layout.layout().fields[slot].mutable {
            return Err(RuntimeError::module_validation(
                "write to read-only inline field",
            ));
        }
        let child = self.project_field(runtime, view, &field)?;
        let mut cx = ConversionContext::new(runtime, &field.owner)?;
        // Conversion can allocate/reenter. It finishes before acquiring any
        // reference; the inert path resolves against the current root afterward.
        let decoded = (field.projector.decode)(&mut cx, &field.target, &value)?;
        let displaced = self.with_scoped_loans(
            runtime,
            [(child, field.projector.target, ReferenceAccess::Mutable)],
            |[pointer]| {
                // SAFETY: The decoder and generated writer share exact F identity;
                // an exclusive path loan protects replacement of initialized F.
                Ok(unsafe { (field.projector.replace)(pointer, decoded) })
            },
        )?;
        drop(displaced);
        Ok(())
    }
}
