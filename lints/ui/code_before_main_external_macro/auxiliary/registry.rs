// edition:2021
//! A dependency like `inventory`, whose `submit!` registers a value before `main` with a static in
//! each target's constructor section, written in the 2021 edition's plain `link_section` form.

#[macro_export]
macro_rules! submit {
    ($value:expr) => {
        const _: () = {
            static VALUE: fn() = $value;
            unsafe extern "C" fn constructor() {
                let _ = VALUE;
            }
            #[used]
            #[cfg_attr(target_os = "linux", link_section = ".init_array")]
            #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__mod_init_func,mod_init_funcs")]
            #[cfg_attr(windows, link_section = ".CRT$XCU")]
            static CONSTRUCTOR: unsafe extern "C" fn() = constructor;
        };
    };
}
