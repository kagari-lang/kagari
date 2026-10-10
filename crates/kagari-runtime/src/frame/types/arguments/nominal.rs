//! Nominal type admission reuses complete layout identities without retaining scopes.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    module::{EnumVariantRef, LoadedModule, StructLayoutRef, layout_admission::NominalAdmission},
};
use kagari_bytecode::instruction::StructId;
use kagari_types::ty::Ty;

impl TypeArgument {
    pub(crate) fn prepare_admission(&self, runtime: &Runtime, fallback: &LoadedModule) {
        if self.data.admission.get().is_some() || !matches!(self.ty(), Ty::Struct(_) | Ty::Enum(_))
        {
            return;
        }
        if let Some(admission) = self.nominal_admission(runtime, fallback) {
            let _ = self.data.admission.set(Box::new(admission));
        }
    }

    fn nominal_admission(
        &self,
        runtime: &Runtime,
        fallback: &LoadedModule,
    ) -> Option<NominalAdmission> {
        // Missing identities retain full comparisons. Cache only successful facts:
        // detached preparation must not poison later installed application admission.
        self.validate(runtime).ok()?;
        match self.ty() {
            Ty::Struct(nominal) => {
                let view = self.view(fallback).normalized()?;
                let (owner, id) = view.owner.find_struct_definition(nominal)?;
                let arguments = (0..nominal.arguments.len())
                    .map(|index| self.parameter(runtime, fallback, index))
                    .collect::<Result<Vec<_>, _>>()
                    .ok()?;
                let layout = runtime
                    .prepare_struct_application(&owner, id, &arguments)
                    .ok()?;
                NominalAdmission::structure(&layout)
            }
            Ty::Enum(_) => runtime
                .prepare_enum_layout(fallback, self)
                .ok()?
                .and_then(|layout| NominalAdmission::enumeration(&layout)),
            _ => None,
        }
    }
}

impl Runtime {
    pub(crate) fn prepare_struct_application(
        &self,
        owner: &LoadedModule,
        id: StructId,
        arguments: &[TypeArgument],
    ) -> Result<StructLayoutRef, RuntimeError> {
        let template = owner
            .bytecode
            .structures
            .get(id.index())
            .ok_or_else(|| RuntimeError::module_validation("missing struct layout"))?;
        for argument in arguments {
            argument.validate(self)?;
        }
        for (compiled, supplied) in template.arguments.iter().zip(arguments) {
            if compiled.is_concrete()
                && !supplied
                    .view(owner)
                    .compatible(TypeView::new(compiled, owner, None))
            {
                return Err(RuntimeError::module_validation(
                    "struct application differs from its compiled argument scope",
                ));
            }
        }
        let scope = self.prepare_layout_scope(owner, template.declaration, arguments)?;
        let types = arguments
            .iter()
            .map(|argument| argument.ty().clone())
            .collect::<Vec<_>>();
        self.modules
            .applied_struct_layout(owner, id, &types, scope)
            .ok_or_else(|| RuntimeError::module_validation("invalid struct application"))
    }

    pub(crate) fn prepare_enum_variants(
        &self,
        fallback: &LoadedModule,
        applied: &TypeArgument,
    ) -> Result<Vec<EnumVariantRef>, RuntimeError> {
        match self.prepare_enum_layout(fallback, applied)? {
            Some(layout) => enum_variants(&layout),
            None => Ok(Vec::new()),
        }
    }

    pub(crate) fn prepare_enum_layout(
        &self,
        fallback: &LoadedModule,
        applied: &TypeArgument,
    ) -> Result<Option<EnumVariantRef>, RuntimeError> {
        let Ty::Enum(nominal) = applied.ty() else {
            return Err(RuntimeError::module_validation(
                "native operation requires an enum type",
            ));
        };
        let view = applied
            .view(fallback)
            .normalized()
            .ok_or_else(|| RuntimeError::module_validation("native enum type scope"))?;
        let (owner, id) = view.owner.find_enum_definition(nominal).ok_or_else(|| {
            RuntimeError::module_validation("native enum layout is absent from the pinned program")
        })?;
        let template = &owner.bytecode.enumerations[id.index()];
        if template.variants.is_empty() {
            return Ok(None);
        }
        let arguments = (0..nominal.arguments.len())
            .map(|position| applied.parameter(self, fallback, position))
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let generic = template.arguments.iter().enumerate().all(|(position, ty)| {
            matches!(ty, Ty::Parameter { owner, position: slot } if *owner == nominal.declaration && *slot == position)
        });
        let scope = if arguments.iter().any(TypeArgument::has_origin) && !generic {
            if !template
                .arguments
                .iter()
                .zip(&arguments)
                .all(|(compiled, supplied)| {
                    compiled.is_concrete()
                        && supplied
                            .view(fallback)
                            .compatible(TypeView::new(compiled, &owner, None))
                })
            {
                return Err(RuntimeError::module_validation(
                    "scoped enum payload differs from its concrete layout",
                ));
            }
            None
        } else {
            self.prepare_layout_scope(&owner, nominal.declaration, &arguments)?
        };
        let layout = self
            .modules
            .applied_enum_variant(&owner, id, &nominal.arguments, 0, scope)
            .ok_or_else(|| RuntimeError::module_validation("native enum layout application"))?;
        Ok(Some(layout))
    }
}

fn enum_variants(layout: &EnumVariantRef) -> Result<Vec<EnumVariantRef>, RuntimeError> {
    (0..layout.layout().variants.len())
        .map(|index| {
            u32::try_from(index)
                .ok()
                .and_then(|index| layout.with_variant(index))
                .ok_or_else(|| RuntimeError::module_validation("enum variant index"))
        })
        .collect()
}
