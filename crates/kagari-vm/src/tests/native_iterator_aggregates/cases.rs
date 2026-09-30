pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in ["native", "custom", "lazy"] {
        for custom_target in [false, true] {
            for length in [0, 1, 3] {
                for method in ["sum", "product"] {
                    let items = match length {
                        0 => "",
                        1 => "2",
                        _ => "2,3,7",
                    };
                    let source = match kind {
                        "native" => "values.iter()",
                        "custom" => "Counter{items:values,index:0}",
                        _ => "values.iter().map(|n|{print(\"item\");n})",
                    };
                    let target = if custom_target { "Total<i32>" } else { "i32" };
                    let expected = if method == "sum" {
                        match length {
                            0 => 0,
                            1 => 2,
                            _ => 12,
                        }
                    } else {
                        match length {
                            0 => 1,
                            1 => 2,
                            _ => 42,
                        }
                    };
                    let read = if custom_target {
                        "result.value"
                    } else {
                        "result"
                    };
                    cases.push((format!("{method}_{kind}_{custom_target}_{length}"),format!(r#"
struct Counter<T> {{val items:ArrayList<T>,var index:usize}}
impl<T> Iterator for Counter<T> {{type Item=T;fn next(self)->Option<T>{{print("next");if self.index>=self.items.len(){{None}}else{{val item=self.items[self.index];self.index+=1;Some(item)}}}}}}
struct Total<T> {{val value:T}}
impl Sum<i32> for Total<i32> {{fn sum<I:Iterable<Item=i32>>(source:I)->Self {{print("sum");val value=source.iter().fold(0,|a,n|{{print("combine");a+n}});Total{{value}}}}}}
impl Product<i32> for Total<i32> {{fn product<I:Iterable<Item=i32>>(source:I)->Self {{print("product");val value=source.iter().fold(1,|a,n|{{print("combine");a*n}});Total{{value}}}}}}
fn consume<I:Iterator<Item=i32>>(source:I)->{target} {{source.{method}()}}
fn main()->i32 {{val values:ArrayList<i32> = [{items}];val result=consume({source});std::debug::assert_eq({read},{expected},"aggregate");print("done");42}}
"#)));
                }
            }
        }
    }
    cases.extend(scalar_cases());
    cases
}
fn scalar_cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for scalar in [
        "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize", "f32", "f64",
    ] {
        for length in [0, 1, 3] {
            for method in ["sum", "product"] {
                let fields = match length {
                    0 => vec![],
                    1 => vec![2],
                    _ => vec![2, 3, 7],
                };
                let items = fields
                    .iter()
                    .map(|n| {
                        if scalar.starts_with('f') {
                            format!("{n}.0{scalar}")
                        } else {
                            format!("{n}{scalar}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                let expected = if method == "sum" {
                    match length {
                        0 => 0,
                        1 => 2,
                        _ => 12,
                    }
                } else {
                    match length {
                        0 => 1,
                        1 => 2,
                        _ => 42,
                    }
                };
                let expected = if scalar.starts_with('f') {
                    format!("{expected}.0{scalar}")
                } else {
                    expected.to_string()
                };
                let call = format!("source.{method}()");
                cases.push((format!("{method}_{scalar}_{length}"),format!(r#"
fn consume<I:Iterator<Item={scalar}>>(source:I)->{scalar} {{{call}}}
fn main()->i32 {{val values:ArrayList<{scalar}> = [{items}];val result=consume(values.iter());std::debug::assert_eq(result,{expected},"scalar aggregate");print("done");42}}
"#)));
            }
        }
    }
    cases
}
