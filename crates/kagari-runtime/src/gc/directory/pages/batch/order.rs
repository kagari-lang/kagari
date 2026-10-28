//! Nonmutating interval proof: sort bounded ordinals, never payloads or fragments.
#[cfg(test)]
mod tests;

use super::Fragment;
use crate::gc::directory::retirement::RETIRE_LIMIT;

// Compute at compile time: admission only reads two entries. Include full scratch
// initialization, every merge output, worst-case key comparisons and adjacency.
const WORK: [usize; RETIRE_LIMIT + 1] = {
    assert!(RETIRE_LIMIT == 32);
    let mut work = [0; RETIRE_LIMIT + 1];
    let mut count = 2;
    while count <= RETIRE_LIMIT {
        let mut total = 2 * RETIRE_LIMIT + count - 1;
        let mut width = 1;
        while width < count {
            total += count;
            let mut start = 0;
            while start + width < count {
                let end = if start + 2 * width < count {
                    start + 2 * width
                } else {
                    count
                };
                total += end - start - 1;
                start += 2 * width;
            }
            width *= 2;
        }
        work[count] = total;
        assert!(work[count] >= work[count - 1]);
        count += 1;
    }
    assert!(work[32] == 384);
    work
};

/// Incremental admission reserves the difference between consecutive prefixes.
pub(in crate::gc::directory) fn ordering_work(count: usize) -> usize {
    WORK[count]
}

pub(super) fn disjoint(fragments: &[Option<Fragment>]) -> bool {
    fragments.len() < 2 || disjoint_sorted(fragments)
}

/// The separate frame keeps scratch initialization out of zero/one-fragment work.
/// The caller has already validated all intervals against their physical owners.
#[inline(never)]
fn disjoint_sorted(fragments: &[Option<Fragment>]) -> bool {
    #[cfg(test)]
    tests::record(0, 2 * RETIRE_LIMIT);
    let mut indices = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31,
    ];
    let mut scratch = [0; RETIRE_LIMIT];
    let mut input = &mut indices;
    let mut output = &mut scratch;
    let mut width = 1;
    while width < fragments.len() {
        merge_pass(fragments, input, output, width);
        (input, output) = (output, input);
        width *= 2;
    }
    for adjacent in input[..fragments.len()].windows(2) {
        #[cfg(test)]
        tests::record(3, 1);
        let previous = fragment(fragments, adjacent[0]);
        let next = fragment(fragments, adjacent[1]);
        if previous.start.page == next.start.page
            && previous.start.slot as usize + previous.count > next.start.slot as usize
        {
            return false;
        }
    }
    true
}

fn fragment(fragments: &[Option<Fragment>], index: u8) -> Fragment {
    fragments[index as usize].expect("validated physical interval")
}

fn precedes(fragments: &[Option<Fragment>], left: u8, right: u8) -> bool {
    #[cfg(test)]
    tests::record(1, 1);
    let left = fragment(fragments, left).start;
    let right = fragment(fragments, right).start;
    (left.page, left.slot) <= (right.page, right.slot)
}

fn merge_pass(
    fragments: &[Option<Fragment>],
    input: &[u8; RETIRE_LIMIT],
    output: &mut [u8; RETIRE_LIMIT],
    width: usize,
) {
    for start in (0..fragments.len()).step_by(2 * width) {
        let middle = (start + width).min(fragments.len());
        let end = (middle + width).min(fragments.len());
        let mut left = start;
        let mut right = middle;
        for destination in &mut output[start..end] {
            let index = if left < middle
                && (right == end || precedes(fragments, input[left], input[right]))
            {
                let index = input[left];
                left += 1;
                index
            } else {
                let index = input[right];
                right += 1;
                index
            };
            #[cfg(test)]
            tests::record(2, 1);
            *destination = index;
        }
    }
}
