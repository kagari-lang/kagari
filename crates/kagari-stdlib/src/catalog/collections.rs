//! Collection contracts carry capabilities, never an allocator or hash algorithm.
use crate::catalog::{
    contracts::{applied_item, boolean, contract, method, option, receiver, unit, usize_type},
    identities,
    key::{self, RegistrationTrait},
};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    collection::CollectionAccess,
    declaration::module::{DeclarationError, ModuleDecl},
    ty::Ty,
};

pub(super) fn declare(
    ids: &DefinitionContext,
    module: &mut ModuleDecl,
) -> Result<(), DeclarationError> {
    let mut list = contract(ids, RegistrationTrait::List, &["T"])?;
    list.storage_access = Some(CollectionAccess::ReadOnly);
    let item = list.generic_params[0].as_type();
    let mut index = key::applied(ids, RegistrationTrait::Index, vec![usize_type()])?;
    index.associated_types.insert(
        identities::associated(ids, &index.declaration, "Output")?,
        item.clone(),
    );
    list.supertraits.extend([
        index,
        applied_item(ids, RegistrationTrait::Iterable, item.clone())?,
    ]);
    let this = receiver(ids, RegistrationTrait::List)?;
    list.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("get", vec![this, usize_type()], option(ids, item)?),
    ]);
    module.traits.push(list);
    let mut writable = contract(ids, RegistrationTrait::MutableList, &["T"])?;
    writable.storage_access = Some(CollectionAccess::Mutable);
    let item = writable.generic_params[0].as_type();
    writable.supertraits.push(key::applied(
        ids,
        RegistrationTrait::List,
        vec![item.clone()],
    )?);
    let this = receiver(ids, RegistrationTrait::MutableList)?;
    writable.methods.extend([
        method("push", vec![this.clone(), item.clone()], unit()),
        method("pop", vec![this.clone()], option(ids, item.clone())?),
        method(
            "insert",
            vec![this.clone(), usize_type(), item.clone()],
            unit(),
        ),
        method(
            "remove",
            vec![this.clone(), usize_type()],
            option(ids, item.clone())?,
        ),
        method("clear", vec![this.clone()], unit()),
        method("set", vec![this, usize_type(), item], unit()),
    ]);
    module.traits.push(writable);
    let mut map = contract(ids, RegistrationTrait::Map, &["K", "V"])?;
    map.storage_access = Some(CollectionAccess::ReadOnly);
    let key = map.generic_params[0].as_type();
    let value = map.generic_params[1].as_type();
    let this = receiver(ids, RegistrationTrait::Map)?;
    map.supertraits.push(applied_item(
        ids,
        RegistrationTrait::Iterable,
        Ty::Tuple(vec![key.clone(), value.clone()]),
    )?);
    map.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains_key", vec![this.clone(), key.clone()], boolean()),
        method("get", vec![this, key], option(ids, value)?),
    ]);
    module.traits.push(map);
    let mut writable = contract(ids, RegistrationTrait::MutableMap, &["K", "V"])?;
    writable.storage_access = Some(CollectionAccess::Mutable);
    let key = writable.generic_params[0].as_type();
    let value = writable.generic_params[1].as_type();
    let this = receiver(ids, RegistrationTrait::MutableMap)?;
    writable.supertraits.push(key::applied(
        ids,
        RegistrationTrait::Map,
        vec![key.clone(), value.clone()],
    )?);
    writable.methods.extend([
        method(
            "insert",
            vec![this.clone(), key.clone(), value.clone()],
            unit(),
        ),
        method("remove", vec![this.clone(), key], option(ids, value)?),
        method("clear", vec![this], unit()),
    ]);
    module.traits.push(writable);
    let mut set = contract(ids, RegistrationTrait::Set, &["T"])?;
    set.storage_access = Some(CollectionAccess::ReadOnly);
    let item = set.generic_params[0].as_type();
    let this = receiver(ids, RegistrationTrait::Set)?;
    set.supertraits.push(applied_item(
        ids,
        RegistrationTrait::Iterable,
        item.clone(),
    )?);
    set.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains", vec![this, item], boolean()),
    ]);
    module.traits.push(set);
    let mut writable = contract(ids, RegistrationTrait::MutableSet, &["T"])?;
    writable.storage_access = Some(CollectionAccess::Mutable);
    let item = writable.generic_params[0].as_type();
    let this = receiver(ids, RegistrationTrait::MutableSet)?;
    writable.supertraits.push(key::applied(
        ids,
        RegistrationTrait::Set,
        vec![item.clone()],
    )?);
    writable.methods.extend([
        method("insert", vec![this.clone(), item.clone()], unit()),
        method("remove", vec![this.clone(), item], boolean()),
        method("clear", vec![this], unit()),
    ]);
    module.traits.push(writable);

    Ok(())
}
