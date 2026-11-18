// edition:2024

// Flagged: an options struct without `Default`.
struct OrderOptions {
    express: bool,
    receipts: u32,
}

// Flagged: a generic options struct without `Default`.
struct RenderOptions<T> {
    theme: T,
}

// Not flagged: it derives `Default`.
#[derive(Default)]
struct DrawOptions {
    scale: f32,
}

// Not flagged: it implements `Default` by hand, with a bound on its parameter.
struct ExportOptions<T> {
    format: T,
}

impl<T: Default> Default for ExportOptions<T> {
    fn default() -> Self {
        Self {
            format: T::default(),
        }
    }
}

// Not flagged: a field whose type has no `Default` is required, so there's nothing to default.
struct ConnectOptions {
    url: Url,
    retries: u32,
}

struct Url;

// Not flagged: the name doesn't end in `Options`.
struct OptionsPanel {
    open: bool,
}

// Not flagged: a tuple struct can't use struct update syntax.
struct ColorOptions(u8, u8);

// Not flagged: a unit struct has no options to default.
struct NoOptions;

fn main() {}

// Not reported: flagged, but an item-level `expect` in the documented form takes it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        options_struct_default,
        reason = "the fixture checks an item-level expect"
    )
)]
struct ExcusedOptions {
    receipts: u32,
}
