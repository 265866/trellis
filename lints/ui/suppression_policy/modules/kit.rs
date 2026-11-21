pub struct Toolbox;

pub fn wrench() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn wrenches() {
        assert_eq!(super::wrench(), 1, "a toolbox should hold one wrench");
    }
}
