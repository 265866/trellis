use std::ops::{Add, AddAssign, Index, IndexMut, Neg, Not, Shl};

#[derive(Clone, Copy, PartialEq)]
struct Amount(u64);

struct History {
    orders: Vec<Amount>,
}

// Flagged: a binary operator used as syntax sugar for pushing onto a list.
impl Add<Amount> for History {
    type Output = History;

    fn add(mut self, order: Amount) -> History {
        self.orders.push(order);
        self
    }
}

// Flagged: the same sugar through a `std::ops` path, implemented for a reference.
impl<'history> std::ops::Shl<Amount> for &'history mut History {
    type Output = &'history mut History;

    fn shl(self, order: Amount) -> &'history mut History {
        self.orders.push(order);
        self
    }
}

// Flagged: an `*Assign` form.
impl AddAssign<Amount> for History {
    fn add_assign(&mut self, order: Amount) {
        self.orders.push(order);
    }
}

// Flagged: unary operators.
impl Neg for Amount {
    type Output = Amount;

    fn neg(self) -> Amount {
        Amount(0)
    }
}

impl Not for Amount {
    type Output = bool;

    fn not(self) -> bool {
        self.0 == 0
    }
}

// Flagged: indexing.
impl Index<usize> for History {
    type Output = Amount;

    fn index(&self, receipt: usize) -> &Amount {
        &self.orders[receipt]
    }
}

impl IndexMut<usize> for History {
    fn index_mut(&mut self, receipt: usize) -> &mut Amount {
        &mut self.orders[receipt]
    }
}

// Not flagged: the `expect` on the impl says the meaning is mathematical, and is fulfilled.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(operator_overload, reason = "adding two amounts sums their cents")
)]
impl Add for Amount {
    type Output = Amount;

    fn add(self, other: Amount) -> Amount {
        Amount(self.0 + other.0)
    }
}

// Not flagged: a comparison trait isn't an arithmetic or indexing operator.
impl PartialOrd for Amount {
    fn partial_cmp(&self, other: &Amount) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

// Not flagged: a named method instead of an operator.
impl History {
    fn record(&mut self, order: Amount) {
        self.orders.push(order);
    }
}

trait Shl2 {
    fn shl(self, other: Self) -> Self;
}

// Not flagged: a local trait that shares an operator method's name isn't the operator.
impl Shl2 for Amount {
    fn shl(self, other: Amount) -> Amount {
        Amount(self.0 << other.0)
    }
}

macro_rules! sub_impl {
    ($type:ident) => {
        impl std::ops::Sub for $type {
            type Output = $type;

            fn sub(self, other: $type) -> $type {
                $type(self.0 - other.0)
            }
        }
    };
}

// Not flagged: a macro expansion writes the impl.
sub_impl!(Amount);

fn main() {
    let mut history = History { orders: Vec::new() } + Amount(1);
    history += Amount(2);
    let _ = &mut history << Amount(3);
    history.record(-Amount(4));
    history[0] = history[1] + Amount(5) - Amount(1);
    let _ = (!Amount(0), Amount(1).shl(Amount(2)), Amount(1) < Amount(2));
}
