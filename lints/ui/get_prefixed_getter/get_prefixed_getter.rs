struct Order {
    price: u64,
}

impl Order {
    // Flagged: a getter takes only `self`.
    fn get_price(&self) -> u64 {
        self.price
    }

    // Not flagged: the name is already right.
    fn price(&self) -> u64 {
        self.price
    }

    // Not flagged: a lookup takes more than `self`.
    fn get_or(&self, fallback: u64) -> u64 {
        if self.price == 0 { fallback } else { self.price }
    }

    // Not flagged: `mut` and `ref` are keywords, so these names can't drop the `get_`.
    fn get_mut(&mut self) -> &mut u64 {
        &mut self.price
    }

    fn get_ref(&self) -> &u64 {
        &self.price
    }

    // Not flagged: a `get_or_*` name says what happens when the value is missing.
    fn get_or_insert_default(&mut self) -> &mut u64 {
        &mut self.price
    }

    // Not flagged: an exception on the method, with a reason.
    #[cfg_attr(
        dylint_lib = "trellis_lints",
        expect(get_prefixed_getter, reason = "mirrors the name in the vendor's API")
    )]
    fn get_balance(&self) -> u64 {
        self.price
    }

    // Not flagged: an associated function has no `self`.
    fn get_default() -> Self {
        Self { price: 0 }
    }
}

// Flagged: a trait declares the names its impls use.
trait Price {
    fn get_price(&self) -> u64;
}

// Not flagged: a trait impl takes its names from the trait.
impl Price for u64 {
    fn get_price(&self) -> u64 {
        *self
    }
}

fn main() {
    let mut order = Order::get_default();
    let _ = (order.get_price(), order.price(), order.get_or(1), 7_u64.get_price());
    let _ = (*order.get_ref(), order.get_balance());
    *order.get_mut() += 1;
    *order.get_or_insert_default() += 1;
}
