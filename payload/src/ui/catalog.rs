//! Approved navigation destinations and stable persisted feature keys.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PageId {
    Overview,
    Body,
    Hands,
    Weapons,
    Movement,
    Rendering,
    Input,
    Capture,
    Experimental,
}

impl PageId {
    pub(crate) const ALL: [Self; 9] = [
        Self::Overview,
        Self::Body,
        Self::Hands,
        Self::Weapons,
        Self::Movement,
        Self::Rendering,
        Self::Input,
        Self::Capture,
        Self::Experimental,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Body => "Body/tracking",
            Self::Hands => "Hands/interaction",
            Self::Weapons => "Weapons",
            Self::Movement => "Movement/vehicles",
            Self::Rendering => "Rendering",
            Self::Input => "Input/bindings",
            Self::Capture => "Capture/testing",
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
    Overview {
        Session => ("overview.session", "Session and input summary"),
        AllStatus => ("overview.status", "All feature status / Needs attention"),
        ActiveTools => ("overview.active-tools", "Performance monitor / Active captures / Active visualizations"),
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
        Devices => ("input.devices", "Scheme, handedness, and devices"),
        Thresholds => ("input.thresholds", "Input thresholds and recovery"),
        Bindings => ("input.bindings", "Context and binding reference"),
        MenuPanel => ("input.menu-panel", "Menu and panel input"),
        Wrist => ("input.wrist", "Wrist controls summary"),
        Semantic => ("input.semantic", "Semantic actions and self-check"),
    },
    Capture {
        TrackingCapture => ("capture.tracking", "Tracking recording"),
        ControllerCapture => ("capture.controllers", "Controller diagnostics capture"),
        RenderCapture => ("capture.render", "Render capture"),
        Replay => ("capture.replay", "Replay, scenarios, and signals"),
        DirectorSafety => ("capture.director-safety", "Director: time, safety, and heat"),
        DirectorSpawn => ("capture.director-spawn", "Director: teleport and spawn"),
        Environment => ("capture.environment", "Environment: time of day and weather"),
        Video => ("capture.video", "Video recording and take markers"),
        Keyboard => ("capture.keyboard", "Keyboard shortcuts"),
        Presets => ("capture.presets", "Test presets"),
        Engine => ("capture.engine", "Engine experiments"),
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
