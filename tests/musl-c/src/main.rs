unsafe extern "C" {
    fn musl_c_answer() -> i32;
}

fn main() {
    // SAFETY: `musl_c_answer` takes no arguments and only returns a constant.
    let answer = unsafe { musl_c_answer() };
    assert_eq!(answer, 42, "the C function returned the wrong value");
    println!("C code built for musl returned {answer}");
}
