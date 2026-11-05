//! Weapon handling: the grab volume each hand is in, the wielded weapon, the grabs and what they
//! did, the foregrip, the hand tiers, the recoil, the dual-wield split, the grapple line and yank,
//! the grenade throw, the C4 throw and the wrist detonator, the true reload, the empty weapon, the
//! one-handed weight, the gun-in-hand watch, the fire input with empty hands, per-hand aim assist,
//! and the switches of the hold, the support, the reload, and the explosives.

use egui::Ui;

use crate::{
    controllers::{
        aim::{self as controller_aim, AimStatus},
        bindings::{self, yank::Yank},
        config::{ControllersConfig, WeaponsConfig},
        frame::Hand,
        hands::{
            anchor::{self, AnchorStatus},
            tiers::{self, TierState, TierStatus},
        },
        loadout::slot_label,
        snatch,
        weapons::{
            self, WeaponsStatus,
            empty::{self, EmptyStatus},
            foregrip::{self, ForegripStatus},
            grenade::{self, GrenadeStatus},
            grip_watch::{self, GripWatchStatus},
            heft::{self, HeftStatus},
            hold::{
                self, Cause, HoldStatus,
                act::{self, DockStatus},
                armed::{self, ArmedReadout},
                button_pickup::{self, ButtonPickupStatus},
                dock::SnapKind,
                tick::{self, TickStatus},
            },
            recoil::{self, RecoilStatus},
            regions::{self, RegionsStatus},
            reload::{self, ReloadStatus},
            unarmed_fire::{self, UnarmedFireStatus},
        },
    },
    status::{explosives as explosives_status, grapple as grapple_status},
    ui::{
        controllers::{
            explosives::{ExplosivesReadout, explosives_readout, explosives_settings},
            reload::{reload_readout, reload_settings},
            vec3, yes_no,
        },
        status_row::headlines,
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
    button_pickup: ButtonPickupStatus,
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
            button_pickup: button_pickup::status(),
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
    grip_watch_readout(ui, &readout.grip_watch);
    ui.label(format!(
        "Fire with empty hands: draws held back {}",
        readout.unarmed_fire.held_back
    ));
    ui.label(format!(
        "Button pickup: held back {} · hand fallbacks passed {} · panel hidden frames {}",
        readout.button_pickup.held_back,
        readout.button_pickup.granted,
        readout.button_pickup.panel_hidden
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

pub(super) fn feature_readout(
    ui: &mut Ui,
    readout: &WeaponsReadout,
    id: crate::ui::catalog::FeatureId,
) {
    match id {
        crate::ui::catalog::FeatureId::Grapple => {
            grapple_readout(ui, &readout.anchor, readout.yanks)
        }
        crate::ui::catalog::FeatureId::Explosives => {
            headlines(
                ui,
                &[
                    &explosives_status::LIVE_GRENADES,
                    &explosives_status::C4_BY_HAND,
                ],
            );
            grenade_readout(ui, &readout.grenade);
            explosives_readout(ui, &readout.explosives);
        }
        crate::ui::catalog::FeatureId::Reload => {
            reload_readout(ui, &readout.reload);
            empty_readout(ui, &readout.empty);
        }
        crate::ui::catalog::FeatureId::Support => heft_readout(ui, &readout.heft),
        _ => {}
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
        "Direct pickups: {} · left to the game {} · weapon came by {}",
        h.pickups,
        h.pickup_fallbacks,
        h.origin.map_or("the game's draw", Cause::label)
    ));
    ui.label(format!(
        "Takes: guns in hands near {} · on racks near {} · taken from hands {} · off racks {} · rack copies {}",
        h.take.held_targets,
        h.take.rack_targets,
        h.take.steals,
        h.take.rack_takes,
        h.take.rack_copies
    ));
    let sliding = |hand: Hand| d.snapping[hand.index()].map_or("nothing", SnapKind::label);
    ui.label(format!(
        "Docking: sliding left {} right {} · grip holds left {} right {} · draws {} · docks {} · drops {} · last setter request {}",
        sliding(Hand::Left),
        sliding(Hand::Right),
        yes_no(d.grip_holds[0]),
        yes_no(d.grip_holds[1]),
        d.draws,
        d.docks,
        d.drops,
        d.last_throw_mps
            .map_or("the game's".to_owned(), |v| format!("{v:.1} m/s"))
    ));
    ui.label(format!(
        "Last refusal of a pickup, take, draw, dock, or drop: {}",
        h.last_refusal.as_deref().unwrap_or("none")
    ));
    ui.collapsing("Latest gun release evidence", |ui| {
        ui.label(format!("Release attempts: {}.", d.releases.attempts));
        if let Some(record) = d.releases.latest {
            ui.label(format!(
                "Attempt {} · physical {} hand · route {:?} · weapon {:?} · refusal {:?}",
                record.sequence,
                record.hand.label(),
                record.route,
                record.weapon,
                record.refusal,
            ));
            ui.label(format!("Returned stages: {:?}.", record.stages));
            ui.label(format!("Cached gun sample and setter request: {:?}.", record.drop));
            if let Some(motion) = record.drop.and_then(|drop| drop.cached_world_motion) {
                ui.label(format!(
                    "Cached sample finite: linear {} · angular {}.",
                    yes_no(motion.linear_is_finite()),
                    yes_no(motion.angular_is_finite()),
                ));
            }
            ui.label(format!(
                "Separate cockpit controller context: {:?}.",
                record.controller
            ));
            ui.label(format!(
                "Pre-disarm sample: {:?}. Native readbacks: {:?}.",
                record.before_disarm_sample, record.native_readback
            ));
            ui.label("Cached gun sample age is unknown. Setter completion does not prove applied velocity or next-step persistence.");
        } else {
            ui.label("No inventory-gun release attempt is recorded.");
        }
    });
}

fn regions_readout(ui: &mut Ui, r: &RegionsStatus) {
    let hand = |hand: Hand| {
        r.hands[hand.index()].map_or("off the gun".to_owned(), |on| {
            format!("{} ({})", on.region.label(), on.role.label())
        })
    };
    ui.label(format!(
        "Grab anywhere: left {} · right {} · braces {} · passed to the brace {} · slides {}",
        hand(Hand::Left),
        hand(Hand::Right),
        r.braces,
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
    headlines(
        ui,
        &[
            &grapple_status::STATE,
            &grapple_status::TARGET,
            &grapple_status::REEL,
            &grapple_status::TETHERS,
            &grapple_status::LOCK,
            &grapple_status::SWING,
            &grapple_status::SNATCH,
            &grapple_status::HAPTICS,
        ],
    );
    ui.label(format!(
        "Grapple line: live {} · attached at {}",
        yes_no(a.wire),
        a.anchor.map_or("nothing".to_owned(), vec3)
    ));
    ui.label(match a.displacement {
        Some(d) => format!(
            "Hang: hand moved ({:+.2}, {:+.2}) m · impulse ({:+.1}, {:+.1})",
            d.x, d.y, a.impulse.x, a.impulse.y
        ),
        None => "Hang: not swinging".to_owned(),
    });
    ui.label(format!("Hang pump: ({:+.1}, {:+.1})", a.pump.x, a.pump.y));
    let pull = crate::grapple::physical::status().pull;
    ui.label(format!(
        "Hand pull: {:.2} m/s, {:.2} m deep",
        pull.speed_mps, pull.depth_m
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
        "Snatches failed: {} · tosses {}",
        n.failed, n.tosses
    ));
    if let Some(mass) = n.aimed_mass_kg {
        ui.label(format!("  Object under the aim: {mass:.0} kg"));
    }
    if let Some(swing) = n.swing {
        ui.label(if swing.tethered {
            "  Holding the line: release grip to throw"
        } else {
            "  Holding the line: release grip to throw · release B to snatch"
        });
    }
}

fn grenade_readout(ui: &mut Ui, g: &GrenadeStatus) {
    ui.label(format!(
        "Grenade · primed {} · thrown {} · gone before the throw {}{}",
        g.primed,
        g.thrown,
        g.gone,
        if g.weapon_seen {
            ""
        } else {
            " · the grenade weapon has not updated yet"
        }
    ));
    ui.label(format!(
        "  stock {} · put back into the pouch {} · refused by a full pouch {}",
        g.stock.map_or("unknown".to_owned(), |stock| {
            let capacity = stock
                .capacity
                .map_or("no cap".to_owned(), |cap| format!("of {cap}"));
            let unlimited = if stock.unlimited { ", unlimited" } else { "" };
            format!("{} {capacity}{unlimited}", stock.count())
        }),
        g.returned,
        g.returns_refused
    ));
    if let Some(f) = g.fuse {
        ui.label(format!(
            "  fuse {:.1} s of {:.1} s",
            f.remaining_s.max(0.0),
            f.total_s
        ));
    }
    ui.label(format!("  live grenades lost in the hand {}", g.catch.lost));
    if let Some(v) = g.last_release {
        ui.label(format!(
            "  last release {} ({:.1} m/s)",
            vec3(v),
            v.length()
        ));
    }
    for event in g.events.iter().rev() {
        ui.label(format!(
            "  {}{:.1} m/s",
            event
                .hand
                .map_or(String::new(), |h| format!("{}: ", h.label())),
            event.release.velocity.length()
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

pub(super) fn hold_feature_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.hold.either_hand, "Hold weapons in either hand")
        .on_hover_text(
            "A weapon rides the hand that drew it, took it, or was handed it, and only that hand's \
             trigger fires it. Either hand draws while the other holds a weapon, the free hand \
             takes the held gun at its grip, and a grab takes guns from characters and racks.",
        );
    ui.checkbox(&mut w.hold.dock_attraction, "Attract weapons to sockets")
        .on_hover_text(
            "A weapon let go of near a free socket that takes it travels into the socket: over \
             the shoulder for the back, beside the thigh for a hip. In front of the body, or \
             thrown, it falls.",
        );
    ui.checkbox(&mut w.hold.hand_pickup_only, "Pick weapons up only by hand")
        .on_hover_text(
            "Hides the game's weapon pickup panel and its button prompt, and the use button no \
             longer picks a weapon up. A hand's grab picks weapons up. The gamepad scheme keeps \
             the game's own prompt and button.",
        );
    ui.checkbox(&mut w.regions.enabled, "Grab long guns anywhere")
        .on_hover_text(
            "A second hand braces a long gun wherever it closes on it. A lone hand takes a gun \
             by its grip wherever it touches it, and a bracing hand left alone slides onto the grip.",
        );
}

pub(super) fn support_settings(ui: &mut Ui, c: &mut ControllersConfig) {
    let w = &mut c.weapons;
    ui.checkbox(&mut w.foregrip.enabled, "Foregrip")
        .on_hover_text("Let the off-hand grab a two-handed weapon's foregrip.");
    ui.checkbox(&mut w.recoil.enabled, "Recoil")
        .on_hover_text("Kick the gun hand away from the controller on each shot.");
    ui.checkbox(&mut w.heft.enabled, "One-handed weight")
        .on_hover_text(
            "Held in one hand, a long gun sags, trails the hand, sways, kicks harder, and shoots \
             wider, by its class. Bracing it on the foregrip takes it all away.",
        );
    ui.checkbox(&mut w.heft.braced.enabled, "Two-handed weight")
        .on_hover_text(
            "Braced on the foregrip, a long gun lags a quick swing a little and settles, and sags \
             a little, by its class; the hands stay on it. Pistols carry none.",
        );
    ui.checkbox(&mut w.separate_pistols, "Separate pistols")
        .on_hover_text(
            "Make the game's pistol pair two weapons, each picked up, held, dropped, reloaded, and \
             fired on its own. Needs the VR inventory and its carrying.",
        );
    let brace = &mut c.physical_hands.bodies.brace;
    ui.label("Brace joint rebuild").on_hover_text(
        "How far the bracing palm's frame on a held gun may move before its joint is remade, \
         and how long a joint stands at least.",
    );
    ui.add(egui::Slider::new(&mut brace.distance_m, 0.0..=0.05).text("Moved past (m)"));
    ui.add(
        egui::Slider::new(&mut brace.angle_rad, 0.0..=0.3)
            .text("Turned past (rad)")
            .fixed_decimals(3),
    );
    ui.add(egui::Slider::new(&mut brace.min_interval_s, 0.0..=1.0).text("Stands at least (s)"));
    ui.add(egui::Slider::new(&mut brace.break_distance_m, 0.05..=0.5).text("Breaks past (m)"))
        .on_hover_text(
            "How far the tracked support hand may be from its place on the gun before the brace \
             breaks and the two-handed hold lets go.",
        );
}

pub(super) fn reload_feature_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    reload_settings(ui, &mut w.reload);
}

/// The switches of the grenade throw and catch, the C4 throw and press, and the wrist detonator.
pub(super) fn explosive_feature_settings(ui: &mut Ui, w: &mut WeaponsConfig) {
    ui.checkbox(&mut w.grenade_throw, "Physical grenade throw").on_hover_text(
        "A grip press at the grenade pouch primes a grenade; opening the grip throws it. The fuse \
         runs from the prime.",
    );
    ui.checkbox(&mut w.grenade.catch.enabled, "Catch live grenades")
        .on_hover_text(
            "A grip press at a live grenade near the player catches it; opening the grip throws it.",
        );
    explosives_settings(ui, w);
}
