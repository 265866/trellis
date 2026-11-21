pub fn total() -> u32 {
    #[path = "totals_parts.rs"]
    mod parts;
    parts::count()
}
