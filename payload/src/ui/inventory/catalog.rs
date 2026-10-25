//! Approved navigation destinations, independent of the live page implementations.

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
}

impl PageId {
    pub(crate) const ALL: [Self; 8] = [
        Self::Overview,
        Self::Body,
        Self::Hands,
        Self::Weapons,
        Self::Movement,
        Self::Rendering,
        Self::Input,
        Self::Capture,
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
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum FeatureSection {
    #[default]
    Controls,
    Status,
    Visualizations,
}

impl FeatureSection {
    pub(crate) const ALL: [Self; 3] = [Self::Controls, Self::Status, Self::Visualizations];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Controls => "Controls/tuning",
            Self::Status => "Live status",
            Self::Visualizations => "Visualizations",
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
        Calibration => ("body.calibration", "Calibration"),
        HeadView => ("body.head-view", "Head and view"),
        Posture => ("body.posture", "Posture and roomscale"),
        RootWalls => ("body.root-walls", "Body root and walls"),
        Torso => ("body.torso", "Torso and pelvis"),
        Procedural => ("body.procedural", "Procedural body"),
        AnimationIk => ("body.animation-ik", "Animation and IK"),
        Presentation => ("body.presentation", "Body presentation"),
    },
    Hands {
        HandTracking => ("hands.tracking", "Hand tracking and poses"),
        Fingers => ("hands.fingers", "Finger curls and grip poses"),
        PhysicalHands => ("hands.physical", "Physical hands and held objects"),
        CharactersDoors => ("hands.characters-doors", "Characters and doors"),
        Snatch => ("hands.snatch", "Reach and grapple snatch"),
        Feedback => ("hands.feedback", "Feedback: haptics and impact sounds"),
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
        Locomotion => ("movement.stick", "Stick movement and turning"),
        Grapple => ("movement.grapple", "Grapple traversal"),
        Swimming => ("movement.swimming", "Swimming"),
        Climbing => ("movement.climbing", "Climbing and stunt positions"),
        Driving => ("movement.driving", "Driving view and passengers"),
        VehicleControls => ("movement.vehicle-controls", "Physical vehicle controls"),
        Mounted => ("movement.mounted", "Mounted guns"),
        Wingsuit => ("movement.wingsuit", "Wingsuit"),
        Parachute => ("movement.parachute", "Parachute"),
    },
    Rendering {
        Runtime => ("rendering.runtime", "Runtime and eye output"),
        Stereo => ("rendering.stereo", "Stereo and image corrections"),
        Resolution => ("rendering.resolution", "Resolution, FSR, and foveation"),
        Hud => ("rendering.hud", "HUD and panels"),
        Graphics => ("rendering.experiments", "Graphics experiments"),
        Previews => ("rendering.previews", "Previews"),
        Performance => ("rendering.performance", "Performance"),
        RenderDiagnostics => ("rendering.diagnostics", "Render diagnostics"),
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
        RenderCapture => ("capture.render", "Render trace capture"),
        Replay => ("capture.replay", "Replay, scenarios, and signals"),
        DirectorSafety => ("capture.director-safety", "Director: time, safety, and heat"),
        DirectorSpawn => ("capture.director-spawn", "Director: teleport and spawn"),
        Environment => ("capture.environment", "Environment: time of day and weather"),
        Video => ("capture.video", "Video recording and take markers"),
        Keyboard => ("capture.keyboard", "Keyboard shortcuts"),
        Presets => ("capture.presets", "Test presets"),
        Sandbox => ("capture.inventory", "Inventory sandbox"),
        Engine => ("capture.engine", "Engine experiments"),
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
