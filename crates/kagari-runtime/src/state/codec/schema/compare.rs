use crate::{
    native::conversion::context::ConversionContext,
    state::{
        codec::walk::Walk,
        data::{NominalKey, StateType},
        error::SnapshotResult,
    },
};
use std::cmp::Ordering;

pub(super) fn key(
    cx: &ConversionContext<'_>,
    a: &NominalKey,
    b: &NominalKey,
    walk: &mut Walk,
) -> SnapshotResult<Ordering> {
    walk.poll(cx)?;
    walk.budget.work(1)?;
    let order = walk.compare_text(cx, &a.package, &b.package)?;
    if !order.is_eq() {
        return Ok(order);
    }
    for (a, b) in [(&a.module, &b.module), (&a.declaration, &b.declaration)] {
        for (a, b) in a.iter().zip(b) {
            let order = walk.compare_text(cx, a, b)?;
            if !order.is_eq() {
                return Ok(order);
            }
        }
        if a.len() != b.len() {
            return Ok(a.len().cmp(&b.len()));
        }
    }
    types(cx, &a.arguments, &b.arguments, walk)
}

fn types(
    cx: &ConversionContext<'_>,
    a: &[StateType],
    b: &[StateType],
    walk: &mut Walk,
) -> SnapshotResult<Ordering> {
    for (a, b) in a.iter().zip(b) {
        let order = ty(cx, a, b, walk)?;
        if !order.is_eq() {
            return Ok(order);
        }
    }
    Ok(a.len().cmp(&b.len()))
}

fn ty(
    cx: &ConversionContext<'_>,
    a: &StateType,
    b: &StateType,
    walk: &mut Walk,
) -> SnapshotResult<Ordering> {
    walk.poll(cx)?;
    walk.budget.work(1)?;
    let rank = |ty: &StateType| match ty {
        StateType::Scalar(_) => 0,
        StateType::Tuple(_) => 1,
        StateType::Sequence(_) => 2,
        StateType::Nominal(_) => 3,
    };
    match (a, b) {
        (StateType::Scalar(a), StateType::Scalar(b)) => Ok(a.cmp(b)),
        (StateType::Sequence(a), StateType::Sequence(b)) => ty(cx, a, b, walk),
        (StateType::Tuple(a), StateType::Tuple(b)) => types(cx, a, b, walk),
        (StateType::Nominal(a), StateType::Nominal(b)) => key(cx, a, b, walk),
        _ => Ok(rank(a).cmp(&rank(b))),
    }
}
