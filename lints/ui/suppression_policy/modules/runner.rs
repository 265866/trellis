pub fn run() -> u32 {
    mod inner {
        pub(super) mod deeper {}

        pub(super) const fn two() -> u32 {
            2
        }

        pub(super) const fn three() -> u32 {
            3
        }
    }
    inner::two() + inner::three()
}
