// aux-build:named.rs
// edition:2024

extern crate named;

mod facade {
    pub trait Named {
        fn name(&self) -> &'static str;
    }

    pub use named::Named;
}

mod derived_here {
    // Not flagged: the module's derive finds its macro through the import, so moving the import
    // into `describe`, which calls the trait's method, would break the derive.
    use crate::facade::Named;

    #[derive(Named)]
    struct Order;

    fn describe(order: &Order) -> &'static str {
        order.name()
    }
}

mod derived_in_functions {
    // Not flagged: a derive in one function and a call in another both need the import.
    use crate::facade::Named;

    fn local() -> &'static str {
        #[derive(Named)]
        struct Legacy;

        "legacy"
    }

    fn describe(named: &impl crate::facade::Named) -> &'static str {
        named.name()
    }
}

mod derived_once {
    // Flagged: the derive and the call are in one function, which can hold the import.
    use crate::facade::Named;

    fn describe() -> &'static str {
        #[derive(Named)]
        struct Local;

        Local.name()
    }
}

mod derived_by_path {
    // Flagged: the derive names its macro by a path, so only `describe` needs the import.
    use crate::facade::Named;

    #[derive(crate::facade::Named)]
    struct Order;

    fn describe(order: &Order) -> &'static str {
        order.name()
    }
}

mod derived_through_block_import {
    // Flagged: the derive finds its macro through the function's own import, so only `describe`
    // needs the module's.
    use crate::facade::Named;

    #[derive(crate::facade::Named)]
    struct Order;

    fn local() -> &'static str {
        use crate::facade::Named;

        #[derive(Named)]
        struct Local;

        "local"
    }

    fn describe() -> &'static str {
        Order.name()
    }
}

mod derived_through_block_glob {
    // Flagged: the function's glob import brings the macro, so only `describe` needs the module's.
    use crate::facade::Named;

    #[derive(crate::facade::Named)]
    struct Order;

    fn local() -> &'static str {
        use named::*;

        #[derive(Named)]
        struct Local;

        "local"
    }

    fn describe() -> &'static str {
        Order.name()
    }
}

mod derived_in_nested_block {
    // Not flagged: an inner block's import doesn't reach the derive in the outer one.
    use crate::facade::Named;

    #[derive(crate::facade::Named)]
    struct Order;

    fn local() -> &'static str {
        {
            use crate::facade::Named;

            #[derive(Named)]
            struct Inner;
        }

        #[derive(Named)]
        struct Local;

        "local"
    }

    fn describe() -> &'static str {
        Order.name()
    }
}

mod derived_from_prelude {
    use std::hash::DefaultHasher;

    // Flagged: the prelude brings the same `Hash` derive, so the derive doesn't need the import.
    use std::hash::Hash;

    #[derive(Hash)]
    struct Order(u8);

    fn digest(order: &Order) -> u64 {
        use std::hash::Hasher as _;

        let mut hasher = DefaultHasher::new();
        order.hash(&mut hasher);
        hasher.finish()
    }
}

fn main() {}
