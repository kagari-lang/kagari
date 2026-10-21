use super::*;
use kagari_abi::representation::ValueType;

fn range(slot: usize, start: usize, end: usize) -> LocalLiveRange {
    LocalLiveRange {
        slot: Slot::new(slot),
        start,
        end,
        binding: slot as u32,
        name: String::new(),
        span: Default::default(),
        ty: ValueType::I32,
        is_parameter: false,
    }
}

#[test]
fn arbitrary_queries_match_half_open_scan_with_repeated_and_empty_ranges() {
    let ranges = [
        range(0, 3, 8),
        range(1, 0, 2),
        range(0, 1, 5),
        range(2, 2, 2),
        range(3, 8, 10),
        range(0, 3, 4),
    ];
    let index = DebugAvailability::new(&ranges, |_| Ok::<_, ()>(())).unwrap();
    for pc in [0, 1, 2, 3, 4, 5, 7, 8, 9, 10, 6, 3, 0] {
        let mut found = Vec::new();
        index
            .visit(pc, |_| Ok::<_, ()>(()), |slot| found.push(slot.index()))
            .unwrap();
        let mut expected = ranges
            .iter()
            .filter(|r| r.start <= pc && pc < r.end)
            .map(|r| r.slot.index())
            .collect::<Vec<_>>();
        found.sort_unstable();
        expected.sort_unstable();
        assert_eq!(found, expected, "pc {pc}");
    }
}

#[test]
fn queries_preserve_start_and_original_ordinal_order_in_both_directions() {
    let ranges = [
        range(7, 3, 8),
        range(9, 1, 6),
        range(2, 3, 5),
        range(8, 0, 9),
        range(4, 3, 7),
    ];
    let index = DebugAvailability::new(&ranges, |_| Ok::<_, ()>(())).unwrap();
    let cases = [
        (0, vec![8]),
        (1, vec![8, 9]),
        (3, vec![8, 9, 7, 2, 4]),
        (5, vec![8, 9, 7, 4]),
        (6, vec![8, 7, 4]),
        (7, vec![8, 7]),
        (8, vec![8]),
        (9, vec![]),
    ];
    for (pc, expected) in cases.iter().chain(cases.iter().rev()) {
        let mut found = Vec::new();
        index
            .visit(*pc, |_| Ok::<_, ()>(()), |slot| found.push(slot.index()))
            .unwrap();
        assert_eq!(&found, expected, "pc {pc}");
    }
}

#[test]
fn sparse_queries_have_bounded_construction_and_path_work() {
    for count in [16usize, 256, 4096] {
        let ranges = (0..count)
            .rev()
            .map(|i| range(0, i * 2, i * 2 + 1))
            .collect::<Vec<_>>();
        let mut build = 0;
        let index = DebugAvailability::new(&ranges, |n| {
            build += n;
            Ok::<_, ()>(())
        })
        .unwrap();
        let mut work = 0;
        let mut emitted = 0;
        for pc in (0..count * 2).rev() {
            index
                .visit(
                    pc,
                    |n| {
                        work += n;
                        Ok::<_, ()>(())
                    },
                    |_| emitted += 1,
                )
                .unwrap();
        }
        assert_eq!(emitted, count);
        let log = count.ilog2() as usize + 1;
        assert!(
            build <= count * (8 * log + 4),
            "construction {count}: {build}"
        );
        assert!(work <= count * 2 * (2 * log + 2), "queries {count}: {work}");
        println!(
            "availability ranges={count} scan={} build={build} queries={work} scratch_bytes={}",
            count * count * 2,
            DebugAvailability::scratch_bytes(count)
        );
    }
}

#[test]
fn errors_interrupt_construction_sorting_and_queries() {
    let ranges = (0..64)
        .rev()
        .map(|i| range(i, i, i + 10))
        .collect::<Vec<_>>();
    for limit in [0, 80, 200] {
        let mut work = 0;
        assert!(
            DebugAvailability::new(&ranges, |n| {
                work += n;
                if work > limit {
                    Err("cancelled or exhausted")
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
    }
    let index = DebugAvailability::new(&ranges, |_| Ok::<_, ()>(())).unwrap();
    let mut work = 0;
    assert!(
        index
            .visit(
                32,
                |n| {
                    work += n;
                    if work > 3 { Err("cancelled") } else { Ok(()) }
                },
                |_| {}
            )
            .is_err()
    );
    assert!(work <= 5);
}
