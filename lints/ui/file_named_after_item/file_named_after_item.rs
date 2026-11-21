// compile-flags: --test

// Flagged: the module's one type is `Config`.
#[path = "modules/settings.rs"]
mod settings;

// Flagged: `Price` starts every other type's name, so it is the primary item.
#[path = "modules/earnings.rs"]
mod earnings;

// Flagged: the one visible type is primary; the private helper doesn't count.
#[path = "modules/shelf.rs"]
mod shelf;

// Flagged: a `#[cfg(test)]` child module doesn't make the module a non-leaf.
#[path = "modules/metrics.rs"]
mod metrics;

// Not flagged: the module is named after its type.
#[path = "modules/app_view.rs"]
mod app_view;

// Not flagged: names match across case conventions and acronyms.
#[path = "modules/http_server.rs"]
mod http_server;

// Not flagged: a function shares the module's name.
#[path = "modules/order.rs"]
mod order;

// Not flagged: neither type starts the other's name, so there is no primary item.
#[path = "modules/geometry.rs"]
mod geometry;

// Not flagged: the module defines no types.
#[path = "modules/helpers.rs"]
mod helpers;

// Not flagged: a module of public functions isn't about its private helper type, so the guide
// lets it be named as desired.
#[path = "modules/render.rs"]
mod render;

// Not flagged: a module with a child module isn't a leaf.
#[path = "modules/widgets.rs"]
mod widgets;

// Not flagged: test modules are named for what they test.
#[cfg(test)]
#[path = "modules/fixture_tests.rs"]
mod fixture_tests;

// Not flagged: an inline module has no file to name.
mod settings_inline {
    pub struct Config;
}

fn main() {}

// Not reported here: flagged, but an item-level `expect` in the documented form takes it,
// on the declaration of a leaf module, where `suppression_policy` allows it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        file_named_after_item,
        reason = "the fixture checks an item-level expect"
    )
)]
#[path = "modules/excused.rs"]
mod excused;
