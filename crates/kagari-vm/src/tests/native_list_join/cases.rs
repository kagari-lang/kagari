pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for route in [
        "array",
        "generic_array",
        "custom",
        "generic_custom",
        "dynamic_array",
        "dynamic_custom",
        "mutable_array",
    ] {
        for (index, (items, separator)) in [
            (vec![], "/"),
            (vec!["中文😀"], "/"),
            (vec!["中文😀", "", "c"], "/"),
            (vec!["a", "é", "😀"], ""),
            (vec!["", "é", ""], "😀"),
        ]
        .into_iter()
        .enumerate()
        {
            let expected = format!("{:?}", items.join(separator));
            let values = items
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>()
                .join(",");
            let call = match route {
                "array" => "storage(values).join(separator())",
                "generic_array" => "consume(storage(values),separator())",
                "custom" => "sequence(values).join(separator())",
                "generic_custom" => "consume(sequence(values),separator())",
                "dynamic_array" => "view_array(storage(values)).join(separator())",
                "dynamic_custom" => "view_sequence(sequence(values)).join(separator())",
                _ => "mutable(storage(values)).join(separator())",
            };
            cases.push((format!("{route}_{index}"), format!(r#"
struct Sequence<T> {{val items:ArrayList<T>}}
impl<T> Index<usize> for Sequence<T> {{type Output=T;fn index(self,index:usize)->T{{self.items[index]}}}}
impl<T> Iterable for Sequence<T> {{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{{print("iter");self.items.iter().map(|item|{{print("item");item}})}}}}
impl<T> List<T> for Sequence<T> {{fn len(self)->usize{{self.items.len()}}fn is_empty(self)->bool{{self.items.is_empty()}}fn get(self,index:usize)->Option<T>{{self.items.get(index)}}}}
fn storage(values:ArrayList<String>)->ArrayList<String>{{print("source");values}}
fn sequence(values:ArrayList<String>)->Sequence<String>{{print("source");Sequence{{items:values}}}}
fn view_array(value:ArrayList<String>)->List<String>{{value}}
fn view_sequence(value:Sequence<String>)->List<String>{{value}}
fn mutable(value:ArrayList<String>)->MutableList<String>{{value}}
fn separator()->String{{print("separator");{separator:?}}}
fn consume<L:List<String>>(value:L,separator:String)->String{{value.join(separator)}}
fn main()->i32{{val values:ArrayList<String> = [{values}];val result={call};std::debug::assert_eq(result,{expected},"joined");print("done");42}}
"#)));
        }
    }
    cases
}
