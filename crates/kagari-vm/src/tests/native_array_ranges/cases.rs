pub(super) const TYPES: &str = r#"
struct Cell {var value:i32}
struct Region<T> {val items:ArrayList<T>,val lower:Bound<usize>,val upper:Bound<usize>}
impl<T> RangeBounds<usize> for Region<T> {
 fn start_bound(self)->Bound<usize>{print("start");self.lower}
 fn end_bound(self)->Bound<usize>{print("end");self.upper}
}
fn receiver<T>(a:ArrayList<T>)->ArrayList<T>{print("receiver");a}
fn interval<R:RangeBounds<usize>>(r:R)->R{print("range");r}
fn destination(d:usize)->usize{print("destination");d}
fn copy<T,R:RangeBounds<usize>>(a:ArrayList<T>,r:R,d:usize){a.copy_within(r,d);}
fn remove<T,R:RangeBounds<usize>>(a:ArrayList<T>,r:R)->List<T>{a.remove_range(r)}
"#;
pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    let ranges = [
        ("0usize..0usize", 0, 0, 4),
        ("0usize..2usize", 0, 2, 1),
        ("1usize..=2usize", 1, 3, 0),
        ("..2usize", 0, 2, 2),
        ("..=1usize", 0, 2, 1),
        ("1usize..", 1, 4, 0),
        ("..", 0, 4, 0),
        ("4usize..4usize", 4, 4, 4),
        ("1usize..1usize", 1, 1, 4),
    ];
    for method in ["copy", "remove"] {
        for heap in [false, true] {
            for (number, (range, start, end, dest)) in ranges.iter().enumerate() {
                for route in ["native", "generic", "custom", "generic_custom"] {
                    let item = if heap { "Cell" } else { "i32" };
                    let literal = |n| {
                        if heap {
                            format!("Cell{{value:{n}}}")
                        } else {
                            format!("{n}")
                        }
                    };
                    let mut expected: Vec<_> = (20..24).collect();
                    let removed = expected[*start..*end].to_vec();
                    if method == "copy" {
                        for (i, n) in removed.iter().enumerate() {
                            expected[dest + i] = *n;
                        }
                    } else {
                        expected.drain(*start..*end);
                    }
                    let range = if matches!(route, "custom" | "generic_custom") {
                        let lower = if number == 2 {
                            format!("Bound::Excluded({}usize)", start - 1)
                        } else if *start == 0 {
                            "Bound::Unbounded".into()
                        } else {
                            format!("Bound::Included({start}usize)")
                        };
                        let upper = if number == 2 {
                            format!("Bound::Included({}usize)", end - 1)
                        } else if *end == 4 {
                            "Bound::Unbounded".into()
                        } else {
                            format!("Bound::Excluded({end}usize)")
                        };
                        format!("Region{{items:items,lower:{lower},upper:{upper}}}")
                    } else {
                        range.to_string()
                    };
                    let setup = format!("val r={range};");
                    let action = match (method, route) {
                        ("copy", "generic" | "generic_custom") => {
                            format!("copy(receiver(items),interval(r),destination({dest}usize));")
                        }
                        ("copy", _) => format!(
                            "receiver(items).copy_within(interval(r),destination({dest}usize));"
                        ),
                        ("remove", "generic" | "generic_custom") => format!(
                            "val removed:List<{item}> =remove(receiver(items),interval(r));"
                        ),
                        _ => format!(
                            "val removed:List<{item}> =receiver(items).remove_range(interval(r));"
                        ),
                    };
                    let field = if heap { ".value" } else { "" };
                    let assertions = |name: &str, values: &[i32]| {
                        let mut out = format!(
                            "std::debug::assert({name}.len()=={}usize,\"length\");",
                            values.len()
                        );
                        for (i, value) in values.iter().enumerate() {
                            out.push_str(&format!(
                                "std::debug::assert({name}[{i}usize]{field}=={value},\"slots\");"
                            ));
                        }
                        out
                    };
                    let mut checks = assertions("items", &expected);
                    if method == "remove" {
                        checks.push_str(&assertions("removed", &removed));
                    }
                    let values = (20..24).map(literal).collect::<Vec<_>>().join(",");
                    cases.push((format!("{method}_{route}_{heap}_{number}"),format!("{TYPES}\nfn main()->i32{{val items=[{values}];{setup}{action}print(\"committed\");{checks}print(\"done\");42}}")));
                }
            }
        }
    }
    cases
}
