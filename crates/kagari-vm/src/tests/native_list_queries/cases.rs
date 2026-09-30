pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for source_kind in ["array", "custom", "dynamic", "view"] {
        for custom_ord in [false, true] {
            for length in [0, 1, 5] {
                for method in ["first", "last", "binary_search"] {
                    if custom_ord && method != "binary_search" {
                        continue;
                    }
                    let ty = if custom_ord { "Rank<i32>" } else { "i32" };
                    let items = (0..length)
                        .map(|n| {
                            if custom_ord {
                                format!("Rank{{value:{}}}", n * 2)
                            } else {
                                (n * 2).to_string()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    let probes = if method == "binary_search" {
                        vec![-1, 0, 3, 8, 9]
                    } else {
                        vec![0]
                    };
                    for probe in probes {
                        let call = if method == "binary_search" {
                            format!(
                                "source.binary_search({})",
                                if custom_ord {
                                    format!("Rank{{value:{probe}}}")
                                } else {
                                    probe.to_string()
                                }
                            )
                        } else {
                            format!("source.{method}()")
                        };
                        let expected = if method == "binary_search" {
                            let index = (0..length).position(|n| n * 2 == probe);
                            if let Some(index) = index {
                                format!("Ok({index}usize)")
                            } else {
                                format!(
                                    "Err({}usize)",
                                    (0..length).filter(|n| n * 2 < probe).count()
                                )
                            }
                        } else if length == 0 {
                            "None".into()
                        } else {
                            format!(
                                "Some({})",
                                if method == "first" {
                                    0
                                } else {
                                    (length - 1) * 2
                                }
                            )
                        };
                        let argument = match source_kind {
                            "array" | "view" => "values",
                            _ => "Sequence{items:values}",
                        };
                        let constraint = if source_kind == "array" || source_kind == "custom" {
                            format!("fn consume<L:List<{ty}>>(source:L)")
                        } else {
                            format!("fn consume(source:List<{ty}>)")
                        };
                        let source = format!(
                            r#"
struct Sequence<T> {{val items:ArrayList<T>}}
impl<T> Iterable for Sequence<T> {{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{print("iter");self.items.iter()}}}}
impl<T> Index<usize> for Sequence<T> {{type Output=T;fn index(self,index:usize)->T {{self.items[index]}}}}
impl<T> List<T> for Sequence<T> {{fn len(self)->usize{{print("len");self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<T>{{print("get");self.items.get(index)}}}}
struct Rank<T> {{val value:T}}
impl<T:PartialEq> PartialEq for Rank<T> {{fn eq(self,other:Self)->bool{{self.value==other.value}}}}
impl<T:Eq> Eq for Rank<T> {{}}
impl<T:PartialOrd> PartialOrd for Rank<T> {{fn partial_cmp(self,other:Self)->Option<Ordering>{{self.value.partial_cmp(other.value)}}}}
impl<T:Ord> Ord for Rank<T> {{fn cmp(self,other:Self)->Ordering{{print("cmp");self.value.cmp(other.value)}}}}
{constraint} {{val result={call};std::debug::assert_eq(result,{expected},"selected");print("done");}}
fn main()->i32 {{val values:ArrayList<{ty}> = [{items}];consume({argument});42}}
"#
                        );
                        cases.push((
                            format!("{source_kind}_{custom_ord}_{length}_{method}_{probe}"),
                            source,
                        ));
                    }
                }
            }
        }
    }
    cases
}
