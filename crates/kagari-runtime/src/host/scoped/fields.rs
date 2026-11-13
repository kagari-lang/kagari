//! Executable field access links portable identities to the reserved root's proof.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    host::{HostBorrowTable, scoped::ScopedHostView},
    module::{ExecutableModule, StructLayoutRef},
    native::{
        binding::NativeResult, conversion::context::ConversionContext, projection::GeneratedField,
    },
    value::{
        EphemeralValue,
        place::{PublishedResult, ResultPlace},
        view::ValueView,
    },
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
}

impl Runtime {
    pub(crate) fn read_scoped_field<'result>(
        &mut self,
        view: ScopedHostView,
        layout: &StructLayoutRef,
        slot: usize,
        access: ReferenceAccess,
        destination: ResultPlace<'result>,
    ) -> NativeResult<PublishedResult<'result>> {
        let field = self.host_borrows.field(self, view, layout, slot, access)?;
        let mut child = self.host_borrows.project_field(self, view, &field)?;
        child.access = access.field_access(layout.layout().fields[slot].mutable);
        if matches!(field.target.ty(), Ty::Struct(_)) {
            let mut cx = ConversionContext::new(self, &field.owner)?;
            let value = cx.allocate_ephemeral(EphemeralValue::HostView(child))?;
            return destination.publish(cx.runtime_mut(), &value);
        }
        if !matches!(field.target.ty(), Ty::Builtin(_) | Ty::NativeObject(_)) {
            return Err(RuntimeError::module_validation(
                "inline field read requires a struct view or owned scalar/handle",
            ));
        }
        self.with_host_loans(
            [(child, field.projector.target, ReferenceAccess::Shared)],
            |runtime, [pointer]| {
                let mut cx = ConversionContext::new(runtime, &field.owner)?;
                // SAFETY: The entire path's exact Rust identity and shared loan precede
                // this generated reader. Its output owns no borrowed Rust reference.
                let value = unsafe { (field.projector.snapshot)(pointer, &mut cx, &field.target) }?;
                cx.check_value(&field.target, &value)?;
                destination.publish(cx.runtime_mut(), &value)
            },
        )
    }

    pub(crate) fn write_scoped_field(
        &mut self,
        view: ScopedHostView,
        layout: &StructLayoutRef,
        slot: usize,
        value: &ValueView<'_>,
    ) -> NativeResult<()> {
        let field = self
            .host_borrows
            .field(self, view, layout, slot, ReferenceAccess::Mutable)?;
        if !layout.layout().fields[slot].mutable {
            return Err(RuntimeError::module_validation(
                "write to read-only inline field",
            ));
        }
        let child = self.host_borrows.project_field(self, view, &field)?;
        // Conversion can allocate/reenter. It finishes before acquiring any
        // reference; the inert path resolves against the current root afterward.
        let decoded = {
            let mut cx = ConversionContext::new(self, &field.owner)?;
            (field.projector.decode)(&mut cx, &field.target, value)?
        };
        let displaced = self.with_host_loans(
            [(child, field.projector.target, ReferenceAccess::Mutable)],
            |_, [pointer]| {
                // SAFETY: The decoder and generated writer share exact F identity;
                // an exclusive path loan protects replacement of initialized F.
                Ok(unsafe { (field.projector.replace)(pointer, decoded) })
            },
        )?;
        drop(displaced);
        Ok(())
    }
}
