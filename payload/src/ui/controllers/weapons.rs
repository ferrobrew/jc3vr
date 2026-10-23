//! Weapon handling: the grab volume each hand is in, the wielded weapon, the grabs and what they
//! did, the foregrip, the hand tiers, the recoil, the dual-wield split, the grapple
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
        loadout::slot_label,
        snatch::{self, SnatchConfig},
        weapons::{
            self, WeaponsStatus,
            config::{
                BracedHeftConfig, ForegripConfig, GrenadeConfig, HeftConfig, HeftTuning,
                HoldConfig, HolstersConfig, PouchSide, PouchesConfig, RecoilConfig, RightGrip,
                VolumeSpec,
            },
            empty::{self, EmptyStatus},
            foregrip::{self, ForegripStatus},
            grenade::{self, GrenadeStatus},
            grip_watch::{self, GripWatchStatus},
            heft::{self, HeftStatus},
            hold::{
                self, Cause, HoldStatus,
                act::{self, DockStatus},
                armed::{self, ArmedReadout},
                dock::SnapKind,
                tick::{self, TickStatus},
            },
            recoil::{self, RecoilStatus},
            regions::{self, RegionsStatus, config::RegionsConfig},
            reload::{self, ReloadStatus},
            unarmed_fire::{self, UnarmedFireStatus},
            wheel::config::{SteerMode, WheelConfig},
        },
    },
    ui::controllers::{
        explosives::{ExplosivesReadout, explosives_readout, explosives_settings},
        pouch_models::pouch_model_settings,
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
    hold: HoldStatus,
    regions: RegionsStatus,
    dock: DockStatus,
    armed: [Option<ArmedReadout>; 2],
    tick: TickStatus,
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
            hold: hold::status(),
            regions: regions::status(),
            dock: act::status(),
            armed: armed::status(),
            tick: tick::status(),
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
        }
    });
    hold_readout(ui, &readout.hold, &readout.dock);
    regions_readout(ui, &readout.regions);
    armed_readout(ui, &readout.armed);
    tick_readout(ui, &readout.tick, &readout.empty);
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

fn hold_readout(ui: &mut Ui, h: &HoldStatus, d: &DockStatus) {
    ui.label(format!(
        "Hold: weapon in {} · bones owned {} · waiting {} · handovers {} · writes {} · snaps {}",
        h.holder.map_or("no hand", Hand::label),
        yes_no(h.owned),
        h.pending
            .map_or("nothing".to_owned(), |(hand, cause)| format!(
                "{} for the {} hand",
                cause.label(),
                hand.label()
            )),
        h.handovers,
        h.writes,
        h.snaps
    ));
    ui.label(format!(
        "Direct pickups: {} · left to the game {} · last refusal: {} · weapon came by {}",
        h.pickups,
        h.pickup_fallbacks,
        h.last_pickup_refusal.as_deref().unwrap_or("none"),
        h.origin.map_or("the game's draw", Cause::label)
    ));
    ui.label(format!(
        "Takes: guns in hands near {} · on racks near {} · taken from hands {} · off racks {} · rack copies {} · last refusal: {}",
        h.take.held_targets,
        h.take.rack_targets,
        h.take.steals,
        h.take.rack_takes,
        h.take.rack_copies,
        h.take.last_refusal.as_deref().unwrap_or("none")
    ));
    let sliding = |hand: Hand| d.snapping[hand.index()].map_or("nothing", SnapKind::label);
    ui.label(format!(
        "Docking: sliding left {} right {} · grip holds left {} right {} · draws {} · docks {} · drops {} · last throw {} · last refusal: {}",
        sliding(Hand::Left),
        sliding(Hand::Right),
        yes_no(d.grip_holds[0]),
        yes_no(d.grip_holds[1]),
        d.draws,
        d.docks,
        d.drops,
        d.last_throw_mps
            .map_or("the game's".to_owned(), |v| format!("{v:.1} m/s")),
        d.last_refusal.map_or("none".to_owned(), |r| r.to_string())
    ));
}

fn regions_readout(ui: &mut Ui, r: &RegionsStatus) {
    let hand = |hand: Hand| {
        r.hands[hand.index()].map_or("off the gun".to_owned(), |on| {
            format!("{} ({})", on.region.label(), on.role.label())
        })
    };
    ui.label(format!(
        "Grab anywhere: left {} · right {} · carries {} · braces {} · grips taken {} · left in the brace {} · slides {}",
        hand(Hand::Left),
        hand(Hand::Right),
        r.carries,
        r.braces,
        r.grips_taken,
        r.let_go,
        r.slides
    ));
    match r.shape {
        Some(s) => ui.label(format!(
            "Gun shape: {:.2} m long · foregrip {} · magazine well {} · action {}",
            s.length_m,
            yes_no(s.foregrip),
            yes_no(s.magazine),
            yes_no(s.action)
        )),
        None if r.unread => ui.label("Gun shape: not read for the wielded long gun"),
        None => ui.label("Gun shape: no long gun wielded"),
    };
}

fn armed_readout(ui: &mut Ui, armed: &[Option<ArmedReadout>; 2]) {
    let hand = |hand: Hand| {
        armed[hand.index()].map_or("nothing".to_owned(), |a| {
            format!("{} ({}/{})", slot_label(a.slot), a.magazine.0, a.magazine.1)
        })
    };
    ui.label(format!(
        "Armed beside the wielded weapon: left {} · right {}",
        hand(Hand::Left),
        hand(Hand::Right)
    ));
}

fn tick_readout(ui: &mut Ui, t: &TickStatus, e: &EmptyStatus) {
    ui.label(format!(
        "Per-hand tick: {} · fire left {} right {} · updates left {} right {} · wielded inputs \
         handed over {} · aim modes set {} · hints put back {} · reload requests refused {}",
        if t.per_hand { "on" } else { "off" },
        if t.fire[0] { "pulled" } else { "up" },
        if t.fire[1] { "pulled" } else { "up" },
        t.updates[0],
        t.updates[1],
        t.substituted,
        t.aim_modes,
        t.hints,
        e.requests_refused
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
    ui.label(format!(
        "Hang pump: ({:+.1}, {:+.1}) · pumped {}",
        a.pump.x, a.pump.y, a.pumps
    ));
    let g = crate::grapple::physical::status();
    ui.label(format!(
        "Physical grapple: {} · pull {:.2} m/s, {:.2} m deep · reel {} · tethers {} · retract {} · \
         haptic {:.2}",
        g.phase.label(),
        g.pull.speed_mps,
        g.pull.depth_m,
        g.reel_scale
            .map_or("game's own".to_owned(), |s| format!("{s:.2}x")),
        g.tethers,
        g.retract.map_or("none".to_owned(), |v| format!("{v:.2}")),
        g.haptic
    ));
    ui.label(match last {
        Some(y) => format!(
            "Yanks: {yanks} · last {:.2} m at {:.1} m/s",
            y.distance_m, y.speed_mps
        ),
        None => format!("Yanks: {yanks}"),
    });
    let n = snatch::status();
    ui.label(format!(
        "Snatch: {} · target {} · fired {} · taken {} · failed {} · tosses {}",
        n.phase,
        n.target.map_or_else(
            || n.unoffered.unwrap_or("none").to_owned(),
            |t| format!("{} at {:.1} m", t.kind, t.distance_m)
        ),
        n.fired,
        n.taken,
        n.failed,
        n.tosses
    ));
    if let Some(refusal) = &n.last_refusal {
        ui.label(format!("Snatch: last refusal: {refusal}"));
    }
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
    ui.label(format!(
        "  pin {} · pulled {}{}",
        g.pin.map_or("none", |p| p.label()),
        g.pins_pulled,
        g.last_puller
            .map_or(String::new(), |p| format!(", last by {}", p.label()))
    ));
    ui.label(format!(
        "  live grenades {} · in reach {} · caught {} · thrown on {} · lost in the hand {}{}",
        g.catch.tracked,
        g.catch.in_reach,
        g.catch.caught,
        g.catch.thrown,
        g.catch.lost,
        g.caught_hand.map_or(String::new(), |h| format!(
            " · holding one in the {} hand",
            h.label()
        ))
    ));
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
    ui.label(format!("Weight: scatter widened {} times", h.widened_shots));
    for hand in Hand::BOTH {
        let s = &h.hands[hand.index()];
        let Some(target) = s.target else {
            ui.label(format!("  {} hand: no weapon", hand.label()));
            continue;
        };
        ui.label(format!(
            "  {} hand: {} · unbraced {:.2} · reach {:.2} · sag {:.1}° · sway pitch {:+.2}° yaw {:+.2}° · lag {:.1}° · total {:.1}°",
            hand.label(),
            target.class.label(),
            s.share,
            s.reach,
            s.sag_deg,
            s.sway_deg.x,
            s.sway_deg.y,
            s.lag_deg,
            s.offset_deg
        ));
    }
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

pub(super) fn weapons_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.holsters.enabled, "Sockets on the body")
        .on_hover_text(
            "Draw a weapon with a grip press at the socket it sits on, and dock a held one on a \
             socket that fits it.",
        );
    ui.add_enabled_ui(w.holsters.enabled, |ui| {
        ui.indent("holsters", |ui| holster_settings(ui, &mut w.holsters));
    });
    ui.label("Pouches");
    ui.indent("pouches", |ui| pouch_settings(ui, &mut w.pouches));
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
    ui.checkbox(&mut w.heft.braced.enabled, "Two-handed weight")
        .on_hover_text(
            "Braced on the foregrip, a long gun lags a quick swing a little and settles, and sags \
             a little, by its class; the hands stay on it. Pistols carry none.",
        );
    ui.add_enabled_ui(w.heft.braced.enabled, |ui| {
        ui.indent("braced heft", |ui| {
            braced_heft_settings(ui, &mut w.heft.braced)
        });
    });
    ui.checkbox(&mut w.separate_pistols, "Separate pistols")
        .on_hover_text(
            "Make the game's pistol pair two weapons, each picked up, held, dropped, reloaded, and \
         fired on its own. Needs the VR inventory and its carrying.",
        );
    hold_settings(ui, &mut w.hold);
    regions_settings(ui, &mut w.regions);
    snatch_settings(ui, &mut w.snatch);
}

fn snatch_settings(ui: &mut Ui, s: &mut SnatchConfig) {
    ui.checkbox(&mut s.enabled, "Grapple snatch").on_hover_text(
        "Grip with the empty grapple hand at a weapon out of reach under the grapple's aim, \
             then yank the hand back: the hook pulls the weapon into the hand. The grapple's aim \
             also sees weapons on the ground: a tap tosses one to your feet, a hold tethers it.",
    );
    ui.add_enabled_ui(s.enabled, |ui| {
        ui.indent("snatch", |ui| {
            ui.add(egui::Slider::new(&mut s.range_m, 2.0..=60.0).text("Range (m)"));
            ui.add(
                egui::Slider::new(&mut s.cone_deg, 1.0..=20.0).text("Cone for guns in hands (°)"),
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_speed_mps, 0.5..=5.0).text("Yank speed (m/s)"),
            );
            ui.add(
                egui::Slider::new(&mut s.yank.min_distance_m, 0.05..=0.4).text("Yank length (m)"),
            );
            ui.add(egui::Slider::new(&mut s.yank.window_s, 0.05..=0.6).text("Yank window (s)"));
            ui.add(egui::Slider::new(&mut s.max_speed_mps, 2.0..=40.0).text("Fastest pull (m/s)"));
            ui.add(
                egui::Slider::new(&mut s.max_accel_mps2, 5.0..=300.0).text("Hardest pull (m/s²)"),
            );
            ui.add(
                egui::Slider::new(&mut s.stiffness_per_s, 0.5..=15.0).text("Pull stiffness (1/s)"),
            );
            ui.add(egui::Slider::new(&mut s.arc, 0.0..=0.8).text("Pull arc"));
            ui.add(
                egui::Slider::new(&mut s.capture_radius_m, 0.1..=1.0).text("Capture radius (m)"),
            );
            ui.add(
                egui::Slider::new(&mut s.stall_distance_m, 0.01..=0.5).text("Stall distance (m)"),
            );
            ui.add(egui::Slider::new(&mut s.stall_window_s, 0.1..=2.0).text("Stall window (s)"));
            ui.add(
                egui::Slider::new(&mut s.attach_timeout_s, 0.5..=6.0).text("Hook flight limit (s)"),
            );
            ui.checkbox(&mut s.holstered, "Guns holstered on characters")
                .on_hover_text(
                    "A character drops the gun on its body and staggers. Needs the steal.",
                );
            ui.checkbox(&mut s.props, "Loose objects")
                .on_hover_text("An object one hand can carry comes into the hand's hold.");
            ui.checkbox(&mut s.doors, "Car doors").on_hover_text(
                "The line pulls a closed door of a still vehicle open at its handle.",
            );
            ui.add_enabled_ui(s.doors, |ui| {
                ui.add(
                    egui::Slider::new(&mut s.door_reach_m, 0.1..=1.5).text("Door handle reach (m)"),
                );
                ui.add(
                    egui::Slider::new(&mut s.door_max_vehicle_speed_mps, 0.0..=10.0)
                        .text("Fastest vehicle (m/s)"),
                );
                ui.add(
                    egui::Slider::new(&mut s.door_swing_mps, 0.0..=10.0).text("Door swing (m/s)"),
                );
            });
        });
    });
}

fn regions_settings(ui: &mut Ui, r: &mut RegionsConfig) {
    ui.checkbox(&mut r.enabled, "Grab long guns anywhere")
        .on_hover_text(
            "A hand takes a long gun where it closes on it: the grip fires it, the handguard or \
             barrel braces it, and a lone hand anywhere but the grip carries it unable to fire.",
        );
    ui.add_enabled_ui(r.enabled, |ui| {
        ui.add(egui::Slider::new(&mut r.reach_m, 0.0..=0.15).text("Reach beyond the gun (m)"));
        ui.add(egui::Slider::new(&mut r.grip_radius_m, 0.02..=0.15).text("Grip radius (m)"));
        ui.add(egui::Slider::new(&mut r.rebrace_s, 0.0..=3.0).text("Brace waits for (s)"));
        ui.add(egui::Slider::new(&mut r.draw_wait_s, 0.0..=6.0).text("Draw waits for (s)"));
    });
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
             Off, weapons come off the body's sockets.",
        );
    ui.checkbox(&mut w.wheel.enabled, "Weapon wheel")
        .on_hover_text(
            "Open the game's selection cross with a held input and choose an arm with the hand.",
        );
    ui.add_enabled_ui(w.wheel.enabled, |ui| {
        ui.indent("wheel", |ui| wheel_settings(ui, &mut w.wheel));
    });
}

fn wheel_settings(ui: &mut Ui, w: &mut WheelConfig) {
    ui.checkbox(
        &mut w.trackpad,
        "Open with the weapon-select trackpad press",
    );
    ui.checkbox(&mut w.stick, "Open with the weapon wheel stick push");
    ui.add_enabled_ui(w.stick, |ui| {
        ui.add(egui::Slider::new(&mut w.stick_open, 0.0..=1.0).text("Stick opens at"));
        ui.add(egui::Slider::new(&mut w.stick_release, 0.0..=1.0).text("Stick closes below"));
    });
    ui.horizontal(|ui| {
        ui.label("Steer by");
        ui.selectable_value(&mut w.steer, SteerMode::Point, "Pointing the hand");
        ui.selectable_value(&mut w.steer, SteerMode::Offset, "Moving the hand");
    });
    match w.steer {
        SteerMode::Point => {
            ui.add(
                egui::Slider::new(&mut w.point_leave_deg, 1.0..=45.0)
                    .text("Leave the centre at (°)"),
            );
            ui.add(
                egui::Slider::new(&mut w.point_return_deg, 0.0..=45.0)
                    .text("Return to the centre at (°)"),
            );
        }
        SteerMode::Offset => {
            ui.add(
                egui::Slider::new(&mut w.offset_leave_m, 0.005..=0.2)
                    .text("Leave the centre at (m)"),
            );
            ui.add(
                egui::Slider::new(&mut w.offset_return_m, 0.0..=0.2)
                    .text("Return to the centre at (m)"),
            );
        }
    }
    ui.add(egui::Slider::new(&mut w.hysteresis_deg, 0.0..=44.0).text("Arm hysteresis (°)"));
    ui.checkbox(&mut w.place_at_hand, "Place the cross where the hand was");
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
    ui.add(
        egui::Slider::new(&mut g.warning_interval_s[0], 0.05..=1.0).text("Slowest pulse gap (s)"),
    );
    ui.add(
        egui::Slider::new(&mut g.warning_interval_s[1], 0.02..=1.0).text("Fastest pulse gap (s)"),
    );
    ui.checkbox(&mut g.pin.enabled, "Pull the pin to start the fuse")
        .on_hover_text(
            "A grenade from the pouch comes out with its fuse held until the other hand grabs its \
             pin or its own trigger pulls it; then it cooks in the hand.",
        );
    ui.add_enabled_ui(g.pin.enabled, |ui| {
        ui.add(egui::Slider::new(&mut g.pin.radius_m, 0.02..=0.2).text("Pin radius (m)"));
        ui.checkbox(
            &mut g.pin.trigger,
            "The holding hand's trigger pulls the pin",
        );
    });
    ui.checkbox(&mut g.catch.enabled, "Catch live grenades")
        .on_hover_text("A grip press at a live grenade near the player catches it; opening the grip throws it.");
    ui.add_enabled_ui(g.catch.enabled, |ui| {
        ui.add(egui::Slider::new(&mut g.catch.radius_m, 0.05..=0.5).text("Catch radius (m)"));
        ui.add(egui::Slider::new(&mut g.catch.range_m, 1.0..=10.0).text("Catch range (m)"));
        ui.add(
            egui::Slider::new(&mut g.catch.own_grace_s, 0.0..=2.0).text("Own throw's grace (s)"),
        );
        ui.add(
            egui::Slider::new(&mut g.catch.max_carry_mps, 1.0..=60.0).text("Fastest carry (m/s)"),
        );
    });
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

fn hold_settings(ui: &mut Ui, h: &mut HoldConfig) {
    ui.checkbox(&mut h.either_hand, "Hold weapons in either hand")
        .on_hover_text(
            "A weapon rides the hand that drew it, took it off the ground, or was handed it; only \
             that hand's trigger fires it.",
        );
    ui.add_enabled_ui(h.either_hand, |ui| {
        ui.checkbox(&mut h.handover, "Hand the gun over at its grip")
            .on_hover_text("A grip press by the free hand at the held gun's grip takes the gun.");
        ui.add(egui::Slider::new(&mut h.handle_radius_m, 0.02..=0.2).text("Gun grip radius (m)"));
    });
    ui.add_enabled_ui(h.either_hand, |ui| {
        ui.add(egui::Slider::new(&mut h.snap_s, 0.0..=0.5).text("Slide time (s)"))
            .on_hover_text("How long a gun slides between a socket and the hand.");
        ui.checkbox(&mut h.grip_to_hold, "Hold the grip to hold a gun")
            .on_hover_text(
                "Opening the grip on a gun docks it on a socket under the hand that takes it, and \
                 lets it fall anywhere else.",
            );
        ui.add(egui::Slider::new(&mut h.max_throw_mps, 0.0..=30.0).text("Fastest throw (m/s)"));
    });
    ui.add_enabled(
        h.either_hand,
        egui::Checkbox::new(&mut h.second_weapon, "Hold a weapon in each hand"),
    )
    .on_hover_text(
        "Either hand draws any weapon off the body while the other holds one, and fires it with \
         its own trigger. Off, a hand draws only while the other holds no weapon.",
    );
    ui.add_enabled(
        h.either_hand,
        egui::Checkbox::new(&mut h.per_hand_tick, "Fire each gun from its own hand"),
    )
    .on_hover_text(
        "Every gun in a hand, the wielded one too, fires from that hand's trigger, and one gun's \
         ammunition check never reloads the other's. Off, the wielded gun fires from the game's \
         own fire input.",
    );
    ui.horizontal(|ui| {
        ui.label("Right hand gun grip:");
        for grip in RightGrip::ALL {
            ui.selectable_value(&mut h.right_grip, grip, grip.label());
        }
    })
    .response
    .on_hover_text(
        "The grip a gun held in the right hand takes on the wrist: the right hand's own bind-pose \
         grip, the left hand's mirrored, or the right's unless it is turned far from the mirror.",
    );
    ui.checkbox(
        &mut h.magazine_pips,
        "Show the magazine of the gun beside the wielded one over its hand",
    )
    .on_hover_text(
        "Pips over the hand holding a gun the game does not wield count its rounds; the HUD \
         shows the wielded one's.",
    );
    ui.checkbox(
        &mut h.drop_replaced,
        "A pickup lets the replaced weapon fall",
    )
    .on_hover_text("The weapon a direct pickup replaces in its slot falls to the ground.");
    ui.checkbox(
        &mut h.direct_pickup,
        "Pick weapons up straight into the hand",
    )
    .on_hover_text(
        "A grab of a weapon on the ground takes that weapon at once, with no pickup \
             animation; off, the grab presses the game's pickup action.",
    );
    ui.add_enabled_ui(h.direct_pickup, |ui| {
        ui.checkbox(&mut h.steal, "Take guns out of characters' hands")
            .on_hover_text("A grab of a gun a character holds takes it; the character staggers.");
        ui.checkbox(&mut h.racks, "Take weapons off racks")
            .on_hover_text(
                "A grab of a weapon on a rack takes it, or the copy the rack hands out.",
            );
        ui.checkbox(&mut h.take_slide, "Slide taken guns into the hand")
            .on_hover_text("A gun taken from the world slides from where it was into the hand.");
        ui.add(
            egui::Slider::new(&mut h.take_radius_m, 0.03..=0.25).text("Take reach along a gun (m)"),
        );
    });
}

fn holster_settings(ui: &mut Ui, h: &mut HolstersConfig) {
    ui.checkbox(&mut h.follow_docks, "Sockets sit on their weapons")
        .on_hover_text(
            "Each socket is reached where its weapon is seen on the body, or where a held weapon \
             would sit on it. Off, the hips, the shoulders, and the chest are reached at the \
             landmarks below.",
        );
    ui.checkbox(
        &mut h.long_guns_dominant,
        "Only the dominant hand draws a long gun with both hands empty",
    );
    ui.checkbox(&mut h.stick_click_cycles, "Move stick click cycles weapons")
        .on_hover_text("The fallback for drawing without reaching a socket.");
    ui.label("Offsets from the landmark, right side (+X right, +Y up, +Z back)");
    volume_settings(ui, "Hip", &mut h.hip);
    volume_settings(ui, "Shoulder", &mut h.shoulder);
    volume_settings(ui, "Chest", &mut h.chest);
    if ui.button("Reset sockets").clicked() {
        *h = HolstersConfig {
            enabled: h.enabled,
            ..HolstersConfig::new()
        };
    }
}

fn pouch_settings(ui: &mut Ui, p: &mut PouchesConfig) {
    ui.horizontal(|ui| {
        ui.label("Grenade pouch on the");
        for side in PouchSide::ALL {
            ui.selectable_value(&mut p.grenade_side, side, side.label());
        }
    })
    .response
    .on_hover_text("The C4 pouch takes the other side of the chest.");
    ui.add(egui::Slider::new(&mut p.reach_margin_m, 0.0..=0.15).text("Reach margin (m)"));
    ui.label("Offsets from the body's surface, right side (+X right, +Y up, +Z back)");
    volume_settings(ui, "Grenade pouch", &mut p.grenade);
    volume_settings(ui, "C4 pouch", &mut p.c4);
    volume_settings(ui, "Magazine pouch", &mut p.magazine);
    pouch_model_settings(ui, &mut p.models);
    if ui.button("Reset pouches").clicked() {
        *p = PouchesConfig {
            grenade_side: p.grenade_side,
            ..PouchesConfig::new()
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
    ui.checkbox(&mut f.hold, "Hold the grip to hold the foregrip");
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
            braced: h.braced,
            ..HeftConfig::new()
        };
    }
}

fn braced_heft_settings(ui: &mut Ui, b: &mut BracedHeftConfig) {
    ui.add(egui::Slider::new(&mut b.strength, 0.0..=2.0).text("Strength"));
    ui.add(egui::Slider::new(&mut b.max_angle_deg, 0.0..=20.0).text("Largest angle (°)"));
    for (label, tuning) in [
        ("SMGs", &mut b.smg),
        ("Rifles", &mut b.rifle),
        ("Heavy", &mut b.heavy),
    ] {
        ui.collapsing(label, |ui| {
            ui.add(egui::Slider::new(&mut tuning.sag_deg, 0.0..=10.0).text("Sag (°)"));
            ui.add(
                egui::Slider::new(&mut tuning.stiffness, 0.0..=20_000.0)
                    .logarithmic(true)
                    .text("Stiffness (0 follows rigidly)"),
            );
        });
    }
    if ui.button("Reset the two-handed weight").clicked() {
        *b = BracedHeftConfig {
            enabled: b.enabled,
            strength: b.strength,
            ..BracedHeftConfig::new()
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
