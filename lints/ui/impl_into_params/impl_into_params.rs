// edition:2024

struct Window {
    title: String,
}

// Flagged: `impl Into<T>` in argument position.
fn titled(title: impl Into<String>) -> Window {
    Window {
        title: title.into(),
    }
}

// Flagged: a generic parameter bounded by `Into<T>` in the parameter list.
fn retitle<T: Into<String>>(window: &mut Window, title: T) {
    window.title = title.into();
}

// Flagged: a generic parameter bounded by `Into<T>` in a where clause.
fn width<W>(width: W) -> u64
where
    W: Into<u64>,
{
    width.into()
}

impl Window {
    // Flagged: an inherent method.
    fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    // Not flagged: the parameter is the target type itself.
    fn rename(&mut self, title: String) {
        self.title = title;
    }

    // Not flagged: other bounds aren't `Into`.
    fn append(&mut self, suffix: impl AsRef<str>) {
        self.title.push_str(suffix.as_ref());
    }

    // Flagged: the bound is on a type nested in the parameter's type.
    fn extend(&mut self, parts: Vec<impl Into<String>>) {
        for part in parts {
            self.title.push_str(&part.into());
        }
    }
}

// Flagged: a bounded generic wrapped in another type.
fn count<T: Into<u64>>(values: Vec<T>) -> u64 {
    values.into_iter().map(Into::into).sum()
}

// Flagged: a bounded generic behind a reference.
fn first<T>(values: &[T]) -> Option<u64>
where
    T: Into<u64> + Copy,
{
    values.first().map(|&value| value.into())
}

// Flagged: a bounded generic reached through another parameter's bound.
fn titles(items: impl IntoIterator<Item = impl Into<String>>) -> Vec<String> {
    items.into_iter().map(Into::into).collect()
}

// Not flagged: a generic without an `Into` bound.
fn total<T: Copy>(values: &[T]) -> usize {
    values.len()
}

// Not flagged: an `expect` on the item covers it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(impl_into_params, reason = "the fixture checks an item-level expect")
)]
fn excused(title: impl Into<String>) -> String {
    title.into()
}

trait Titled {
    // Flagged: a trait declares its own signature.
    fn with_title(title: impl Into<String>) -> Self;
}

struct Pane;

// Not flagged: a trait impl takes its signature from the trait.
impl Titled for Pane {
    fn with_title(_title: impl Into<String>) -> Self {
        Pane
    }
}

fn main() {
    let mut window = titled("order");
    retitle(&mut window, "order");
    window.set_title("order");
    let _ = (width(1_u32), Pane::with_title("order"));
}
