//! The crates the payload is built from and the dependency order between them.

use std::fmt;

/// One of the crates the payload is built from, from the bottom of the stack to the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Core,
    Input,
    View,
    Render,
    Capture,
    Body,
    Physical,
    Gameplay,
    Hud,
    Tooling,
    Ui,
    Payload,
}

impl Layer {
    pub const ALL: [Layer; 12] = [
        Layer::Core,
        Layer::Input,
        Layer::View,
        Layer::Render,
        Layer::Capture,
        Layer::Body,
        Layer::Physical,
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
            Layer::View => "view",
            Layer::Render => "render",
            Layer::Capture => "capture",
            Layer::Body => "body",
            Layer::Physical => "physical",
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
    /// Each crate may depend on any crate below it, with three exceptions. Input sits directly
    /// above core but depends on no layer crate, so it stays pure and builds and tests on any host.
    /// Render sits side by side with body and gameplay on the view crate, so render and those two
    /// may not depend on each other: they reach the render passes through the registries and cells
    /// the view crate holds, and a render edit rebuilds neither. Gameplay sits on body, which holds
    /// the subsystems it builds on and fills through registrations, and on physical, which sits on
    /// body beside render and below gameplay. Capture sits on render, beside
    /// body and gameplay, and
    /// only tooling and the layers above it see it: the render hooks reach the capture tooling
    /// through the probe tables the capture crate fills, so a capture edit rebuilds neither render
    /// nor gameplay nor the HUD. HUD and tooling sit side by side between gameplay and the UI, so
    /// neither may depend on the other.
    pub fn may_depend_on(self, other: Layer) -> bool {
        let below: &[Layer] = match self {
            Layer::Core | Layer::Input => &[],
            Layer::View => &[Layer::Core, Layer::Input],
            Layer::Render | Layer::Body => &[Layer::Core, Layer::Input, Layer::View],
            Layer::Capture => &[Layer::Core, Layer::Input, Layer::View, Layer::Render],
            Layer::Physical => &[Layer::Core, Layer::Input, Layer::View, Layer::Body],
            Layer::Gameplay => &[
                Layer::Core,
                Layer::Input,
                Layer::View,
                Layer::Body,
                Layer::Physical,
            ],
            Layer::Hud => &[
                Layer::Core,
                Layer::Input,
                Layer::View,
                Layer::Render,
                Layer::Body,
                Layer::Physical,
                Layer::Gameplay,
            ],
            Layer::Tooling => &[
                Layer::Core,
                Layer::Input,
                Layer::View,
                Layer::Render,
                Layer::Capture,
                Layer::Body,
                Layer::Physical,
                Layer::Gameplay,
            ],
            Layer::Ui => &[
                Layer::Core,
                Layer::Input,
                Layer::View,
                Layer::Render,
                Layer::Capture,
                Layer::Body,
                Layer::Physical,
                Layer::Gameplay,
                Layer::Hud,
                Layer::Tooling,
            ],
            Layer::Payload => &Layer::ALL[..Layer::ALL.len() - 1],
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
    fn render_and_gameplay_are_siblings_on_view() {
        assert!(!Layer::Gameplay.may_depend_on(Layer::Render));
        assert!(!Layer::Render.may_depend_on(Layer::Gameplay));
        assert!(Layer::Gameplay.may_depend_on(Layer::View));
        assert!(Layer::Render.may_depend_on(Layer::View));
        assert!(!Layer::View.may_depend_on(Layer::Render));
        assert!(Layer::Hud.may_depend_on(Layer::Render));
        assert!(Layer::Tooling.may_depend_on(Layer::Render));
    }

    #[test]
    fn capture_sits_on_render_below_tooling() {
        assert!(Layer::Capture.may_depend_on(Layer::Render));
        assert!(Layer::Capture.may_depend_on(Layer::View));
        assert!(!Layer::Capture.may_depend_on(Layer::Gameplay));
        for below in [
            Layer::View,
            Layer::Render,
            Layer::Body,
            Layer::Gameplay,
            Layer::Hud,
        ] {
            assert!(!below.may_depend_on(Layer::Capture), "{below}");
        }
        assert!(Layer::Tooling.may_depend_on(Layer::Capture));
        assert!(Layer::Ui.may_depend_on(Layer::Capture));
    }

    #[test]
    fn body_sits_between_view_and_gameplay_beside_render() {
        assert!(Layer::Body.may_depend_on(Layer::View));
        assert!(Layer::Gameplay.may_depend_on(Layer::Body));
        assert!(!Layer::Body.may_depend_on(Layer::Gameplay));
        assert!(!Layer::Body.may_depend_on(Layer::Render));
        assert!(!Layer::Render.may_depend_on(Layer::Body));
        assert!(Layer::Hud.may_depend_on(Layer::Body));
        assert!(Layer::Tooling.may_depend_on(Layer::Body));
        assert!(Layer::Ui.may_depend_on(Layer::Body));
    }

    #[test]
    fn physical_sits_on_body_below_gameplay() {
        assert!(Layer::Physical.may_depend_on(Layer::Body));
        assert!(Layer::Physical.may_depend_on(Layer::View));
        assert!(!Layer::Physical.may_depend_on(Layer::Gameplay));
        assert!(!Layer::Physical.may_depend_on(Layer::Render));
        assert!(!Layer::Physical.may_depend_on(Layer::Capture));
        assert!(!Layer::Body.may_depend_on(Layer::Physical));
        assert!(!Layer::Render.may_depend_on(Layer::Physical));
        for above in [Layer::Gameplay, Layer::Hud, Layer::Tooling, Layer::Ui] {
            assert!(above.may_depend_on(Layer::Physical), "{above}");
        }
    }

    #[test]
    fn input_depends_on_no_layer() {
        for layer in Layer::ALL {
            assert!(!Layer::Input.may_depend_on(layer), "{layer}");
        }
        assert!(Layer::View.may_depend_on(Layer::Input));
    }

    #[test]
    fn everything_below_is_allowed() {
        assert!(Layer::View.may_depend_on(Layer::Core));
        assert!(Layer::Render.may_depend_on(Layer::View));
        assert!(Layer::Hud.may_depend_on(Layer::Gameplay));
        assert!(!Layer::View.may_depend_on(Layer::Gameplay));
        assert!(!Layer::Core.may_depend_on(Layer::Input));
        for layer in Layer::ALL {
            assert_eq!(Layer::Payload.may_depend_on(layer), layer != Layer::Payload);
        }
    }
}
