pub(super) const TYPES: &str = r#"
struct Cursor<T>{val items:ArrayList<T>,var index:usize}
impl<T> Iterator for Cursor<T>{type Item=T;fn next(self)->Option<T>{print("next");val result=self.items.get(self.index);self.index+=1usize;result}}
struct Sequence<T>{val items:ArrayList<T>}
impl<T> Iterable for Sequence<T>{type Item=T;type Iter=Cursor<T>;fn iter(self)->Cursor<T>{print("iter");Cursor{items:self.items,index:0}}}
struct Proxy<T>{val items:ArrayList<T>}
impl<T> Iterable for Proxy<T>{type Item=T;type Iter=Iter<T>;fn iter(self)->Iter<T>{print("list iter");self.items.iter()}}
impl<T> Index<usize> for Proxy<T>{type Output=T;fn index(self,index:usize)->T{self.items[index]}}
impl<T> List<T> for Proxy<T>{fn len(self)->usize{print("len");self.items.len()}fn is_empty(self)->bool{self.items.is_empty()}fn get(self,index:usize)->Option<T>{print("get");self.items.get(index)}}
fn input<T>(value:T)->T{print("input");value}
fn callback<T>(value:T)->T{print("callback");value}
fn other<T>(value:T)->T{print("other");value}
"#;

fn source(kind: &str, items: &str) -> String {
    match kind {
        "native" => format!("[{items}].iter()"),
        "script" => format!("Cursor{{items:[{items}],index:0}}"),
        _ => format!("[{items}].iter().map(|item|{{print(\"upstream\");item}})"),
    }
}

pub(super) fn cases() -> Vec<(String, String)> {
    let mut cases = Vec::new();
    for kind in ["native", "script", "lazy"] {
        for empty in [false, true] {
            let items = if empty { "" } else { "1,2,3" };
            for (operation, arguments, item, expected) in [
                (
                    "map",
                    "callback(|item:i32|{print(\"visit\");item*2})",
                    "i32",
                    "2,4,6",
                ),
                (
                    "filter",
                    "callback(|item:i32|{print(\"visit\");item%2==1})",
                    "i32",
                    "1,3",
                ),
                (
                    "filter_map",
                    "callback(|item:i32|{print(\"visit\");if item==2{None}else{Some(item*2)}})",
                    "i32",
                    "2,6",
                ),
                ("take", "other(2usize)", "i32", "1,2"),
                ("skip", "other(2usize)", "i32", "3"),
                (
                    "enumerate",
                    "",
                    "(usize,i32)",
                    "(0usize,1),(1usize,2),(2usize,3)",
                ),
                (
                    "zip",
                    "other(Sequence{items:[20,22]})",
                    "(i32,i32)",
                    "(1,20),(2,22)",
                ),
                (
                    "chain",
                    "other(Sequence{items:[20,22]})",
                    "i32",
                    "1,2,3,20,22",
                ),
                (
                    "take_while",
                    "callback(|item:i32|{print(\"visit\");item<2})",
                    "i32",
                    "1",
                ),
                (
                    "skip_while",
                    "callback(|item:i32|{print(\"visit\");item<2})",
                    "i32",
                    "2,3",
                ),
                (
                    "inspect",
                    "callback(|item:i32|{print(\"visit\");})",
                    "i32",
                    "1,2,3",
                ),
                ("fuse", "", "i32", "1,2,3"),
                (
                    "flat_map",
                    "callback(|item:i32|{print(\"visit\");[item,item+10]})",
                    "i32",
                    "1,11,2,12,3,13",
                ),
                (
                    "flat_map_custom",
                    "callback(|item:i32|{print(\"visit\");Sequence{items:if item==2{[]}else{[item]}}})",
                    "i32",
                    "1,3",
                ),
            ] {
                let expected = if empty {
                    if operation == "chain" { "20,22" } else { "" }
                } else {
                    expected
                };
                let source = source(kind, items);
                let method = operation.strip_suffix("_custom").unwrap_or(operation);
                cases.push((format!("{operation}_{kind}_{empty}"),format!(r#"{TYPES}
fn main()->i32{{val iterator=input({source}).{method}({arguments});print("constructed");val result:ArrayList<{item}> =iterator.collect();val expected:ArrayList<{item}> =[{expected}];std::debug::assert(result.len()==expected.len() && result.iter().zip(expected).all(|pair|pair[0]==pair[1]),"items");print("done");42}}
"#)));
            }
            for custom in [false, true] {
                let (ty, values) = if custom {
                    (
                        "Sequence<i32>",
                        "Sequence{items:[1,2]},Sequence{items:[]},Sequence{items:[3]}",
                    )
                } else {
                    ("ArrayList<i32>", "[1,2],[],[3]")
                };
                let source = source(kind, if empty { "" } else { values });
                let iterator = if kind == "script" {
                    format!("Cursor<{ty}>")
                } else {
                    format!("Iter<{ty}>")
                };
                let expected = if empty { "" } else { "1,2,3" };
                cases.push((format!("flatten_{kind}_{empty}_{custom}"),format!(r#"{TYPES}
fn main()->i32{{val source:{iterator} ={source};val iterator=input(source).flatten();print("constructed");val result:ArrayList<i32> =iterator.collect();val expected:ArrayList<i32> =[{expected}];std::debug::assert(result.len()==expected.len() && result.iter().zip(expected).all(|pair|pair[0]==pair[1]),"flatten");print("done");42}}
"#)));
            }
        }
    }
    for operation in ["windows", "chunks"] {
        for kind in ["array", "script", "dynamic", "view"] {
            for (items, width, length, total) in [
                ("", 2, 0, 0),
                ("1,2,3", 2, 2, if operation == "windows" { 8 } else { 6 }),
                (
                    "1,2,3",
                    5,
                    if operation == "windows" { 0 } else { 1 },
                    if operation == "windows" { 0 } else { 6 },
                ),
            ] {
                let (parameter, source) = match kind {
                    "array" => ("source:ArrayList<i32>", "items"),
                    "script" => ("source:Proxy<i32>", "Proxy{items}"),
                    "dynamic" => ("source:List<i32>", "Proxy{items}"),
                    _ => ("source:List<i32>", "items"),
                };
                cases.push((format!("{operation}_{kind}_{length}_{width}"),format!(r#"{TYPES}
fn consume({parameter})->i32{{val iterator=input(source).{operation}(other({width}usize));print("constructed");val pieces:ArrayList<List<i32>> =iterator.collect();std::debug::assert(pieces.len()=={length}usize && pieces.iter().flat_map(|piece|piece).sum::<i32>()=={total},"pieces");print("done");42}}
fn main()->i32{{val items:ArrayList<i32> =[{items}];consume({source})}}
"#)));
            }
        }
    }
    for kind in ["native", "script", "lazy"] {
        for (scenario, (operation, args, expected, index)) in [
            ("take", "0usize", "", 0),
            ("take", "9usize", "1,2,3", 3),
            ("skip", "0usize", "1,2,3", 3),
            ("skip", "9usize", "", 3),
            ("take_while", "|item:i32|{print(\"visit\");false}", "", 1),
            ("skip_while", "|item:i32|{print(\"visit\");true}", "", 3),
        ]
        .into_iter()
        .enumerate()
        {
            let input = source(kind, "1,2,3");
            let source_check = if kind == "script" {
                format!(
                    "std::debug::assert(source.index=={}usize,\"progress\");",
                    if index == 3 { 4 } else { index }
                )
            } else {
                String::new()
            };
            cases.push((format!("{operation}_limits_{kind}_{scenario}"),format!(r#"{TYPES}
fn main()->i32{{val source={input};val iterator=input(source).{operation}({args});print("constructed");val result:ArrayList<i32> =iterator.collect();val expected:ArrayList<i32> =[{expected}];std::debug::assert(result.len()==expected.len() && result.iter().zip(expected).all(|pair|pair[0]==pair[1]),"limits");{source_check}print("done");42}}
"#)));
        }
    }
    cases
}
