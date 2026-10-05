//! Weapon handling: the grab volume each hand is in, the wielded weapon, the grabs and what they
//! did, the foregrip, the hand tiers, the recoil, the haptics, the dual-wield split, the grapple
//! line and yank, the grenade throw, the C4 throw and the wrist detonator, the true reload, the empty weapon, the one-handed weight, the gun-in-hand watch, the fire
//! input with empty hands, per-hand aim assist, and the weapon-handling settings.

use egui::Ui;

use crate::{
    controllers::{
        aim::{self as controller_aim, AimStatus},
        bindings::{self, yank::Yank},
        config::WeaponsConfig,
        frame::Hand,
        hands::{
            anchor::{self, AnchorStatus},
            tiers::{self, TierState, TierStatus},
        },
        weapons::{
            self, WeaponsStatus,
            config::{
                DualWieldConfig, ForegripConfig, GrenadeConfig, HapticsConfig, HeftConfig,
                HeftTuning, HolsterAnchor, HolstersConfig, PulseSpec, RecoilConfig, VolumeSpec,
            },
            dual_wield::{self, DualWieldStatus},
            empty::{self, EmptyStatus},
            foregrip::{self, ForegripStatus},
            grenade::{self, GrenadeStatus},
            grip_watch::{self, GripWatchStatus},
            haptics::{self, HapticsStatus},
            heft::{self, HeftStatus},
            holster::slot_label,
            recoil::{self, RecoilStatus},
            reload::{self, ReloadStatus},
            unarmed_fire::{self, UnarmedFireStatus},
        },
    },
    ui::controllers::{
        explosives::{ExplosivesReadout, explosives_readout, explosives_settings},
        reload::{reload_readout, reload_settings},
        vec3, vec3_drag, yes_no,
    },
};

/// Everything the weapon-handling section shows, read before the config is locked for editing.
pub(super) struct WeaponsReadout {
    status: WeaponsStatus,
    foregrip: ForegripStatus,
    tiers: TierStatus,
    recoil: RecoilStatus,
    haptics: HapticsStatus,
    dual_wield: DualWieldStatus,
    anchor: AnchorStatus,
    yanks: (u64, Option<Yank>),
    grenade: GrenadeStatus,
    explosives: ExplosivesReadout,
    aim: AimStatus,
    empty: EmptyStatus,
    heft: HeftStatus,
    grip_watch: GripWatchStatus,
    unarmed_fire: UnarmedFireStatus,
    reload: ReloadStatus,
}

impl WeaponsReadout {
    pub(super) fn read() -> Self {
        Self {
            status: weapons::status(),
            foregrip: foregrip::status(),
            tiers: tiers::status(),
            recoil: recoil::status(),
            haptics: haptics::status(),
            dual_wield: dual_wield::status(),
            anchor: anchor::status(),
            yanks: bindings::yank_status(),
            grenade: grenade::status(),
            explosives: ExplosivesReadout::read(),
            aim: controller_aim::status(),
            empty: empty::status(),
            heft: heft::status(),
            grip_watch: grip_watch::status(),
            unarmed_fire: unarmed_fire::status(),
            reload: reload::status(),
        }
    }
}

pub(super) fn weapons_readout(ui: &mut Ui, readout: &WeaponsReadout) {
    let WeaponsReadout {
        status,
        foregrip,
        tiers,
        ..
    } = readout;
    if !status.active {
        ui.label("Inactive: the bindings are not injecting.");
    }
    ui.label(format!(
        "State: {} · volumes placed {} · fire held back {}",
        tiers.state.map_or("none yet", TierState::label),
        status.candidates,
        yes_no(status.fire_blocked)
    ));
    ui.label(format!(
        "Body volumes placed for: {}",
        match status.stance {
            Some(stance) => stance.label(),
            None => "nothing",
        }
    ));
    if let Some((hand, miss)) = status.last_miss {
        ui.label(format!(
            "Last near miss: {} hand {:.2} m outside the {} (offset from its centre {})",
            hand.label(),
            miss.outside_m,
            miss.volume.label(),
            vec3(miss.offset)
        ));
    }
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            ui.strong(hand.label());
            ui.label(format!(
                "in: {}",
                status.volumes[hand.index()].map_or("no volume", |v| v.label())
            ));
            ui.label(format!(
                "tier: {} (acts as {}) · anchor {:.2}",
                tiers.tiers[hand.index()].label(),
                tiers.effective[hand.index()].label(),
                tiers.anchor_weights[hand.index()]
            ));
            let r = &readout.recoil;
            ui.label(format!(
                "recoil: pitch {:+.1}° yaw {:+.1}° back {:.3} m · kicks {}",
                r.offsets[hand.index()].x,
                r.offsets[hand.index()].y,
                r.offsets[hand.index()].z,
                r.kicks[hand.index()]
            ));
            let d = &readout.dual_wield;
            ui.label(format!(
                "trigger {} fires gun {} · shots {}",
                if d.triggers[hand.index()].held {
                    "held"
                } else {
                    "up"
                },
                d.guns[hand.index()].map_or("none".to_owned(), |g| g.to_string()),
                d.shots[hand.index()]
            ));
        }
    });
    dual_wield_readout(ui, &readout.dual_wield);
    grapple_readout(ui, &readout.anchor, readout.yanks);
    grenade_readout(ui, &readout.grenade);
    explosives_readout(ui, &readout.explosives);
    reload_readout(ui, &readout.reload);
    empty_readout(ui, &readout.empty);
    heft_readout(ui, &readout.heft);
    grip_watch_readout(ui, &readout.grip_watch);
    ui.label(format!(
        "Fire with empty hands: draws held back {}",
        readout.unarmed_fire.held_back
    ));
    assist_readout(ui, &readout.aim);
    haptics_readout(ui, &readout.haptics);
    match &status.wielded {
        Some(w) => {
            let hands = Hand::BOTH
                .iter()
                .filter(|h| w.weapon.wielded.hands[h.index()])
                .map(|h| h.label())
                .collect::<Vec<_>>()
                .join(" and ");
            ui.label(format!(
                "Wielded: {} in {} · two-handed {}",
                slot_label(w.weapon.wielded.slot),
                if hands.is_empty() { "no hand" } else { &hands },
                yes_no(w.weapon.two_handed)
            ));
            if let Some(g) = w.geometry {
                ui.label(format!(
                    "Foregrip {} in the wrist frame ({}) · barrel {}",
                    vec3(g.foregrip),
                    if g.native {
                        "the weapon's grip"
                    } else {
                        "fallback"
                    },
                    vec3(g.barrel)
                ));
            } else if w.weapon.two_handed {
                ui.label("Foregrip: not measured yet");
            }
        }
        None => {
            ui.label("Wielded: nothing");
        }
    }
    ui.label(match foregrip.hold {
        Some(h) if h.engaged => format!(
            "Foregrip: held by the {} hand · weight {:.2}",
            h.holder.other().label(),
            h.weight
        ),
        Some(h) => format!("Foregrip: letting go · weight {:.2}", h.weight),
        None => "Foregrip: not held".to_owned(),
    });
    if let Some(t) = foregrip.turn {
        ui.label(format!(
            "Foregrip turn: {:.1}° wanted{} · span share {:.2}",
            t.wanted_deg,
            if t.limited {
                " (held at the limit)"
            } else {
                ""
            },
            t.span_fade
        ));
    }
    ui.label(format!(
        "Foregrip grabs {} · releases {}{}",
        foregrip.grabs,
        foregrip.releases,
        foregrip
            .last_release
            .map_or(String::new(), |r| format!(" (last: {})", r.label()))
    ));
    ui.label(format!("Grabs: {}", status.grabs));
    for event in status.events.iter().rev() {
        ui.label(format!(
            "  {} at the {}: {}",
            event.hand.label(),
            event.volume.label(),
            event.outcome.label()
        ));
    }
}

fn dual_wield_readout(ui: &mut Ui, d: &DualWieldStatus) {
    ui.label(format!(
        "Dual wield: split {} · last update handed {} · pinned gun {}",
        if d.active { "acting" } else { "idle" },
        d.chosen.map_or("no hand", Hand::label),
        d.pinned.map_or("none".to_owned(), |g| g.to_string()),
    ));
    let a = &d.attach;
    let drawn = |hand: Hand| {
        if a.drawn[hand.index()] {
            "drawn"
        } else {
            "holstered"
        }
    };
    ui.label(format!(
        "Pistol bones: owned {} · wielded {} · left {} · right {} · first pistol in {} · waiting draw {} · writes {} · snaps {}",
        yes_no(a.owned),
        yes_no(a.wielded),
        drawn(Hand::Left),
        drawn(Hand::Right),
        a.gun_hands.map_or("the game's layout", |h| h[0].label()),
        a.pending.map_or("none", Hand::label),
        a.writes,
        a.snaps
    ));
}

fn grapple_readout(ui: &mut Ui, a: &AnchorStatus, (yanks, last): (u64, Option<Yank>)) {
    ui.label(format!(
        "Grapple line: live {} · attached at {}",
        yes_no(a.wire),
        a.anchor.map_or("nothing".to_owned(), vec3)
    ));
    ui.label(match a.displacement {
        Some(d) => format!(
            "Hang: hand moved ({:+.2}, {:+.2}) m · impulse ({:+.1}, {:+.1}) · swings {}",
            d.x, d.y, a.impulse.x, a.impulse.y, a.swings
        ),
        None => format!("Hang: not swinging · swings {}", a.swings),
    });
    ui.label(match last {
        Some(y) => format!(
            "Yanks: {yanks} · last {:.2} m at {:.1} m/s",
            y.distance_m, y.speed_mps
        ),
        None => format!("Yanks: {yanks}"),
    });
}

fn grenade_readout(ui: &mut Ui, g: &GrenadeStatus) {
    let hand = g
        .stage
        .hand()
        .map_or(String::new(), |h| format!(" in the {} hand", h.label()));
    ui.label(format!(
        "Grenade: {}{hand} · primed {} · thrown {} · gone before the throw {}{}",
        g.stage.label(),
        g.primed,
        g.thrown,
        g.gone,
        if g.weapon_seen {
            ""
        } else {
            " · the grenade weapon has not updated yet"
        }
    ));
    if let Some(f) = g.fuse {
        ui.label(format!(
            "  fuse {:.1} s of {:.1} s",
            f.remaining_s.max(0.0),
            f.total_s
        ));
    }
    if let Some(v) = g.last_release {
        ui.label(format!(
            "  last release {} ({:.1} m/s)",
            vec3(v),
            v.length()
        ));
    }
    for event in g.events.iter().rev() {
        ui.label(format!(
            "  {}{}",
            event
                .hand
                .map_or(String::new(), |h| format!("{}: ", h.label())),
            event.outcome.label()
        ));
    }
}

fn empty_readout(ui: &mut Ui, e: &EmptyStatus) {
    ui.label(format!(
        "Empty weapon: dry fires {} · automatic switches suppressed {}, allowed {}",
        e.dry_fires, e.suppressed, e.allowed
    ));
}

fn heft_readout(ui: &mut Ui, h: &HeftStatus) {
    let Some(target) = h.target else {
        ui.label(format!(
            "One-handed weight: no weapon · scatter widened {} times",
            h.widened_shots
        ));
        return;
    };
    ui.label(format!(
        "One-handed weight: {} in the {} hand · unbraced {:.2} · reach {:.2}",
        target.class.label(),
        target.hand.label(),
        h.share,
        h.reach
    ));
    ui.label(format!(
        "  sag {:.1}° · sway pitch {:+.2}° yaw {:+.2}° · lag {:.1}° · total {:.1}° · scatter widened {} times",
        h.sag_deg, h.sway_deg.x, h.sway_deg.y, h.lag_deg, h.offset_deg, h.widened_shots
    ));
}

fn grip_watch_readout(ui: &mut Ui, g: &GripWatchStatus) {
    let degrees = |d: Option<f32>| d.map_or("-".to_owned(), |d| format!("{d:.1}°"));
    for hand in Hand::BOTH {
        let i = hand.index();
        let pinned = match g.pinned[i] {
            Some(true) => "pinned",
            Some(false) => "not pinned",
            None => "no gun",
        };
        ui.label(format!(
            "Gun in {} hand ({pinned}): tick change {} · from grab {} · animation off the grip {} · snaps {} · sweeps {} · bone changes {}",
            hand.label(),
            degrees(g.change_deg[i]),
            degrees(g.from_grab_deg[i]),
            degrees(g.animated_deviation_deg[i]),
            g.flips[i],
            g.sweeps[i],
            g.bone_changes[i],
        ));
    }
    if let Some((hand, c)) = g.last_flip {
        ui.label(format!("Last turn: {} hand {c:.0}°", hand.label()));
    }
}

fn assist_readout(ui: &mut Ui, a: &AimStatus) {
    let per_hand = Hand::BOTH
        .iter()
        .map(|h| match a.assist[h.index()] {
            Some(c) => format!("{} {:.1}°", h.label(), c.angle_rad.to_degrees()),
            None => format!("{} none", h.label()),
        })
        .collect::<Vec<_>>()
        .join(", ");
    ui.label(format!(
        "Aim assist: corrections {} · last update {per_hand}",
        a.assist_corrections
    ));
}

fn haptics_readout(ui: &mut Ui, h: &HapticsStatus) {
    ui.label(format!("Haptic pulses: {}", h.routed));
    for event in h.events.iter().rev() {
        let amplitude = |hand: Hand| {
            event.amplitudes[hand.index()].map_or("-".to_owned(), |a| format!("{a:.2}"))
        };
        ui.label(format!(
            "  {}: left {} right {} for {:.0} ms",
            event.source.label(),
            amplitude(Hand::Left),
            amplitude(Hand::Right),
            event.duration_s * 1000.0
        ));
    }
}

pub(super) fn weapons_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.holsters.enabled, "Holsters")
        .on_hover_text("Draw and holster weapons with a grip press at the body holsters.");
    ui.add_enabled_ui(w.holsters.enabled, |ui| {
        ui.indent("holsters", |ui| holster_settings(ui, &mut w.holsters));
    });
    ui.checkbox(&mut w.foregrip.enabled, "Foregrip")
        .on_hover_text("Let the off-hand grab a two-handed weapon's foregrip.");
    ui.add_enabled_ui(w.foregrip.enabled, |ui| {
        ui.indent("foregrip", |ui| foregrip_settings(ui, &mut w.foregrip));
    });
    ui.checkbox(
        &mut w.block_fire_during_actions,
        "Hold fire back during reloads and draws",
    );
    ui.checkbox(&mut w.recoil.enabled, "Recoil")
        .on_hover_text("Kick the gun hand away from the controller on each shot.");
    ui.add_enabled_ui(w.recoil.enabled, |ui| {
        ui.indent("recoil", |ui| recoil_settings(ui, &mut w.recoil));
    });
    ui.checkbox(
        &mut w.recoil.suppress_camera_kick,
        "Suppress the camera recoil while aiming by hand",
    );
    ui.checkbox(&mut w.heft.enabled, "One-handed weight")
        .on_hover_text(
            "Held in one hand, a long gun sags, trails the hand, sways, kicks harder, and shoots \
         wider, by its class. Bracing it on the foregrip takes it all away.",
        );
    ui.add_enabled_ui(w.heft.enabled, |ui| {
        ui.indent("heft", |ui| heft_settings(ui, &mut w.heft));
    });
    ui.checkbox(&mut w.haptics.enabled, "Haptics");
    ui.add_enabled_ui(w.haptics.enabled, |ui| {
        ui.indent("haptics", |ui| haptics_settings(ui, &mut w.haptics));
    });
    dual_wield_settings(ui, &mut w.dual_wield);
}

/// The settings of the grenade throw, the C4 throw and the wrist detonator, and the true reload.
pub(super) fn breadth_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.grenade_throw, "Physical grenade throw").on_hover_text(
        "A grip press at the grenade pouch primes a grenade; opening the grip throws it. The fuse \
         runs from the prime.",
    );
    ui.add_enabled_ui(w.grenade_throw, |ui| {
        ui.indent("grenade", |ui| grenade_settings(ui, &mut w.grenade));
    });
    explosives_settings(ui, w);
    reload_settings(ui, &mut w.reload);
    ui.checkbox(
        &mut w.auto_switch_when_empty,
        "Switch weapons automatically when empty",
    )
    .on_hover_text(
        "Let the game switch to another weapon when the trigger of an empty weapon that cannot \
         reload is pulled. Off, the empty weapon stays in the hand and the pull only dry-fires.",
    );
    ui.checkbox(&mut w.fire_draws_weapon, "Fire draws a weapon")
        .on_hover_text(
            "Let the trigger draw the selected weapon with nothing in the hands, as the game does. \
             Off, weapons come out of the holsters.",
        );
}

fn grenade_settings(ui: &mut Ui, g: &mut GrenadeConfig) {
    vec3_drag(
        ui,
        "Hold offset (grip frame, m)",
        &mut g.hold_offset_m,
        0.005,
    );
    ui.add(egui::Slider::new(&mut g.release_window_s, 0.02..=0.3).text("Release window (s)"));
    ui.add(egui::Slider::new(&mut g.release_peak_span_s, 0.0..=0.05).text("Release peak span (s)"));
    ui.add(egui::Slider::new(&mut g.velocity_scale, 0.25..=3.0).text("Velocity scale"));
    ui.add(egui::Slider::new(&mut g.max_speed_mps, 5.0..=60.0).text("Fastest throw (m/s)"));
    ui.checkbox(
        &mut g.hand_aimed_fallback,
        "The game's own throw aims along the hand",
    );
    ui.add(egui::Slider::new(&mut g.warning_s, 0.0..=5.0).text("Fuse warning (s)"));
    ui.checkbox(&mut g.warning_overlay, "Fuse warning ring");
    ui.checkbox(&mut g.warning_haptics, "Fuse warning pulses");
    pulse_settings(ui, "Warning pulse", &mut g.warning_pulse);
    ui.add(
        egui::Slider::new(&mut g.warning_interval_s[0], 0.05..=1.0).text("Slowest pulse gap (s)"),
    );
    ui.add(
        egui::Slider::new(&mut g.warning_interval_s[1], 0.02..=1.0).text("Fastest pulse gap (s)"),
    );
    pulse_settings(ui, "Prime pulse", &mut g.prime_pulse);
}

fn recoil_settings(ui: &mut Ui, r: &mut RecoilConfig) {
    ui.add(egui::Slider::new(&mut r.kick_scale, 0.0..=5.0).text("Kick"));
    ui.add(egui::Slider::new(&mut r.kick_back_m_per_rad, 0.0..=2.0).text("Kick back (m/rad)"));
    ui.add(egui::Slider::new(&mut r.max_angle_deg, 1.0..=60.0).text("Largest angle (°)"));
    ui.add(
        egui::Slider::new(&mut r.spring_stiffness, 10.0..=2000.0)
            .logarithmic(true)
            .text("Spring stiffness"),
    );
    ui.add(egui::Slider::new(&mut r.spring_damping_ratio, 0.1..=2.0).text("Damping ratio"));
    ui.add(
        egui::Slider::new(&mut r.two_hand_stiffness_scale, 1.0..=5.0).text("Two-hand stiffness"),
    );
    ui.add(egui::Slider::new(&mut r.two_hand_kick_scale, 0.0..=1.0).text("Two-hand kick"));
}

fn haptics_settings(ui: &mut Ui, h: &mut HapticsConfig) {
    ui.add(egui::Slider::new(&mut h.intensity, 0.0..=1.0).text("Intensity"));
    ui.checkbox(&mut h.game_rumble, "Game rumbles")
        .on_hover_text(
            "Weapon fire, damage, impacts, landings, vehicles, and the game's other rumbles.",
        );
    ui.checkbox(&mut h.gamepad_rumble, "Also rumble the gamepad")
        .on_hover_text("Let the physical gamepad rumble too, instead of staying silent.");
    ui.add(
        egui::Slider::new(&mut h.kick_amplitude_per_rad, 0.0..=20.0)
            .text("Kick amplitude (per rad)"),
    );
    ui.add(egui::Slider::new(&mut h.max_pulse_s, 0.02..=2.0).text("Longest one-off pulse (s)"));
    ui.add(
        egui::Slider::new(&mut h.continuous_pulse_s, 0.02..=0.5).text("Continuous pulse hold (s)"),
    );
    ui.checkbox(&mut h.grapple, "Grapple pulses");
    pulse_settings(ui, "Grapple fire", &mut h.grapple_fire);
    pulse_settings(ui, "Grapple attach", &mut h.grapple_attach);
    pulse_settings(ui, "Yank", &mut h.yank);
    pulse_settings(ui, "Dry fire", &mut h.dry_fire);
}

pub(super) fn pulse_settings(ui: &mut Ui, label: &str, p: &mut PulseSpec) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::Slider::new(&mut p.amplitude, 0.0..=1.0).text("amplitude"));
        ui.add(egui::Slider::new(&mut p.duration_s, 0.0..=0.5).text("s"));
    });
}

fn dual_wield_settings(ui: &mut Ui, d: &mut DualWieldConfig) {
    ui.checkbox(&mut d.split, "Dual-wield split").on_hover_text(
        "Each pistol fires from its own hand's trigger; the off-hand's stick click retracts \
         tethers while the off-hand holds a pistol.",
    );
    ui.add_enabled(
        d.split,
        egui::Checkbox::new(&mut d.independent_draw, "Draw each pistol by hand"),
    )
    .on_hover_text(
        "A hip holster grab draws or holsters only the grabbing hand's pistol; an undrawn \
         pistol waits on the hip.",
    );
    ui.add_enabled(
        d.split,
        egui::Checkbox::new(
            &mut d.place_by_handedness,
            "First pistol in the dominant hand",
        ),
    );
}

fn holster_settings(ui: &mut Ui, h: &mut HolstersConfig) {
    ui.horizontal(|ui| {
        ui.label("Holsters ride:");
        for anchor in HolsterAnchor::ALL {
            ui.selectable_value(&mut h.anchor, anchor, anchor.label());
        }
    })
    .response
    .on_hover_text(
        "Rico's skeleton puts the volumes on his hip and shoulder joints, falling back to the \
         player's body in vehicles, ragdolls, and skydives. The player's body turns with the \
         estimated torso.",
    );
    ui.checkbox(&mut h.show_volumes, "Show every holster volume")
        .on_hover_text(
            "Draw all the body volumes through the overlay, the one a hand is in highlighted.",
        );
    ui.checkbox(&mut h.stick_click_cycles, "Move stick click cycles weapons")
        .on_hover_text("The fallback for drawing without reaching a holster.");
    ui.add(egui::Slider::new(&mut h.crouch_deadband_m, 0.0..=0.5).text("Crouch deadband (m)"));
    ui.label("Offsets from the landmark, right side (+X right, +Y up, +Z back)");
    volume_settings(ui, "Hip", &mut h.hip);
    volume_settings(ui, "Shoulder", &mut h.shoulder);
    volume_settings(ui, "Chest", &mut h.chest);
    volume_settings(ui, "Grenade pouch", &mut h.pouch);
    if ui.button("Reset holsters").clicked() {
        *h = HolstersConfig {
            enabled: h.enabled,
            ..HolstersConfig::new()
        };
    }
}

pub(super) fn volume_settings(ui: &mut Ui, label: &str, v: &mut VolumeSpec) {
    for (axis, value) in [
        ("x", &mut v.offset_m.x),
        ("y", &mut v.offset_m.y),
        ("z", &mut v.offset_m.z),
    ] {
        ui.add(egui::Slider::new(value, -0.4..=0.4).text(format!("{label} offset {axis} (m)")));
    }
    ui.add(egui::Slider::new(&mut v.radius_m, 0.02..=0.3).text(format!("{label} radius (m)")));
}

fn foregrip_settings(ui: &mut Ui, f: &mut ForegripConfig) {
    ui.add(egui::Slider::new(&mut f.radius_m, 0.02..=0.3).text("Grab radius (m)"));
    ui.add(egui::Slider::new(&mut f.release_distance_m, 0.05..=0.6).text("Release distance (m)"));
    ui.add(egui::Slider::new(&mut f.blend_s, 0.0..=0.5).text("Blend (s)"));
    ui.add(
        egui::Slider::new(&mut f.fallback_distance_m, 0.1..=0.8)
            .text("Fallback foregrip distance (m)"),
    );
    ui.add(egui::Slider::new(&mut f.min_span_m, 0.0..=0.3).text("Shortest span (m)"));
    ui.add(egui::Slider::new(&mut f.max_turn_deg, 10.0..=180.0).text("Largest turn (°)"));
}

fn heft_settings(ui: &mut Ui, h: &mut HeftConfig) {
    ui.checkbox(&mut h.spread, "Wider spread in one hand");
    ui.add(egui::Slider::new(&mut h.reach_near_m, 0.0..=1.0).text("Reach sag starts at (m)"));
    ui.add(egui::Slider::new(&mut h.reach_far_m, 0.0..=1.2).text("Reach sag full at (m)"));
    ui.add(egui::Slider::new(&mut h.sway_hz, 0.0..=2.0).text("Sway frequency (Hz)"));
    ui.add(egui::Slider::new(&mut h.max_angle_deg, 0.0..=90.0).text("Largest angle (°)"));
    for (label, tuning) in [
        ("Sidearms", &mut h.sidearm),
        ("Rifles", &mut h.rifle),
        ("Heavy", &mut h.heavy),
    ] {
        ui.collapsing(label, |ui| heft_tuning_settings(ui, tuning));
    }
    if ui.button("Reset the weight").clicked() {
        *h = HeftConfig {
            enabled: h.enabled,
            ..HeftConfig::new()
        };
    }
}

fn heft_tuning_settings(ui: &mut Ui, t: &mut HeftTuning) {
    ui.add(egui::Slider::new(&mut t.sag_deg, 0.0..=40.0).text("Sag (°)"));
    ui.add(egui::Slider::new(&mut t.reach_sag_deg, 0.0..=40.0).text("Reach sag (°)"));
    ui.add(
        egui::Slider::new(&mut t.lag_stiffness, 0.0..=2000.0)
            .logarithmic(true)
            .text("Lag stiffness (0 follows rigidly)"),
    );
    ui.add(egui::Slider::new(&mut t.lag_damping_ratio, 0.1..=2.0).text("Lag damping ratio"));
    ui.add(egui::Slider::new(&mut t.sway_deg, 0.0..=5.0).text("Sway (°)"));
    ui.add(egui::Slider::new(&mut t.kick_scale, 0.0..=4.0).text("One-handed kick"));
    ui.add(egui::Slider::new(&mut t.spread_scale, 0.0..=5.0).text("One-handed spread"));
}
