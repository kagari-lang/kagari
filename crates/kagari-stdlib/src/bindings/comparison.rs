//! Library-authored collection equality and hashing over checked reads and callbacks.
mod readers;
use crate::bindings::comparison::readers::{Cursor, Reader};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::{NativeBinding, NativeResult},
        declarations::SelectedCall,
        interfaces::Interface,
        selected_method::SelectedMethod,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
};
use kagari_types::declaration::NativeDeclaration;
use std::iter;

#[derive(Clone, Copy)]
enum Shape {
    List,
    Set,
    Map,
}

impl Shape {
    fn components(self) -> usize {
        if matches!(self, Self::Map) { 2 } else { 1 }
    }
}

type Equality = SelectedMethod<(ScriptValue, ScriptValue), bool>;
type Hash = SelectedMethod<(ScriptValue,), i64>;

pub(super) fn binding(
    name: &str,
    declaration: &DefinitionMetadata<NativeDeclaration>,
) -> NativeResult<Option<NativeBinding>> {
    match name {
        name if name.starts_with("$foundation_interface_") && name.ends_with("_eq") => {
            return Ok(Some(NativeBinding::declared(
                declaration,
                |cx, (left, right): (Interface, Interface)| left.receiver_equal(cx, &right),
            )));
        }
        name if name.starts_with("$foundation_interface_") && name.ends_with("_debug") => {
            return Ok(Some(NativeBinding::declared(
                declaration,
                |cx, (value,): (Interface,)| value.debug_preview(cx),
            )));
        }
        _ => {}
    }
    let (shape, public, hash) = match name {
        "list_content_equal" => (Shape::List, true, false),
        "set_content_equal" => (Shape::Set, true, false),
        "map_content_equal" => (Shape::Map, true, false),
        "$foundation_list_eq" => (Shape::List, false, false),
        "$foundation_set_eq" => (Shape::Set, false, false),
        "$foundation_map_eq" => (Shape::Map, false, false),
        "$foundation_list_hash" => (Shape::List, false, true),
        "$foundation_set_hash" => (Shape::Set, false, true),
        "$foundation_map_hash" => (Shape::Map, false, true),
        _ => return Ok(None),
    };
    let calls = declaration
        .records()
        .callable_requirements
        .iter()
        .map(|required| SelectedCall::from_declaration(declaration, required))
        .collect::<NativeResult<Vec<_>>>()?;
    if calls.len() != shape.components() + if public { 0 } else { 3 } {
        return Err(RuntimeError::metadata_conflict(
            "collection comparison requirements",
        ));
    }
    Ok(Some(if public {
        NativeBinding::declared(
            declaration,
            move |cx, (left, right): (Interface, Interface)| {
                let left = Reader::interface(cx, left)?;
                let right = Reader::interface(cx, right)?;
                equal(cx, shape, left, right, &calls)
            },
        )
    } else if hash {
        NativeBinding::declared(declaration, move |cx, (value,): (ScriptValue,)| {
            let source = Reader::concrete(cx, value, &calls[shape.components()..])?;
            content_hash(cx, shape, source, &calls[..shape.components()])
        })
    } else {
        NativeBinding::declared(
            declaration,
            move |cx, (left, right): (ScriptValue, ScriptValue)| {
                let left = Reader::concrete(cx, left, &calls[shape.components()..])?;
                let right = Reader::concrete(cx, right, &calls[shape.components()..])?;
                equal(cx, shape, left, right, &calls[..shape.components()])
            },
        )
    }))
}

fn equal(
    cx: &mut NativeContext<'_>,
    shape: Shape,
    left: Reader,
    right: Reader,
    calls: &[SelectedCall],
) -> NativeResult<bool> {
    let count = left.len(cx)?;
    if count != right.len(cx)? {
        return Ok(false);
    }
    let comparisons = calls
        .iter()
        .map(|call| cx.selected_method(call))
        .collect::<NativeResult<Vec<Equality>>>()?;
    let left = left.cursor(cx)?;
    let right = right.cursor(cx)?;
    if matches!(shape, Shape::List) {
        for _ in 0..count {
            let a = left.item(cx)?;
            let b = right.item(cx)?;
            if !comparisons[0].call(cx, (a, b))? {
                return Ok(false);
            }
        }
        left.finish(cx)?;
        right.finish(cx)?;
        return Ok(true);
    }
    unordered(cx, shape, left, right, count, &comparisons)
}

fn entries_equal(
    cx: &mut NativeContext<'_>,
    shape: Shape,
    a: &ScriptValue,
    b: &ScriptValue,
    comparisons: &[Equality],
) -> NativeResult<bool> {
    if matches!(shape, Shape::Map) {
        let (ak, av) = a.decode::<(ScriptValue, ScriptValue)>(cx)?;
        let (bk, bv) = b.decode::<(ScriptValue, ScriptValue)>(cx)?;
        Ok(comparisons[0].call(cx, (ak, bk))? && comparisons[1].call(cx, (av, bv))?)
    } else {
        comparisons[0].call(cx, (a.clone(), b.clone()))
    }
}

fn push<T>(values: &mut Vec<T>, value: T) -> NativeResult<()> {
    values
        .try_reserve(1)
        .map_err(|_| RuntimeError::resource_limit("collection comparison working state"))?;
    values.push(value);
    Ok(())
}

fn unordered(
    cx: &mut NativeContext<'_>,
    shape: Shape,
    left: Cursor,
    right: Cursor,
    count: usize,
    comparisons: &[Equality],
) -> NativeResult<bool> {
    let mut items = Vec::new();
    for _ in 0..count {
        push(&mut items, right.item(cx)?)?;
    }
    right.finish(cx)?;
    let mut candidates = Vec::new();
    for _ in 0..count {
        let a = left.item(cx)?;
        let mut matches = Vec::new();
        for (index, b) in items.iter().enumerate() {
            cx.poll()?;
            if entries_equal(cx, shape, &a, b, comparisons)? {
                push(&mut matches, index)?;
            }
        }
        if matches.is_empty() {
            return Ok(false);
        }
        push(&mut candidates, matches)?;
    }
    left.finish(cx)?;
    perfect_matching(cx, &candidates)
}

// Augmenting paths avoid greedily choosing the wrong value when a custom Map's
// lookup relation allows several keys equal under the helper's selected Eq.
fn perfect_matching(cx: &NativeContext<'_>, candidates: &[Vec<usize>]) -> NativeResult<bool> {
    let count = candidates.len();
    let mut owners: Vec<Option<usize>> = cx.collect(iter::repeat_n(None, count))?;
    let mut matches: Vec<Option<usize>> = cx.collect(iter::repeat_n(None, count))?;
    for start in 0..count {
        let mut parents: Vec<Option<usize>> = cx.collect(iter::repeat_n(None, count))?;
        let mut visited = cx.collect(iter::repeat_n(false, count))?;
        let mut queue = Vec::new();
        push(&mut queue, start)?;
        visited[start] = true;
        let mut head = 0;
        let mut free = None;
        while head < queue.len() && free.is_none() {
            let left = queue[head];
            head += 1;
            for &right in &candidates[left] {
                cx.poll()?;
                if parents[right].is_some() {
                    continue;
                }
                parents[right] = Some(left);
                if let Some(previous) = owners[right] {
                    if !visited[previous] {
                        visited[previous] = true;
                        push(&mut queue, previous)?;
                    }
                } else {
                    free = Some(right);
                    break;
                }
            }
        }
        let Some(mut right) = free else {
            return Ok(false);
        };
        loop {
            cx.poll()?;
            let left = parents[right].expect("visited matching edge");
            let previous = matches[left].replace(right);
            owners[right] = Some(left);
            let Some(previous) = previous else {
                break;
            };
            right = previous;
        }
    }
    Ok(true)
}

const SEED: u64 = 14695981039346656037;
const PRIME: u64 = 1099511628211;

fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(PRIME)
}

fn content_hash(
    cx: &mut NativeContext<'_>,
    shape: Shape,
    source: Reader,
    calls: &[SelectedCall],
) -> NativeResult<i64> {
    let count = source.len(cx)?;
    let hashes = calls
        .iter()
        .map(|call| cx.selected_method(call))
        .collect::<NativeResult<Vec<Hash>>>()?;
    let cursor = source.cursor(cx)?;
    let mut result = if matches!(shape, Shape::List) {
        SEED
    } else {
        0
    };
    for _ in 0..count {
        let value = cursor.item(cx)?;
        result = match shape {
            Shape::List => mix(result, hashes[0].call(cx, (value,))? as u64),
            Shape::Set => result.wrapping_add(mix(SEED, hashes[0].call(cx, (value,))? as u64)),
            Shape::Map => {
                let (key, value) = value.decode::<(ScriptValue, ScriptValue)>(cx)?;
                let key = hashes[0].call(cx, (key,))? as u64;
                let value = hashes[1].call(cx, (value,))? as u64;
                result.wrapping_add(mix(mix(SEED, key), value))
            }
        };
    }
    cursor.finish(cx)?;
    Ok(mix(result, count as u64) as i64)
}
