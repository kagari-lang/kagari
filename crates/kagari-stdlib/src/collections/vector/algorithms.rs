//! Callback algorithms keep all authoritative elements in their installed payload.
#[cfg(feature = "execution-diagnostics")]
use crate::collections::diagnostics;
use crate::collections::vector::{
    ScriptVec,
    storage::{Work, recover},
};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        conversion::FromKagari,
        storage::{NativePayload, access::EditPlan},
        typed::NativeContext,
        value_handle::ScriptValue,
        views::NativeOperationGuard,
    },
};
use std::cmp::Ordering;
#[cfg(feature = "execution-diagnostics")]
use std::time::Instant;

impl<T: FromKagari> ScriptVec<T> {
    /// Stable fixed-split mergesort with shorter-run scratch in the same payload.
    /// A failed callback stops immediately; recovery preserves the original multiset.
    pub fn sort_by(
        &self,
        cx: &mut NativeContext<'_>,
        mut compare: impl FnMut(&mut NativeContext<'_>, T, T) -> NativeResult<Ordering>,
    ) -> NativeResult<()> {
        self.sort_retained(cx, |cx, left, right| {
            let left = left.decode(cx)?;
            let right = right.decode(cx)?;
            compare(cx, left, right)
        })
    }

    /// Library callbacks already consume retained script values, so no host
    /// conversion or additional root registration is needed for each argument.
    pub(crate) fn sort_retained(
        &self,
        cx: &mut NativeContext<'_>,
        mut compare: impl FnMut(
            &mut NativeContext<'_>,
            ScriptValue,
            ScriptValue,
        ) -> NativeResult<Ordering>,
    ) -> NativeResult<()> {
        let count = self.len(cx)?;
        let guard = self.operation(cx)?;
        // SAFETY: Empty scratch reservation introduces no script edges.
        unsafe {
            self.object.edit(
                cx,
                Some(&guard),
                EditPlan {
                    max_logical_units: count,
                    structural: false,
                },
                |v, _| {
                    v.scratch
                        .try_reserve(count / 2)
                        .map_err(|_| RuntimeError::resource_limit("Vec sort scratch"))?;
                    #[cfg(feature = "execution-diagnostics")]
                    diagnostics::update(|counts| {
                        counts.scratch_peak_bytes =
                            counts.scratch_peak_bytes.max(v.scratch.retained_bytes())
                    });
                    Ok(())
                },
            )?;
        }
        self.sort_range(cx, &guard, 0, count, &mut compare)
    }

    fn sort_range(
        &self,
        cx: &mut NativeContext<'_>,
        guard: &NativeOperationGuard<'_>,
        start: usize,
        end: usize,
        compare: &mut impl FnMut(
            &mut NativeContext<'_>,
            ScriptValue,
            ScriptValue,
        ) -> NativeResult<Ordering>,
    ) -> NativeResult<()> {
        if end - start < 2 {
            return Ok(());
        }
        let max_units = self
            .units(cx, guard)?
            .checked_add((end - start) / 2)
            .ok_or_else(|| RuntimeError::resource_limit("Vec merge work"))?;
        let middle = start + (end - start) / 2;
        self.sort_range(cx, guard, start, middle, compare)?;
        self.sort_range(cx, guard, middle, end, compare)?;
        // Fixed splits make the left half the shorter run, including odd lengths.
        // Copying it is merge work, and every partial prefix is scanner-visible.
        for index in start..middle {
            // SAFETY: Main remains authoritative until all scratch is initialized.
            unsafe {
                self.object.edit(
                    cx,
                    Some(guard),
                    EditPlan {
                        max_logical_units: max_units,
                        structural: false,
                    },
                    |v, access| {
                        let value = v.values.get(index).expect("merge source index");
                        access.republish_existing(&value);
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::update(|counts| counts.scratch_copies += 1);
                        v.scratch.push(value)
                    },
                )?;
            }
        }
        // SAFETY: Register gap indices before overwriting any main source slot.
        unsafe {
            self.object.edit(
                cx,
                Some(guard),
                EditPlan {
                    max_logical_units: max_units,
                    structural: false,
                },
                |v, _| {
                    v.work = Some(Work::Merge {
                        scratch: 0,
                        other: middle,
                        output: start,
                        end,
                    });
                    Ok(())
                },
            )?;
        }
        loop {
            // SAFETY: Each comparator argument is retained while its authoritative
            // slot is still installed. Neither a slice nor payload loan escapes.
            let pair = unsafe {
                self.object.read(cx, Some(guard), |v, access| {
                    let Some(Work::Merge {
                        scratch,
                        other,
                        end,
                        ..
                    }) = v.work
                    else {
                        unreachable!("active merge");
                    };
                    if scratch == v.scratch.len() || other == end {
                        return Ok(None);
                    }
                    Ok(Some((
                        access
                            .retain(&v.contract, &v.scratch.get(scratch).expect("scratch item"))?,
                        access.retain(&v.contract, &v.values.get(other).expect("right item"))?,
                    )))
                })?
            };
            let Some((left, right)) = pair else {
                break;
            };
            #[cfg(feature = "execution-diagnostics")]
            let started = Instant::now();
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::update(|counts| counts.comparisons += 1);
            let compared = compare(cx, left, right);
            #[cfg(feature = "execution-diagnostics")]
            diagnostics::update(|counts| {
                counts.callback_max_ns = counts.callback_max_ns.max(started.elapsed().as_nanos());
            });
            let take_left = compared? != Ordering::Greater;
            // SAFETY: Publish each moved edge before overwriting a stale main copy;
            // update authoritative gap indices in the same noncollecting loan.
            unsafe {
                self.object.edit(
                    cx,
                    Some(guard),
                    EditPlan {
                        max_logical_units: max_units,
                        structural: false,
                    },
                    |v, access| {
                        let Some(Work::Merge {
                            scratch,
                            other,
                            output,
                            end,
                        }) = v.work
                        else {
                            unreachable!("active merge");
                        };
                        let value = if take_left {
                            v.scratch.get(scratch)
                        } else {
                            v.values.get(other)
                        }
                        .expect("merge source");
                        access.republish_existing(&value);
                        v.values.set(output, value)?;
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::update(|counts| counts.output_copies += 1);
                        v.work = Some(Work::Merge {
                            scratch: scratch + usize::from(take_left),
                            other: other + usize::from(!take_left),
                            output: output + 1,
                            end,
                        });
                        Ok(())
                    },
                )?;
            }
        }
        // Finish in bounded transfers so cancellation remains observable. On error
        // the guard performs the identical remaining transfer without admission.
        loop {
            let done = unsafe {
                self.object.edit(
                    cx,
                    Some(guard),
                    EditPlan {
                        max_logical_units: max_units,
                        structural: false,
                    },
                    |v, access| {
                        let Some(Work::Merge {
                            scratch,
                            other,
                            output,
                            end,
                        }) = v.work
                        else {
                            unreachable!("active merge");
                        };
                        if scratch == v.scratch.len() {
                            v.work = None;
                            v.scratch.clear();
                            return Ok(true);
                        }
                        let value = v.scratch.get(scratch).expect("remaining scratch");
                        access.republish_existing(&value);
                        v.values.set(output, value)?;
                        #[cfg(feature = "execution-diagnostics")]
                        diagnostics::update(|counts| counts.output_copies += 1);
                        v.work = Some(Work::Merge {
                            scratch: scratch + 1,
                            other,
                            output: output + 1,
                            end,
                        });
                        Ok(false)
                    },
                )?
            };
            if done {
                break;
            }
        }
        Ok(())
    }

    fn units(
        &self,
        cx: &NativeContext<'_>,
        guard: &NativeOperationGuard<'_>,
    ) -> NativeResult<usize> {
        // SAFETY: Only scalar charge leaves the loan.
        unsafe {
            self.object
                .read(cx, Some(guard), |v, _| Ok(v.charge().logical_units))
        }
    }

    /// Linear stable compaction. Completed removals survive callback failure.
    pub fn retain(
        &self,
        cx: &mut NativeContext<'_>,
        mut keep: impl FnMut(&mut NativeContext<'_>, T) -> NativeResult<bool>,
    ) -> NativeResult<()> {
        self.compact(cx, |cx, _, value| keep(cx, value), false)
    }

    /// Linear adjacent deduplication, preserving the first element of each run.
    pub fn dedup_by(
        &self,
        cx: &mut NativeContext<'_>,
        mut equal: impl FnMut(&mut NativeContext<'_>, T, T) -> NativeResult<bool>,
    ) -> NativeResult<()> {
        self.compact(
            cx,
            |cx, previous, value| match previous {
                Some(previous) => Ok(!equal(cx, previous, value)?),
                None => Ok(true),
            },
            true,
        )
    }

    fn compact(
        &self,
        cx: &mut NativeContext<'_>,
        mut keep: impl FnMut(&mut NativeContext<'_>, Option<T>, T) -> NativeResult<bool>,
        adjacent: bool,
    ) -> NativeResult<()> {
        let count = self.len(cx)?;
        let guard = self.operation(cx)?;
        // SAFETY: Work state changes no edges; the unvisited suffix remains live.
        unsafe {
            self.object.edit(
                cx,
                Some(&guard),
                EditPlan {
                    max_logical_units: count,
                    structural: false,
                },
                |v, _| {
                    v.work = Some(Work::Compact { read: 0, write: 0 });
                    Ok(())
                },
            )?;
        }
        for index in 0..count {
            // SAFETY: Retain current/previous elements before invoking user code.
            let (previous, value) = unsafe {
                self.object.read(cx, Some(&guard), |v, access| {
                    let Some(Work::Compact { write, .. }) = v.work else {
                        unreachable!("active compaction");
                    };
                    let previous =
                        if adjacent && write != 0 {
                            Some(access.retain(
                                &v.contract,
                                &v.values.get(write - 1).expect("kept prefix"),
                            )?)
                        } else {
                            None
                        };
                    Ok((
                        previous,
                        access
                            .retain(&v.contract, &v.values.get(index).expect("unvisited item"))?,
                    ))
                })?
            };
            let previous = previous.map(|value| value.decode(cx)).transpose()?;
            let value = value.decode(cx)?;
            let retain = keep(cx, previous, value)?;
            // SAFETY: The decision commits exactly this item. Failure later restores
            // only the unvisited suffix; removed entries never return.
            unsafe {
                self.object.edit(
                    cx,
                    Some(&guard),
                    EditPlan {
                        max_logical_units: count,
                        structural: true,
                    },
                    |v, access| {
                        let Some(Work::Compact { read, write }) = v.work else {
                            unreachable!("active compaction");
                        };
                        if !retain {
                            access.commit_structure();
                        }
                        if retain {
                            let value = v.values.get(read).expect("current item");
                            access.republish_existing(&value);
                            v.values.set(write, value)?;
                        }
                        v.work = Some(Work::Compact {
                            read: read + 1,
                            write: write + usize::from(retain),
                        });
                        Ok(())
                    },
                )?;
            }
        }
        // SAFETY: All decisions completed; recovery now only truncates stale copies.
        unsafe {
            self.object.edit(
                cx,
                Some(&guard),
                EditPlan {
                    max_logical_units: count,
                    structural: true,
                },
                |v, access| {
                    access.commit_structure();
                    recover(v, access);
                    Ok(())
                },
            )
        }
    }
}
