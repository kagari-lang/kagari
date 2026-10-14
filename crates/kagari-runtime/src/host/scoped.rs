//! External root records share the existing frame and whole-object loan owner.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, bindings::TypeBindings, compatibility::TypeView},
    host::{
        BorrowEpoch, FrameBorrowRecord, HostBorrowKind, HostBorrowOwner, HostBorrowTable,
        HostCallGuard, HostFrameId, HostObjectId,
    },
    module::{LoadedModule, ModuleEpochRetention, retention::ProgramLease},
    native::{
        binding::NativeResult,
        projection::GeneratedField,
        rust_type::{RustField, RustType},
    },
    value::{EphemeralValue, Value},
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{reference::ReferenceAccess, ty::Ty};
use std::{
    any::TypeId,
    mem::size_of,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// An inert identity and access capability. It contains no Rust address. The
/// owning frame supplies the checked target and revokes it at scope exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopedHostView {
    owner: HostBorrowOwner,
    frame: HostFrameId,
    epoch: BorrowEpoch,
    object: HostObjectId,
    place: usize,
    access: ReferenceAccess,
}

#[derive(Debug)]
pub(super) struct ScopedRoot {
    object: HostObjectId,
    pointer: *mut (),
    places: Vec<ScopedPlace>,
    owner: LoadedModule,
    access: ReferenceAccess,
    _program: ProgramLease,
}

#[derive(Debug, Clone)]
struct ScopedPlace {
    rust_type: TypeId,
    target: TypeArgument,
    path: Arc<[DefinitionId]>,
    projectors: Arc<[GeneratedField]>,
}

// SAFETY: Only HostScope can publish these records, while holding an exclusive
// Rust borrow of Runtime. HostScope and its branded views are !Send. Before that
// borrow ends, its guard removes the frame and every address. Runtime therefore
// retains its Send contract only when no external address can be transferred.
unsafe impl Send for ScopedRoot {}

impl HostBorrowTable {
    /// The caller keeps T borrowed and Runtime exclusively reserved until frame
    /// removal. Shared pointers may only be dereferenced under a shared loan.
    pub(crate) unsafe fn register_scoped<T: Send + 'static>(
        &self,
        runtime: &Runtime,
        frame: &HostCallGuard<'_>,
        descriptor: &RustType<T>,
        pointer: *mut T,
        access: ReferenceAccess,
    ) -> NativeResult<ScopedHostView> {
        runtime.resources().poll_execution()?;
        runtime.validate_loaded_module(descriptor.owner())?;
        descriptor.type_argument().validate(runtime)?;
        let program = runtime
            .retain_program(descriptor.owner(), ModuleEpochRetention::RuntimeValue)
            .ok_or_else(invalid)?;
        let mut state = self.state.borrow_mut();
        // Distinct zero-sized Rust places can share an address. Give each
        // reservation its own identity; copies of a view still share its key.
        let object = state
            .active_frames
            .values()
            .flat_map(|frame| &frame.roots)
            .find(|root| {
                size_of::<T>() != 0
                    && root.pointer == pointer.cast()
                    && root.places[0].rust_type == TypeId::of::<T>()
            })
            .map(|root| root.object);
        let object = match object {
            Some(object) => object,
            None => {
                static NEXT: AtomicU64 = AtomicU64::new(1);
                HostObjectId(
                    NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                        .map_err(|_| invalid())?,
                )
            }
        };
        let active = state
            .active_frames
            .get_mut(&frame.frame_id)
            .ok_or_else(invalid)?;
        if active.epoch != frame.epoch || frame.table.owner != self.owner {
            return Err(invalid());
        }
        // Identical shared aliases reuse a record. A capability may never be
        // upgraded by merely creating another view of a reserved shared root.
        if let Some(root) = active.roots.iter().find(|root| root.object == object) {
            if !root.places[0]
                .target
                .view(&root.owner)
                .compatible(descriptor.type_argument().view(descriptor.owner()))
                || !root.access.permits(access)
            {
                return Err(invalid());
            }
        } else {
            active
                .roots
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("external host roots"))?;
            active.roots.push(ScopedRoot {
                object,
                pointer: pointer.cast(),
                places: vec![ScopedPlace {
                    rust_type: TypeId::of::<T>(),
                    target: descriptor.type_argument().clone(),
                    path: Arc::default(),
                    projectors: Arc::default(),
                }],
                owner: descriptor.owner().clone(),
                access,
                _program: program,
            });
        }
        Ok(ScopedHostView {
            owner: self.owner,
            frame: frame.frame_id,
            epoch: frame.epoch,
            object,
            place: 0,
            access,
        })
    }

    fn with_root<R>(
        &self,
        view: ScopedHostView,
        f: impl FnOnce(&ScopedRoot) -> NativeResult<R>,
    ) -> NativeResult<R> {
        if view.owner != self.owner {
            return Err(invalid());
        }
        let state = self.state.borrow();
        let frame = state.active_frames.get(&view.frame).ok_or_else(invalid)?;
        if frame.epoch != view.epoch {
            return Err(invalid());
        }
        let root = frame
            .roots
            .iter()
            .find(|root| root.object == view.object)
            .ok_or_else(invalid)?;
        if !root.access.permits(view.access) || root.places.get(view.place).is_none() {
            return Err(invalid());
        }
        f(root)
    }

    pub(crate) fn validate_scoped(&self, view: ScopedHostView) -> NativeResult<()> {
        self.with_root(view, |_| Ok(()))
    }

    pub(crate) fn scoped_matches(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        expected: &TypeArgument,
        access: ReferenceAccess,
        owner: &LoadedModule,
    ) -> bool {
        view.access.permits(access)
            && self
                .with_root(view, |root| {
                    let place = &root.places[view.place];
                    place.target.validate(runtime)?;
                    Ok(place
                        .target
                        .identity(&root.owner)
                        .is_some_and(|actual| Some(actual) == expected.identity(owner)))
                })
                .unwrap_or(false)
    }

    pub(crate) fn scoped_matches_view(
        &self,
        view: ScopedHostView,
        expected: TypeView<'_>,
        access: ReferenceAccess,
    ) -> bool {
        view.access.permits(access)
            && self
                .with_root(view, |root| {
                    Ok(root.places[view.place]
                        .target
                        .view(&root.owner)
                        .compatible(expected))
                })
                .unwrap_or(false)
    }

    fn lend_scoped<T: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        access: ReferenceAccess,
        f: impl FnOnce(*mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        runtime.resources().poll_execution()?;
        if !view.access.permits(access) {
            return Err(RuntimeError::host_borrow_conflict(
                "shared view cannot grant mutable access",
            ));
        }
        let (pointer, place) = self.with_root(view, |root| {
            let place = &root.places[view.place];
            if place.rust_type != TypeId::of::<T>() {
                return Err(invalid());
            }
            place.target.validate(runtime)?;
            Ok((root.pointer, place.clone()))
        })?;
        let loan = self.enter_frame_in(Some(runtime.resources()))?;
        self.borrow_record(
            &loan,
            FrameBorrowRecord {
                object_id: view.object,
                borrow_kind: if access == ReferenceAccess::Mutable {
                    HostBorrowKind::Unique
                } else {
                    HostBorrowKind::Shared
                },
                type_id: None,
                path: place.path.clone(),
            },
        )?;
        let mut pointer = pointer;
        for projector in place.projectors.iter() {
            // SAFETY: Validated generated direct fields start at the scope-reserved
            // live root. No ancestor reference is formed; this path loan is active.
            pointer = unsafe { (projector.project)(pointer) };
        }
        // The temporary loan frame releases its counts on all exits, including
        // unwind. No table borrow spans callback execution or nested validation.
        let result = f(pointer.cast::<T>());
        drop(loan);
        result
    }

    pub(crate) fn project_scoped<T: Send + 'static, F: Send + 'static>(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        field: &RustField<T, F>,
    ) -> NativeResult<ScopedHostView> {
        runtime.resources().poll_execution()?;
        runtime.validate_loaded_module(field.parent.owner())?;
        field.target.validate(runtime)?;
        let place = self.with_root(view, |root| {
            let parent = &root.places[view.place];
            if parent.rust_type != TypeId::of::<T>()
                || !parent
                    .target
                    .view(&root.owner)
                    .compatible(field.parent.type_argument().view(field.parent.owner()))
            {
                return Err(invalid());
            }
            if parent.path.len() >= 64 {
                return Err(RuntimeError::resource_limit("inline host path depth"));
            }
            let mut path = parent.path.to_vec();
            path.try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("inline host path capacity"))?;
            path.push(field.declaration);
            let mut projectors = parent.projectors.to_vec();
            projectors
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("inline host projector capacity"))?;
            projectors.push(field.projector.clone());
            Ok(ScopedPlace {
                rust_type: TypeId::of::<F>(),
                target: field.target.clone(),
                path: path.into(),
                projectors: projectors.into(),
            })
        })?;
        let mut state = self.state.borrow_mut();
        let root = state
            .active_frames
            .get_mut(&view.frame)
            .and_then(|frame| {
                frame
                    .roots
                    .iter_mut()
                    .find(|root| root.object == view.object)
            })
            .ok_or_else(invalid)?;
        let index = if let Some(index) = root
            .places
            .iter()
            .position(|existing| existing.path == place.path)
        {
            index
        } else {
            root.places
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("inline host places"))?;
            root.places.push(place);
            root.places.len() - 1
        };
        Ok(ScopedHostView {
            place: index,
            ..view
        })
    }

    pub(crate) fn read_scoped<T: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        f: impl for<'loan> FnOnce(&'loan T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.lend_scoped(runtime, view, ReferenceAccess::Shared, |pointer| {
            // SAFETY: register_scoped retained a live shared/exclusive T loan.
            // The checked whole-object shared loan excludes every writer, and
            // the higher-ranked closure prevents retention of the Rust reference.
            f(unsafe { &*pointer })
        })
    }

    pub(crate) fn edit_scoped<T: Send + 'static, R>(
        &self,
        runtime: &Runtime,
        view: ScopedHostView,
        f: impl for<'loan> FnOnce(&'loan mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.lend_scoped(runtime, view, ReferenceAccess::Mutable, |pointer| {
            // SAFETY: Only an exclusive host reservation grants this capability;
            // the dynamic unique loan excludes all other active references. Its
            // frame releases on return/error/unwind, after the HRTB reference ends.
            f(unsafe { &mut *pointer })
        })
    }
}

impl Runtime {
    pub(crate) fn matches_scoped_receiver(
        &self,
        value: &Value,
        target: &Ty,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
        access: ReferenceAccess,
    ) -> bool {
        self.scoped_view(value).ok().is_some_and(|view| {
            self.host_borrows.scoped_matches_view(
                view,
                TypeView::new(target, owner, environment),
                access,
            )
        })
    }

    pub(crate) fn scoped_view(&self, value: &Value) -> NativeResult<ScopedHostView> {
        let Value::Ephemeral(id) = value else {
            return Err(invalid());
        };
        let Some(EphemeralValue::HostView(view)) = self.gc().ephemeral(*id) else {
            return Err(invalid());
        };
        self.host_borrows.validate_scoped(view)?;
        Ok(view)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::expired_host_borrow("foreign, expired or incompatible external host view")
}
