//! What a person who uses a project sees, as the `interfaces` setting names it.

/// One of the interfaces `interfaces` names. The first one a project lists is its primary one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interface {
    /// Pages in a browser.
    Web,
    /// A window.
    Desktop,
    /// A full-screen terminal app.
    Terminal,
    /// Commands that print and exit.
    Cli,
    /// No UI: it serves or produces.
    Service,
    /// A library.
    Library,
}

impl Interface {
    /// Every interface, by the name `interfaces` writes it with.
    pub(crate) const ALL: [(&'static str, Self); 6] = [
        ("web", Self::Web),
        ("desktop", Self::Desktop),
        ("terminal", Self::Terminal),
        ("cli", Self::Cli),
        ("service", Self::Service),
        ("library", Self::Library),
    ];

    /// The interface `name` writes, or `None` if it names none.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, interface)| *interface)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_name_parses_to_its_interface() {
        for (name, interface) in Interface::ALL {
            assert_eq!(
                Interface::parse(name),
                Some(interface),
                "{name} should parse"
            );
        }
        assert_eq!(
            Interface::parse("Web"),
            None,
            "a name is lowercase, as Copier stores it"
        );
        assert_eq!(
            Interface::parse("tui"),
            None,
            "an unknown name parses to none"
        );
    }
}
