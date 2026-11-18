pub struct Price(pub u64);

pub enum PriceError {
    Overflow,
}
