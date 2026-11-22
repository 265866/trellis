// edition:2024
//! Statics written directly in the sections that run code before `main`, on every target, and in
//! sections that don't.

extern "C" fn init() {}

// Flagged: ELF's array of constructors.
#[used]
#[unsafe(link_section = ".init_array")]
static INIT_ARRAY: extern "C" fn() = init;

// Flagged: the same, with the priority suffix the linker sorts by.
#[used]
#[unsafe(link_section = ".init_array.00100")]
static INIT_ARRAY_PRIORITY: extern "C" fn() = init;

// Flagged: ELF's older list of constructors.
#[used]
#[unsafe(link_section = ".ctors")]
static CTORS: extern "C" fn() = init;

// Flagged: the same, with a priority suffix.
#[used]
#[unsafe(link_section = ".ctors.65435")]
static CTORS_PRIORITY: extern "C" fn() = init;

// Flagged: Apple's constructors.
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static MOD_INIT_FUNC: extern "C" fn() = init;

// Flagged: the same, with the section type.
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func,mod_init_funcs")]
static MOD_INIT_FUNC_TYPED: extern "C" fn() = init;

// Flagged: the Windows CRT's C++ initializers.
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static CRT_XCU: extern "C" fn() = init;

// Flagged: the same, at another place in their order.
#[used]
#[unsafe(link_section = ".CRT$XCT")]
static CRT_XCT: extern "C" fn() = init;

// Flagged: inside a function, the way a macro might write it.
fn nested() {
    const _: () = {
        #[used]
        #[unsafe(link_section = ".init_array")]
        static NESTED: extern "C" fn() = init;
    };
}

// Not flagged: an ordinary data section.
#[used]
#[unsafe(link_section = ".data.custom")]
static CUSTOM: u32 = 1;

// Not flagged: names that only start like a constructor section.
#[used]
#[unsafe(link_section = ".init_arrays")]
static INIT_ARRAYS: u32 = 1;

#[used]
#[unsafe(link_section = ".ctorsx")]
static CTORSX: u32 = 1;

#[used]
#[unsafe(link_section = "__DATA,__mod_init_funcs")]
static MOD_INIT_FUNCS: u32 = 1;

// Not flagged: a static in no section of its own.
static PLAIN: extern "C" fn() = init;

fn main() {
    nested();
}
