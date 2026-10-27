//! Compare the applied storage closure, not a generation's specialization inventory.
//! Values (including empty variants) never substitute for declared schema evidence.
use crate::{frame::types::compatibility::TypeView, module::ExecutableModule};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::types::{trait_contract, type_contract};
use kagari_types::{
    declaration::{FnDecl, TraitDef, TypeDef},
    ty::{
        GenericBound, NominalTy, Ty,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, resolve_associated_outputs},
    },
};
use std::collections::HashSet;

#[derive(PartialEq, Eq)]
struct Schema {
    external: Option<TypeDef>,
    public_type: Option<TypeDef>,
    interface: Option<TraitDef>,
}

fn schema(program: &ExecutableModule, id: DefinitionId) -> Option<Schema> {
    let identity = program.definitions().resolve(id).ok()?.module().clone();
    let module = program
        .members()
        .find(|member| member.bytecode.identity == identity);
    let mut external = None;
    for member in program.members() {
        for record in &member.bytecode.external_types {
            if record.declaration == id {
                if external
                    .as_ref()
                    .is_some_and(|previous| previous != &record.definition)
                {
                    return None;
                }
                external = Some(record.definition.clone());
            }
        }
    }
    let public_type = module.as_ref().and_then(|module| {
        type_contract(
            &identity,
            &module.bytecode.public_items,
            &id,
            module.definitions(),
        )
        .cloned()
    });
    let interface = module.as_ref().and_then(|module| {
        trait_contract(
            &identity,
            &module.bytecode.public_items,
            &module.bytecode.trait_contracts,
            &id,
            module.definitions(),
        )
        .cloned()
    });
    Some(Schema {
        external,
        public_type,
        interface,
    })
}

fn bound_types(bounds: &[GenericBound], types: &mut Vec<Ty>) {
    for bound in bounds {
        types.push(bound.ty.clone());
        types.extend(bound.constraints.iter().cloned().map(Ty::Trait));
    }
}

fn function_types(function: &FnDecl, types: &mut Vec<Ty>) {
    types.extend(function.params.iter().map(|param| param.ty.clone()));
    types.push(function.return_type.clone());
    bound_types(&function.bounds, types);
}

impl Schema {
    fn has_type(&self) -> bool {
        self.external.is_some() || self.public_type.is_some()
    }

    /// Declaration contracts remain exact, but follow their types with the current
    /// owner's arguments substituted. Method-owned binders stay symbolic; their
    /// aggregate templates are compared without inventing concrete applications.
    fn append_types(&self, nominal: &NominalTy, pending: &mut Vec<Ty>) -> Option<()> {
        let mut types = Vec::new();
        for definition in self.external.iter().chain(&self.public_type) {
            types.extend(definition.fields.iter().map(|field| field.ty.clone()));
            types.extend(
                definition
                    .variants
                    .iter()
                    .flat_map(|variant| variant.payload.clone()),
            );
            bound_types(&definition.bounds, &mut types);
        }
        if let Some(interface) = &self.interface {
            types.extend(interface.supertraits.iter().cloned().map(Ty::Trait));
            bound_types(&interface.bounds, &mut types);
            for method in &interface.methods {
                function_types(method, &mut types);
            }
            types.extend(
                interface
                    .associated_consts
                    .iter()
                    .map(|item| item.ty.clone()),
            );
            for item in &interface.associated_types {
                bound_types(&item.parameter_bounds, &mut types);
                types.extend(item.bounds.iter().cloned().map(Ty::Trait));
            }
        }
        let receiver = Ty::Trait(nominal.clone());
        let mut substitution =
            TypeSubstitution::for_owner(&nominal.declaration, &nominal.arguments);
        if self.interface.is_some() {
            substitution.bind_receiver(&nominal.declaration, &receiver);
        }
        for ty in types {
            let applied = substitution.apply(&ty, &Default::default()).ok()?;
            pending.push(if self.interface.is_some() {
                resolve_associated_outputs(&applied, nominal, &receiver, &Default::default())
                    .ok()?
            } else {
                applied
            });
        }
        Some(())
    }
}

fn applied_aggregate(
    ty: &Ty,
    nominal: &NominalTy,
    old: &ExecutableModule,
    new: &ExecutableModule,
    schema: &Schema,
    pending: &mut Vec<Ty>,
) -> Option<()> {
    match ty {
        Ty::Struct(_) => {
            // Use the same template-preferred lookup as runtime value admission.
            // Comparing a subset of old rows would miss a new selected template.
            match (
                old.find_struct_definition(nominal),
                new.find_struct_definition(nominal),
            ) {
                (Some((a, ai)), Some((b, bi))) => {
                    let a = a.bytecode.structures[ai.index()]
                        .apply(&nominal.arguments, &Default::default())?;
                    let b = b.bytecode.structures[bi.index()]
                        .apply(&nominal.arguments, &Default::default())?;
                    if a != b {
                        return None;
                    }
                    pending.extend(a.fields.iter().map(|field| field.ty.clone()));
                }
                (None, None) if schema.has_type() => {}
                _ => return None,
            }
        }
        Ty::Enum(_) => {
            match (
                old.find_enum_definition(nominal),
                new.find_enum_definition(nominal),
            ) {
                (Some((a, ai)), Some((b, bi))) => {
                    let a = a.bytecode.enumerations[ai.index()]
                        .apply(&nominal.arguments, &Default::default())?;
                    let b = b.bytecode.enumerations[bi.index()]
                        .apply(&nominal.arguments, &Default::default())?;
                    if a != b {
                        return None;
                    }
                    pending.extend(
                        a.variants
                            .iter()
                            .flat_map(|variant| variant.payload.clone()),
                    );
                }
                (None, None) if schema.has_type() => {}
                _ => return None,
            }
        }
        Ty::NativeObject(_) if !schema.has_type() => return None,
        Ty::Trait(_) if schema.interface.is_none() => return None,
        _ => {}
    }
    Some(())
}

pub(super) fn compatible(ty: &Ty, old: &ExecutableModule, new: &ExecutableModule) -> bool {
    if !TypeView::new(ty, old, None).compatible(TypeView::new(ty, new, None)) {
        return false;
    }
    applied_closure(ty, old, new).is_some()
}

fn applied_closure(ty: &Ty, old: &ExecutableModule, new: &ExecutableModule) -> Option<()> {
    let mut pending = vec![ty.clone()];
    let mut seen = HashSet::new();
    while let Some(ty) = pending.pop() {
        if !seen.insert(ty.clone()) {
            continue;
        }
        // Recursive generic expansion is bounded just like executable type transforms.
        if seen.len() > MAX_TYPE_NODES || !ty.within_wire_limits() {
            return None;
        }
        match &ty {
            Ty::Struct(nominal)
            | Ty::Enum(nominal)
            | Ty::NativeObject(nominal)
            | Ty::Trait(nominal) => {
                let left = schema(old, nominal.declaration)?;
                let right = schema(new, nominal.declaration)?;
                if left != right {
                    return None;
                }
                applied_aggregate(&ty, nominal, old, new, &left, &mut pending)?;
                left.append_types(nominal, &mut pending)?;
                pending.extend(nominal.arguments.iter().cloned());
                pending.extend(nominal.associated_types.values().cloned());
            }
            Ty::Host(id) => {
                let left = schema(old, *id)?;
                if left != schema(new, *id)? || !left.has_type() {
                    return None;
                }
                left.append_types(
                    &NominalTy {
                        declaration: *id,
                        arguments: vec![],
                        associated_types: Default::default(),
                    },
                    &mut pending,
                )?;
            }
            Ty::Tuple(items) => pending.extend(items.iter().cloned()),
            Ty::Array(item)
            | Ty::Iter(item)
            | Ty::Range(item, _)
            | Ty::Reference { target: item, .. } => pending.push(item.as_ref().clone()),

            Ty::Function { params, result } => {
                pending.extend(params.iter().cloned());
                pending.push(result.as_ref().clone());
            }
            Ty::Projection {
                receiver,
                interface,
                arguments,
                ..
            } => {
                pending.push(receiver.as_ref().clone());
                pending.push(Ty::Trait(interface.as_ref().clone()));
                pending.extend(arguments.iter().cloned());
            }
            Ty::Builtin(_) | Ty::Parameter { .. } | Ty::SelfType(_) => {}
        }
    }
    Some(())
}
