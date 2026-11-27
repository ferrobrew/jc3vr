//! The true reload: the action and magazine rows, each hand's reloaded weapon, its magazine and
//! chamber, the round in hand, the belt pushes, the action's work, and the counters.

use egui::Ui;
use jc3vr_input::frame::Hand;
use jc3vr_weapons::controllers::weapons::reload::{
    ReloadStatus, WeaponReadout, belt::PushStage, chamber::Chamber, fire_hold::FireHold,
    insert::InsertStage, loose, wells,
};

use crate::{status::explosives::MAGAZINES, ui::status_row::headlines};

pub(super) fn reload_readout(ui: &mut Ui, r: &ReloadStatus) {
    headlines(ui, &[&MAGAZINES]);
    ui.label(format!("Native magazine execution: {:?}", r.serial));
    let c = &r.counts;
    ui.label(format!(
        "True reload: {} · ejects {} · knocks {} · seats {} · belt reloads {} · slides locked back \
         {} · empty pouch {} · game reloads stopped {}",
        if r.active { "acting" } else { "inactive" },
        c.ejects,
        c.knocks,
        c.seats,
        c.belt_reloads,
        c.lock_backs,
        c.empty_pouch,
        c.refused
    ));
    let l = loose::counts();
    ui.label(format!(
        "  magazines fitting in reach {} · dropped {}{}",
        l.in_reach,
        l.dropped,
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
                "{} gun: chamber {} · belt {} · fire {}",
                hand.label(),
                match w.chamber {
                    Chamber::Chambered => "loaded",
                    Chamber::Empty => "empty",
                },
                match r.belt[i] {
                    PushStage::Outside => "clear",
                    PushStage::Pushing => "pushing",
                    PushStage::Reloaded => "reloaded in the pouch",
                    PushStage::Brushed => "brushed the pouch",
                },
                r.fire_held[i].map_or("free", FireHold::label),
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
            if r.weapons[hand.index()]
                .and_then(|w| w.action)
                .is_some_and(|t| t.pulled)
            {
                ui.label(format!("{} action: racked", hand.label()));
            }
        }
        for event in r.events.iter() {
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
