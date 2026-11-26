//! Hand bodies: the hand bodies' status rows, each hand body's drive, the push they give the player, and the held
//! guns' bodies.

use egui::Ui;

use crate::{
    controllers::{
        frame::Hand,
        physical_hands::physics::{BodyPhase, HandBodiesStatus, guns},
    },
    status::controllers::{
        BLOWS, BODY_ERRORS, CALLBACK, CALLBACK_FIRES, CONTACT_EVENTS, JOINTS, LEFT_BODY,
        LEFT_GUN_BODY, RIGHT_BODY, RIGHT_GUN_BODY,
    },
    ui::status_row::headlines,
};

pub(super) fn hand_bodies_readout(ui: &mut Ui, status: &HandBodiesStatus) {
    headlines(
        ui,
        &[
            &CALLBACK,
            &CALLBACK_FIRES,
            &LEFT_BODY,
            &RIGHT_BODY,
            &BODY_ERRORS,
            &CONTACT_EVENTS,
            &BLOWS,
            &JOINTS,
            &LEFT_GUN_BODY,
            &RIGHT_GUN_BODY,
        ],
    );
    ui.label(format!(
        "{} steps · player group {} · push ({:+.2}, {:+.2}) m/s",
        status.steps,
        status
            .system_group
            .map_or_else(|| "none".to_owned(), |g| g.to_string()),
        status.push.x,
        status.push.y
    ));
    if let Some(blow) = status.last_blow {
        ui.label(format!(
            "Last blow: {} hand · {} · {:.0}% of a knockdown · {}",
            blow.hand.label(),
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
            if let Some(filter) = s.filter_info {
                ui.label(format!("filter {filter:#010x}"));
            }
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
    guns_readout(ui, &guns::status());
}

fn guns_readout(ui: &mut Ui, status: &guns::GunsStatus) {
    ui.label(format!(
        "Gun bodies: {} made · {} destroyed · {} put back · {} shots",
        status.made, status.destroyed, status.reseats, status.shots
    ));
    for hand in Hand::BOTH {
        let Some(g) = &status.hands[hand.index()] else {
            continue;
        };
        ui.label(format!(
            "{} gun: held {} · braced {} · {:.2} m/s",
            hand.label(),
            g.holder_joint
                .map_or_else(|| "no".to_owned(), |c| format!("by {c}")),
            g.brace_joint
                .map_or_else(|| "no".to_owned(), |c| format!("by {c}")),
            g.speed_mps
        ));
    }
    if let Some(error) = &status.last_error {
        ui.label(format!("Last gun error: {error}"));
    }
}
