//! The eight target crates and the dependency order between them.

use std::fmt;

/// One of the crates the payload is split into, from the bottom of the stack to the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Core,
    Input,
    Render,
    Gameplay,
    Hud,
    Tooling,
    Ui,
    Payload,
}

impl Layer {
    pub const ALL: [Layer; 8] = [
        Layer::Core,
        Layer::Input,
        Layer::Render,
        Layer::Gameplay,
        Layer::Hud,
        Layer::Tooling,
        Layer::Ui,
        Layer::Payload,
    ];

    /// The short name the data files use.
    pub fn name(self) -> &'static str {
        match self {
            Layer::Core => "core",
            Layer::Input => "input",
            Layer::Render => "render",
            Layer::Gameplay => "gameplay",
            Layer::Hud => "hud",
            Layer::Tooling => "tooling",
            Layer::Ui => "ui",
            Layer::Payload => "payload",
        }
    }

    pub fn from_name(name: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|layer| layer.name() == name)
    }

    /// The Cargo package name of the layer's crate.
    ///
    /// The payload keeps its historical `jc3vr_payload` name, which the injector and the scripts
    /// expect; the others are `jc3vr-<name>`.
    pub fn package(self) -> String {
        match self {
            Layer::Payload => "jc3vr_payload".to_owned(),
            layer => format!("jc3vr-{}", layer.name()),
        }
    }

    /// The layer whose crate has this package or library name, accepting either the hyphenated
    /// package spelling or the underscored spelling that Rust paths use.
    pub fn from_package(package: &str) -> Option<Layer> {
        let normalized = package.replace('_', "-");
        Layer::ALL
            .into_iter()
            .find(|layer| layer.package().replace('_', "-") == normalized)
    }

    /// Whether this layer's crate may depend on `other`'s.
    ///
    /// Each crate may depend on any crate below it, with one exception: input sits directly above
    /// core but depends on no layer crate, so it stays pure and builds and tests on any host. HUD
    /// and tooling sit side by side between gameplay and the UI, so neither may depend on the
    /// other.
    pub fn may_depend_on(self, other: Layer) -> bool {
        let below: &[Layer] = match self {
            Layer::Core | Layer::Input => &[],
            Layer::Render => &[Layer::Core, Layer::Input],
            Layer::Gameplay => &[Layer::Core, Layer::Input, Layer::Render],
            Layer::Hud | Layer::Tooling => {
                &[Layer::Core, Layer::Input, Layer::Render, Layer::Gameplay]
            }
            Layer::Ui => &[
                Layer::Core,
                Layer::Input,
                Layer::Render,
                Layer::Gameplay,
                Layer::Hud,
                Layer::Tooling,
            ],
            Layer::Payload => &Layer::ALL[..7],
        };
        below.contains(&other)
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for layer in Layer::ALL {
            assert_eq!(Layer::from_name(layer.name()), Some(layer));
            assert_eq!(Layer::from_package(&layer.package()), Some(layer));
        }
        assert_eq!(Layer::from_name("rendering"), None);
    }

    #[test]
    fn package_spellings() {
        assert_eq!(Layer::from_package("jc3vr_core"), Some(Layer::Core));
        assert_eq!(Layer::from_package("jc3vr-gameplay"), Some(Layer::Gameplay));
        assert_eq!(Layer::from_package("jc3vr-payload"), Some(Layer::Payload));
        assert_eq!(Layer::from_package("jc3vr_widgets"), None);
    }

    #[test]
    fn never_depends_on_itself() {
        for layer in Layer::ALL {
            assert!(!layer.may_depend_on(layer), "{layer}");
        }
    }

    #[test]
    fn order_is_acyclic() {
        for a in Layer::ALL {
            for b in Layer::ALL {
                assert!(
                    !(a.may_depend_on(b) && b.may_depend_on(a)),
                    "{a} and {b} may depend on each other"
                );
            }
        }
    }

    #[test]
    fn hud_and_tooling_are_siblings() {
        assert!(!Layer::Hud.may_depend_on(Layer::Tooling));
        assert!(!Layer::Tooling.may_depend_on(Layer::Hud));
        assert!(Layer::Ui.may_depend_on(Layer::Hud));
        assert!(Layer::Ui.may_depend_on(Layer::Tooling));
    }

    #[test]
    fn input_depends_on_no_layer() {
        for layer in Layer::ALL {
            assert!(!Layer::Input.may_depend_on(layer), "{layer}");
        }
        assert!(Layer::Render.may_depend_on(Layer::Input));
    }

    #[test]
    fn everything_below_is_allowed() {
        assert!(Layer::Render.may_depend_on(Layer::Core));
        assert!(Layer::Gameplay.may_depend_on(Layer::Render));
        assert!(!Layer::Render.may_depend_on(Layer::Gameplay));
        assert!(!Layer::Core.may_depend_on(Layer::Input));
        for layer in Layer::ALL {
            assert_eq!(Layer::Payload.may_depend_on(layer), layer != Layer::Payload);
        }
    }
}
