//! Hand bodies: each hand body's phase and drive, the push they give the player, and the held
//! guns' bodies.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    physical_hands::physics::{BodyPhase, HandBodiesStatus, guns},
};

pub(super) fn hand_bodies_readout(ui: &mut Ui, status: &HandBodiesStatus) {
    ui.label(format!(
        "Callback {} · {} steps · player group {} · push ({:+.2}, {:+.2}) m/s",
        if status.registered {
            "registered"
        } else {
            "not registered"
        },
        status.steps,
        status
            .system_group
            .map_or_else(|| "none".to_owned(), |g| g.to_string()),
        status.push.x,
        status.push.y
    ));
    ui.label(format!(
        "{} created · {} destroyed · {} contacts ({} dropped) · {} blows",
        status.created, status.destroyed, status.contacts, status.dropped_contacts, status.blows
    ));
    if let Some(blow) = status.last_blow {
        ui.label(format!(
            "Last blow: {} hand · {:.1} N·s · {} · {:.0}% of a knockdown · {}",
            blow.hand.label(),
            blow.impulse_ns,
            if blow.hostile {
                "hostile"
            } else {
                "not hostile"
            },
            blow.strength * 100.0,
            match blow.damage {
                Some(damage) => format!("{damage} damage, {} dealt", blow.dealt),
                None => "no damage".to_owned(),
            }
        ));
    }
    ui.columns(2, |columns| {
        for hand in Hand::BOTH {
            let ui = &mut columns[hand.index()];
            let s = &status.hands[hand.index()];
            ui.strong(hand.label());
            ui.label(match (s.body_id, s.filter_info) {
                (Some(id), Some(filter)) => {
                    format!("{} · body {id} · filter {filter:#010x}", s.phase.label())
                }
                (Some(id), None) => format!("{} · body {id}", s.phase.label()),
                _ => s.phase.label().to_owned(),
            });
            if s.phase == BodyPhase::Acting {
                ui.label(format!(
                    "lag {:.0} mm · {:.2} m/s · pull {:.0}% · twist {:.0}%",
                    s.lag_m * 1000.0,
                    s.speed_mps,
                    s.force_share * 100.0,
                    s.torque_share * 100.0
                ));
            }
            ui.label(match s.joint {
                Some(constraint) => {
                    format!("{} resets · joined (constraint {constraint})", s.resets)
                }
                None => format!("{} resets", s.resets),
            });
        }
    });
    if let Some(error) = &status.last_error {
        ui.label(format!("Last error: {error}"));
    }
    guns_readout(ui, &guns::status());
}

fn guns_readout(ui: &mut Ui, status: &guns::GunsStatus) {
    ui.label(format!(
        "Gun bodies: {} made · {} destroyed · {} put back · {} shots · contacts L {} R {}",
        status.made,
        status.destroyed,
        status.reseats,
        status.shots,
        status.contacts[0],
        status.contacts[1]
    ));
    for hand in Hand::BOTH {
        let Some(g) = &status.hands[hand.index()] else {
            continue;
        };
        ui.label(format!(
            "{} gun: body {} · {} · {:.2} kg from {} · held {} · braced {} · {:.0} mm, {:.1}° off its \
             seat · {:.2} m/s",
            hand.label(),
            g.body_id,
            if g.in_world { "in world" } else { "out of world" },
            g.mass_kg,
            g.mass_source.label(),
            g.holder_joint
                .map_or_else(|| "no".to_owned(), |c| format!("by {c}")),
            g.brace_joint
                .map_or_else(|| "no".to_owned(), |c| format!("by {c}")),
            g.deviation_m * 1000.0,
            g.deviation_rad.to_degrees(),
            g.speed_mps
        ));
    }
    if let Some(error) = &status.last_error {
        ui.label(format!("Last gun error: {error}"));
    }
}
