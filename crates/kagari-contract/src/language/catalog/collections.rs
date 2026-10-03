//! Collection contracts carry capabilities, never an allocator or hash algorithm.
use crate::language::catalog::contracts::{
    applied_item, boolean, contract, method, option, receiver, unit, usize_type,
};
use crate::{
    declaration::ModuleDecl,
    language::{Protocol, primitive},
    types::Ty,
};
use kagari_common::identity::associated_type_id;

pub(super) fn declare(module: &mut ModuleDecl) {
    let mut list = contract(Protocol::List, &["T"]);
    let item = list.generic_params[0].as_type();
    let mut index = primitive::applied(Protocol::Index, vec![usize_type()]);
    index.associated_types.insert(
        associated_type_id(&index.declaration, "Output"),
        item.clone(),
    );
    list.supertraits
        .extend([index, applied_item(Protocol::Iterable, item.clone())]);
    let this = receiver(Protocol::List);
    list.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("get", vec![this, usize_type()], option(item)),
    ]);
    module.traits.push(list);
    let mut writable = contract(Protocol::MutableList, &["T"]);
    let item = writable.generic_params[0].as_type();
    writable
        .supertraits
        .push(primitive::applied(Protocol::List, vec![item.clone()]));
    let this = receiver(Protocol::MutableList);
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
    let mut map = contract(Protocol::Map, &["K", "V"]);
    let key = map.generic_params[0].as_type();
    let value = map.generic_params[1].as_type();
    let this = receiver(Protocol::Map);
    map.supertraits.push(applied_item(
        Protocol::Iterable,
        Ty::Tuple(vec![key.clone(), value.clone()]),
    ));
    map.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains_key", vec![this.clone(), key.clone()], boolean()),
        method("get", vec![this, key], option(value)),
    ]);
    module.traits.push(map);
    let mut writable = contract(Protocol::MutableMap, &["K", "V"]);
    let key = writable.generic_params[0].as_type();
    let value = writable.generic_params[1].as_type();
    let this = receiver(Protocol::MutableMap);
    writable.supertraits.push(primitive::applied(
        Protocol::Map,
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
    let mut set = contract(Protocol::Set, &["T"]);
    let item = set.generic_params[0].as_type();
    let this = receiver(Protocol::Set);
    set.supertraits
        .push(applied_item(Protocol::Iterable, item.clone()));
    set.methods.extend([
        method("len", vec![this.clone()], usize_type()),
        method("is_empty", vec![this.clone()], boolean()),
        method("contains", vec![this, item], boolean()),
    ]);
    module.traits.push(set);
    let mut writable = contract(Protocol::MutableSet, &["T"]);
    let item = writable.generic_params[0].as_type();
    let this = receiver(Protocol::MutableSet);
    writable
        .supertraits
        .push(primitive::applied(Protocol::Set, vec![item.clone()]));
    writable.methods.extend([
        method("insert", vec![this.clone(), item.clone()], unit()),
        method("remove", vec![this.clone(), item], boolean()),
        method("clear", vec![this], unit()),
    ]);
    module.traits.push(writable);
}
