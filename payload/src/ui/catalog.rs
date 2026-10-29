//! Approved navigation destinations and stable persisted feature keys.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PageId {
    Status,
    Body,
    Hands,
    Weapons,
    Movement,
    Rendering,
    Input,
    Tools,
    Experimental,
}

impl PageId {
    pub(crate) const ALL: [Self; 9] = [
        Self::Status,
        Self::Body,
        Self::Hands,
        Self::Weapons,
        Self::Movement,
        Self::Rendering,
        Self::Input,
        Self::Tools,
        Self::Experimental,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::Body => "Body/tracking",
            Self::Hands => "Hands/interaction",
            Self::Weapons => "Weapons",
            Self::Movement => "Movement/vehicles",
            Self::Rendering => "Rendering",
            Self::Input => "Input/bindings",
            Self::Tools => "Tools",
            Self::Experimental => "Experimental",
        }
    }
}

macro_rules! features {
    ($($page:ident { $($variant:ident => ($key:literal, $label:literal)),+ $(,)? }),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub(crate) enum FeatureId {
            $($($variant,)+)+
        }

        pub(crate) const FEATURES: &[Feature] = &[
            $($(Feature {
                id: FeatureId::$variant,
                key: $key,
                label: $label,
                page: PageId::$page,
            },)+)+
        ];
    };
}

features! {
    Status {
        Status => ("status", "Status"),
    },
    Body {
        HeadView => ("body.head-view", "Head and view"),
        Posture => ("body.posture", "Posture"),
        BodyFit => ("body.fit", "Body fit"),
        AnimationIk => ("body.animation-ik", "Animation and IK"),
        Presentation => ("body.presentation", "Hand presentation"),
    },
    Hands {
        GripFingers => ("hands.grip-fingers", "Grip and fingers"),
        PhysicalHands => ("hands.physical", "Physical hands"),
        CharactersDoors => ("hands.characters-doors", "Characters and doors"),
        Snatch => ("hands.snatch", "Snatch"),
        Feedback => ("hands.feedback", "Feedback"),
    },
    Weapons {
        Hold => ("weapons.hold", "Hold, transfer, and grab regions"),
        Support => ("weapons.support", "Support, weight, and recoil"),
        Aim => ("weapons.aim", "Aim, sights, and scopes"),
        Reload => ("weapons.reload", "Reload and gun action"),
        Inventory => ("weapons.inventory", "Inventory and body slots"),
        Explosives => ("weapons.explosives", "Grenades, C4, and wrist detonator"),
        WeaponSelection => ("weapons.selection", "Weapon selection"),
    },
    Movement {
        OnFoot => ("movement.on-foot", "On foot"),
        Grapple => ("movement.grapple", "Grapple"),
        Climbing => ("movement.climbing", "Climbing"),
        Vehicles => ("movement.vehicles", "Vehicles"),
        Air => ("movement.air", "Air"),
    },
    Rendering {
        Runtime => ("rendering.runtime", "Runtime"),
        Stereo => ("rendering.stereo", "Stereo"),
        Foveation => ("rendering.foveation", "Foveation"),
        Hud => ("rendering.hud", "HUD"),
        Previews => ("rendering.previews", "Previews and performance"),
    },
    Input {
        Devices => ("input.devices", "Devices and thresholds"),
        Wrist => ("input.wrist", "Wrist summary"),
        Semantic => ("input.semantic", "Self-check"),
    },
    Tools {
        TrackingCapture => ("tools.tracking", "Tracking capture"),
        ControllerCapture => ("tools.controllers", "Controller capture"),
        RenderCapture => ("tools.render", "Render capture"),
        Replay => ("tools.replay", "Replay"),
        Director => ("tools.director", "Director"),
        Environment => ("tools.environment", "Environment"),
        Video => ("tools.video", "Video"),
        Presets => ("tools.presets", "Presets"),
    },
    Experimental {
        SinglePass => ("experimental.single-pass", "Single-pass stereo"),
        FarField => ("experimental.far-field", "Far field"),
        PostFx => ("experimental.post-fx", "Post-FX skips"),
        RenderRe => ("experimental.render-re", "Render RE window"),
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Feature {
    pub id: FeatureId,
    pub key: &'static str,
    pub label: &'static str,
    pub page: PageId,
}

impl FeatureId {
    pub(crate) fn definition(self) -> &'static Feature {
        FEATURES
            .iter()
            .find(|feature| feature.id == self)
            .expect("registered feature")
    }

    pub(crate) fn from_key(key: &str) -> Option<Self> {
        FEATURES
            .iter()
            .find(|feature| feature.key == key)
            .map(|feature| feature.id)
    }
}
