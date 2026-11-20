//! Helpers for test cases, beside one of the types they return.

pub struct Entries(pub Vec<u32>);

pub fn empty_cart() -> Vec<u32> {
    Vec::new()
}

pub fn expired_coupon() -> u32 {
    0
}
