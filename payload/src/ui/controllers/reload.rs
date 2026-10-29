//! The true reload: each hand's reloaded weapon, its magazine and chamber, the round in hand, the
//! belt pushes, the action's work, the counters, and the reload's switches.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    weapons::reload::{
        ReloadStatus, WeaponReadout,
        belt::PushStage,
        chamber::{ActionKind, Chamber, WorkStage},
        config::ReloadConfig,
        insert::InsertStage,
        loose, wells,
    },
};

pub(super) fn reload_readout(ui: &mut Ui, r: &ReloadStatus) {
    ui.label(format!("Native magazine execution: {:?}", r.serial));
    let c = &r.counts;
    ui.label(format!(
        "True reload: {} · ejects {} · seats {} · belt reloads {} · actions worked {} · slides \
         locked back {} · dry fires {} · empty pouch {} · game reloads stopped {}",
        if r.active { "acting" } else { "inactive" },
        c.ejects,
        c.seats,
        c.belt_reloads,
        c.racks,
        c.lock_backs,
        c.dry_fires,
        c.empty_pouch,
        c.refused
    ));
    let l = loose::counts();
    ui.label(format!(
        "  magazines in the world {} · fitting in reach {} · dropped {} · picked up {} · seated \
         {}{}",
        l.in_world,
        l.in_reach,
        l.dropped,
        l.picked_up,
        l.seated,
        l.last_rounds
            .map_or(String::new(), |n| format!(" (last held {n})"))
    ));
    if r.weapons.iter().all(Option::is_none) {
        return;
    }
    ui.indent("reload", |ui| {
        for hand in Hand::BOTH {
            let i = hand.index();
            let Some(w) = &r.weapons[i] else {
                continue;
            };
            // The dual-wield pair's weapon is in both hands, and is shown once.
            let first = Hand::BOTH
                .into_iter()
                .find(|h| r.weapons[h.index()].is_some_and(|o| o.address == w.address));
            if first == Some(hand) {
                weapon_lines(ui, w);
            }
            ui.label(format!(
                "{} gun: chamber {} · belt {} · fire {} · turn {:.0}°/s",
                hand.label(),
                match w.chamber {
                    Chamber::Chambered => "loaded",
                    Chamber::Empty => "empty",
                },
                match r.belt[i] {
                    PushStage::Outside => "clear",
                    PushStage::Pushing => "pushing",
                    PushStage::Spent => "in the pouch",
                },
                if r.fire_held[i] { "held" } else { "free" },
                r.flick_dps[i]
            ));
        }
        let held = match r.insertion.stage {
            InsertStage::Idle => "nothing in hand".to_owned(),
            InsertStage::Holding { elapsed_s } => format!("holding for {elapsed_s:.1} s"),
            InsertStage::Captured { depth_m, .. } => format!("caught, {depth_m:.3} m in"),
        };
        let hands = r.insert_hands.map_or(String::new(), |(hand, gun)| {
            format!(" ({} hand for the {} gun)", hand.label(), gun.label())
        });
        ui.label(format!(
            "round: {held}{hands}{} · taken {} · seated {} · dropped {}",
            r.insertion
                .distance_m
                .map_or(String::new(), |d| format!(", {d:.2} m from the mouth")),
            r.insertion.taken,
            r.insertion.seated,
            r.insertion.dropped
        ));
        for hand in Hand::BOTH {
            let Some((w, t)) = r.weapons[hand.index()].and_then(|w| Some((w, w.action?))) else {
                continue;
            };
            ui.label(format!(
                "{} action: {:.0}% back{}{}{}{}",
                hand.label(),
                t.position * 100.0,
                if w.plan.action == ActionKind::Bolt {
                    format!(", {:.0}% up", t.lift * 100.0)
                } else {
                    String::new()
                },
                if t.grip.is_some() { " · held" } else { "" },
                if t.locked_back { " · locked back" } else { "" },
                if t.pulled { " · racked" } else { "" },
            ));
        }
        ui.label(match r.work.stage {
            WorkStage::Idle => format!("action: idle · worked {}", r.work.worked),
            WorkStage::Held { pulled, .. } => format!(
                "action: held, {:.0}% back{} · worked {}",
                r.work.progress * 100.0,
                if pulled { ", pulled" } else { "" },
                r.work.worked
            ),
        });
        for event in &r.events {
            ui.label(format!("  {event}"));
        }
    });
}

/// The weapon's plan and its magazine.
fn weapon_lines(ui: &mut Ui, w: &WeaponReadout) {
    ui.label(format!(
        "weapon {:#010x} · {} · action {} · well {} · travel {:.3} m",
        w.name_hash,
        w.plan.load.label(),
        w.plan.action.label(),
        wells::lookup(w.name_hash).map_or("at the wrist".to_owned(), |e| format!(
            "{} ({}, graded {})",
            e.weapon,
            e.kind.label(),
            e.grade.label()
        )),
        w.plan.travel_m
    ));
    ui.label(format!(
        "magazine {}/{}{} · reserve {}",
        w.magazine,
        w.capacity,
        if w.out { " (out)" } else { "" },
        if w.unlimited {
            "unlimited".to_owned()
        } else {
            w.reserve.to_string()
        }
    ));
}

pub(super) fn reload_settings(ui: &mut Ui, r: &mut ReloadConfig) {
    ui.checkbox(&mut r.flick_eject, "Flick the gun to eject")
        .on_hover_text("A sharp flick of the hand holding a gun ejects its magazine.");
    ui.checkbox(&mut r.loose.enabled, "Magazines stay in the world")
        .on_hover_text(
            "An ejected magazine keeps its rounds, and a magazine on the ground that fits the gun \
             can be picked up and seated with them.",
        );
    ui.checkbox(&mut r.belt.enabled, "Belt reload")
        .on_hover_text("Push the gun's well down onto the belt pouch to reload it one-handed.");
    ui.checkbox(&mut r.action.physical, "Physical action").on_hover_text(
        "The slide, charging handle, bolt, or pump follows the hand through its travel. Off, the \
         action is a pull-and-release gesture.",
    );
}
