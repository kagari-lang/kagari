mod prototype;
use prototype::{LoanContext, Player, bind};

fn main() {
    bind(|_: &mut LoanContext, (p, _): (&Player, i32)| Ok(p));
}
