struct Shelf {
    visible: bool,
}

impl Shelf {
    fn order(&self, quantity: u64, express: bool) -> u64 {
        if express && self.visible {
            quantity * 2
        } else {
            quantity
        }
    }

    fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    fn set_both(&mut self, visible: bool, express: bool) {
        self.visible = visible && express;
    }

    fn set_shown(&mut self, shown: bool, cx: &mut Context) {
        self.visible = shown;
        cx.updates += 1;
    }

    fn set_speed(&mut self, speed: u64, express: bool) {
        self.visible = express && speed > 0;
    }

    fn new(visible: bool) -> Shelf {
        Shelf { visible }
    }
}

trait Animate {
    fn animate(&self, fast: bool) -> bool;
}

impl Animate for Shelf {
    fn animate(&self, fast: bool) -> bool {
        fast
    }
}

fn order(quantity: u64, express: bool) -> u64 {
    if express { quantity * 2 } else { quantity }
}

fn remember<T>(value: T) -> T {
    value
}

struct Context {
    updates: u32,
}

struct Flag(bool);

enum Choice {
    Pick(bool),
}

fn main() {
    // Flagged: an associated function, a free function, a method, a method called by its path,
    // a local trait's method, a setter taking two `bool`s, and a `bool` after a setter's value.
    let mut shelf = Shelf::new(true);
    let _ = order(100, false);
    let _ = shelf.order(100, true);
    let _ = Shelf::order(&shelf, 100, false);
    let _ = shelf.animate(true);
    shelf.set_both(true, false);
    shelf.set_speed(2, true);

    // Not flagged: a setter's name says what its `bool` value means, however called, also
    // when context arguments follow it.
    shelf.set_visible(false);
    Shelf::set_visible(&mut shelf, true);
    let mut cx = Context { updates: 0 };
    shelf.set_shown(false, &mut cx);
    Shelf::set_shown(&mut shelf, true, &mut cx);

    // Not flagged: a variable, a negated literal, and a comparison name the value.
    let express = true;
    let _ = order(100, express);
    let _ = order(100, !express);
    let _ = order(100, 1 > 2);

    // Not flagged: a generic parameter that a `bool` fills holds data, not a flag.
    let _ = remember(true);

    // Not flagged: constructors, a struct literal and a closure.
    let _ = (
        Flag(true),
        Choice::Pick(false),
        Some(true),
        Shelf { visible: true },
    );
    let pick = |flag: bool| flag;
    let _ = pick(true);

    // Not flagged: another crate's API.
    let _ = std::hint::black_box(true);
    let _ = std::fs::OpenOptions::new().read(true);
    let _ = Vec::<bool>::with_capacity(1).contains(&true);
    let _ = String::new().split_terminator(' ').is_sorted();

    // Not flagged: a macro writes the call.
    macro_rules! fast_order {
        () => {
            order(100, true)
        };
    }
    let _ = fast_order!();

    let _ = (Flag(false).0, Choice::Pick(true));
    let _ = lone_exception();
}

// Not flagged: the `expect` on the function covers the call in its body, and is fulfilled.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        bool_literal_argument,
        reason = "the express order is what this test fixture exercises"
    )
)]
fn lone_exception() -> u64 {
    order(100, true)
}

// Flagged: the same code with a raw identifier, whose text `is_from_proc_macro` misreads; only
// the code a derive writes is skipped for that.
fn raw_order() -> u64 {
    r#order(100, false)
}
