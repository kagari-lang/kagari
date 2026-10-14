mod prototype;
use prototype::{Player, Runtime};

fn main() {
    let mut runtime = Runtime;
    let mut player = Player { hp: 60 };
    let _escaped = runtime.with_host_scope(|scope| scope.borrow_mut(&mut player));
}
