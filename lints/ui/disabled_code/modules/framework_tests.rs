//! Tests with a test framework's attributes. The fixture builds without `--test`, so only the
//! source scan reads this module.

// Flagged: arguments that skip the test, rerun it until it passes, or run code before its
// failure is reported.
#[framework::test(iterations = 0)]
fn never_runs() {}

#[framework::test(iterations = 5)]
fn runs_five_times() {}

#[framework::test(retries = 3)]
fn flaky() {}

#[framework::test(on_failure = "crate::exit_quietly")]
fn reported_after_a_hook() {}

// Flagged: any argument, even one that only chooses how the test runs.
#[framework::test(seed = 10, retries = 1)]
fn seeded_and_retried() {}

#[framework::test(seed = 10)]
fn with_seed() {}

#[framework::test(seeds(10, 20, 30))]
fn with_seeds() {}

#[::framework::test(seeds())]
fn with_no_seeds() {}

// Flagged: through `cfg_attr`, and from another test macro.
#[cfg_attr(windows, framework::test(iterations = 0))]
fn skipped_on_windows() {}

#[framework::property_test(cases = 0)]
fn no_cases(value: u32) {}

#[tokio::test(flavor = "multi_thread")]
async fn on_many_threads() {}

// Flagged: arguments only the macro's caller knows.
macro_rules! framework_test {
    ($($arguments:tt)*) => {
        #[framework::test($($arguments)*)]
        fn generated() {}
    };
}

// Not flagged: no arguments.
#[framework::test]
fn once() {}

#[test]
fn plain() {}

#[framework::property_test]
fn property(value: u32) {}
