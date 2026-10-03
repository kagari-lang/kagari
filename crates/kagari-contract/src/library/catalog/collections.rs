//! Collection contracts carry capabilities, never an allocator or hash algorithm.
use crate::library::catalog::contracts::{
    applied_item, boolean, contract, method, option, receiver, unit, usize_type,
};
use crate::library::catalog::key::{self, RegistrationTrait};
use crate::{declaration::ModuleDecl, types::Ty};
use kagari_common::collection::CollectionAccess;
use kagari_common::identity::associated_type_id;

pub(super) fn declare(module: &mut ModuleDecl) {
    let mut list = contract(RegistrationTrait::List, &["T"]);
    list.storage_access = Some(CollectionAccess::ReadOnly);
    let item = list.generic_params[0].as_type();
    let mut index = key::applied(RegistrationTrait::Index, vec![usize_type()]);
    index.associated_types.insert(
        associated_type_id(&index.declaration, "Output"),
        item.clone(),
    );
    list.supertraits.extend([
        index,
        applied_item(RegistrationTrait::Iterable, item.clone()),
    ]);
    let this = receiver(RegistrationTrait::List);
    list.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("get", vec![this, usize_type()], option(item)),
    ]);
    module.traits.push(list);
    let mut writable = contract(RegistrationTrait::MutableList, &["T"]);
    writable.storage_access = Some(CollectionAccess::Mutable);
    let item = writable.generic_params[0].as_type();
    writable
        .supertraits
        .push(key::applied(RegistrationTrait::List, vec![item.clone()]));
    let this = receiver(RegistrationTrait::MutableList);
    writable.methods.extend([
        method("push", vec![this.clone(), item.clone()], unit()),
        method("pop", vec![this.clone()], option(item.clone())),
        method(
            "insert",
            vec![this.clone(), usize_type(), item.clone()],
            unit(),
        ),
        method(
            "remove",
            vec![this.clone(), usize_type()],
            option(item.clone()),
        ),
        method("clear", vec![this.clone()], unit()),
        method("set", vec![this, usize_type(), item], unit()),
    ]);
    module.traits.push(writable);
    let mut map = contract(RegistrationTrait::Map, &["K", "V"]);
    map.storage_access = Some(CollectionAccess::ReadOnly);
    let key = map.generic_params[0].as_type();
    let value = map.generic_params[1].as_type();
    let this = receiver(RegistrationTrait::Map);
    map.supertraits.push(applied_item(
        RegistrationTrait::Iterable,
        Ty::Tuple(vec![key.clone(), value.clone()]),
    ));
    map.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains_key", vec![this.clone(), key.clone()], boolean()),
        method("get", vec![this, key], option(value)),
    ]);
    module.traits.push(map);
    let mut writable = contract(RegistrationTrait::MutableMap, &["K", "V"]);
    writable.storage_access = Some(CollectionAccess::Mutable);
    let key = writable.generic_params[0].as_type();
    let value = writable.generic_params[1].as_type();
    let this = receiver(RegistrationTrait::MutableMap);
    writable.supertraits.push(key::applied(
        RegistrationTrait::Map,
        vec![key.clone(), value.clone()],
    ));
    writable.methods.extend([
        method(
            "insert",
            vec![this.clone(), key.clone(), value.clone()],
            unit(),
        ),
        method("remove", vec![this.clone(), key], option(value)),
        method("clear", vec![this], unit()),
    ]);
    module.traits.push(writable);
    let mut set = contract(RegistrationTrait::Set, &["T"]);
    set.storage_access = Some(CollectionAccess::ReadOnly);
    let item = set.generic_params[0].as_type();
    let this = receiver(RegistrationTrait::Set);
    set.supertraits
        .push(applied_item(RegistrationTrait::Iterable, item.clone()));
    set.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains", vec![this, item], boolean()),
    ]);
    module.traits.push(set);
    let mut writable = contract(RegistrationTrait::MutableSet, &["T"]);
    writable.storage_access = Some(CollectionAccess::Mutable);
    let item = writable.generic_params[0].as_type();
    let this = receiver(RegistrationTrait::MutableSet);
    writable
        .supertraits
        .push(key::applied(RegistrationTrait::Set, vec![item.clone()]));
    writable.methods.extend([
        method("insert", vec![this.clone(), item.clone()], unit()),
        method("remove", vec![this.clone(), item], boolean()),
        method("clear", vec![this], unit()),
    ]);
    module.traits.push(writable);
}
