//! The loadout: the live table, dual-written beside the old weapon handling, with where the two
//! disagree; and the core's sandbox table, driven by hand with made-up objects. Each shows the
//! table, the primary hand, the slot projection, the last store's sidecar, and the latest
//! transitions and refusals.

use egui::Ui;

use crate::controllers::{
    frame::Hand,
    loadout::{
        BodyLayout, CacheKind, Event, Give, GiveSource, ItemKind, LoadoutSnapshot, LostWhy,
        Release, Removal, RemovalWhy, ResetKind, Restored, SlotContent, Socket, StoreProjection,
        Take, TakeSource,
        live::LiveStatus,
        sandbox::{self, SandboxStatus},
    },
    weapons::holster::slot_label,
};

pub(super) fn loadout_readout(ui: &mut Ui, live: &LiveStatus, sandbox: &SandboxStatus) {
    ui.label(
        "The live table, written beside the old weapon handling, which still moves the weapons; \
         nothing reads the table yet.",
    );
    match &live.snapshot {
        None => {
            ui.label("Not running: no player yet, or the VR inventory is off.");
        }
        Some(snapshot) => {
            ui.label(format!(
                "{} transitions and {} refusals since the payload loaded · {} divergent",
                live.transitions,
                live.refusals,
                live.divergence.len()
            ));
            table_readout(ui, snapshot);
            for divergence in &live.divergence {
                ui.label(format!("Divergent: {divergence}"));
            }
            if let Some(store) = &live.last_store {
                store_readout(ui, store);
            }
            for line in &live.log {
                ui.label(line);
            }
        }
    }
    egui::CollapsingHeader::new("Sandbox")
        .default_open(false)
        .show(ui, |ui| sandbox_readout(ui, sandbox));
}

fn sandbox_readout(ui: &mut Ui, s: &SandboxStatus) {
    ui.label(
        "A table of its own, of made-up objects, to watch the transitions, the refusals, and the \
         projections by hand.",
    );
    ui.label(format!(
        "Snapshots held: {}",
        if s.caches.is_empty() {
            "none".to_owned()
        } else {
            s.caches
                .iter()
                .map(|c| c.label())
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    table_readout(ui, &s.snapshot);
    if let Some(store) = &s.last_store {
        store_readout(ui, store);
    }
    for line in &s.log {
        ui.label(line);
    }
    ui.separator();
    controls(ui, s);
}

/// The table's items, its primary hand, and its slots in play.
fn table_readout(ui: &mut Ui, snapshot: &LoadoutSnapshot) {
    ui.label(format!(
        "Dominant hand: {} · primary: {} · {} items",
        snapshot.body.dominant().label(),
        snapshot.primary.map_or("none", Hand::label),
        snapshot.items.len(),
    ));
    for (index, item) in snapshot.items.iter().enumerate() {
        ui.label(format!(
            "{index}: {} {} · in {} · {}",
            item.id, item.kind, item.place, item.binding
        ));
    }
    let projection = &snapshot.projection;
    ui.label(format!(
        "Slots in play (selected: {}): {}",
        projection.selected.map_or("none", slot_label),
        projection
            .slots
            .iter()
            .filter_map(|fill| match fill.content {
                SlotContent::Weapon { item, .. } =>
                    Some(format!("{} {item}", slot_label(fill.slot))),
                SlotContent::Empty => Some(format!("{} empty", slot_label(fill.slot))),
                SlotContent::Game => None,
            })
            .collect::<Vec<_>>()
            .join(" · ")
    ));
}

/// A store's slots and sidecar.
fn store_readout(ui: &mut Ui, store: &StoreProjection) {
    ui.label(format!(
        "Last store (selected: {}): {}",
        store.projection.selected.map_or("none", slot_label),
        store
            .sidecar
            .iter()
            .map(|entry| format!(
                "{} {} in {:?}, {}",
                entry.item,
                entry.weapon,
                entry.place,
                entry.slot.map_or("beyond the slots", slot_label)
            ))
            .collect::<Vec<_>>()
            .join(" · ")
    ));
}

/// The sandbox's choices, kept between frames.
#[derive(Debug, Clone)]
struct Form {
    back: u8,
    hips: u8,
    chest: bool,
    hand: usize,
    socket: usize,
    row: usize,
    kind: usize,
    source: usize,
    release: usize,
    lost: usize,
    give: usize,
    removal: usize,
    cache: usize,
    reset: usize,
}

impl Default for Form {
    fn default() -> Self {
        Form {
            back: 3,
            hips: 2,
            chest: true,
            hand: 0,
            socket: 0,
            row: 0,
            kind: 0,
            source: 0,
            release: 0,
            lost: 0,
            give: 0,
            removal: 0,
            cache: 0,
            reset: 0,
        }
    }
}

fn controls(ui: &mut Ui, s: &SandboxStatus) {
    let id = ui.id().with("loadout_sandbox_form");
    let mut form: Form = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let presets = sandbox::presets();
    let kinds: Vec<(String, ItemKind)> = presets
        .iter()
        .map(|&(label, weapon)| (label.to_owned(), ItemKind::Weapon(weapon)))
        .chain([
            ("Prop".to_owned(), ItemKind::Prop),
            ("Grenade".to_owned(), ItemKind::Grenade),
            ("Charge".to_owned(), ItemKind::Charge),
        ])
        .collect();
    let rows: Vec<String> = s
        .snapshot
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| format!("{index}: {}", item.id))
        .collect();
    ui.horizontal(|ui| {
        pick(
            ui,
            "Hand",
            &mut form.hand,
            &Hand::BOTH.map(|h| h.label().to_owned()),
        );
        pick(
            ui,
            "Socket",
            &mut form.socket,
            &Socket::ALL.map(|s| s.to_string()),
        );
        pick(ui, "Item", &mut form.row, &rows);
    });
    let hand = Hand::BOTH[form.hand.min(1)];
    let socket = Socket::ALL[form.socket.min(Socket::COUNT - 1)];
    let row_object = sandbox::object_of(form.row);
    let row_item = s.snapshot.items.get(form.row).map(|item| item.id);
    ui.horizontal(|ui| {
        pick(
            ui,
            "Kind",
            &mut form.kind,
            &labels(&kinds, |(label, _)| label.clone()),
        );
        pick(
            ui,
            "From",
            &mut form.source,
            &labels(&TakeSource::ALL, |s| s.label().to_owned()),
        );
        let kind = kinds.get(form.kind).map(|(_, kind)| *kind);
        let source = TakeSource::ALL[form.source.min(TakeSource::ALL.len() - 1)];
        if ui.button("Take a new object").clicked()
            && let Some(kind) = kind
        {
            sandbox::send(Event::Take(Take {
                hand,
                object: sandbox::object(),
                kind,
                source,
            }));
        }
        if ui.button("Take the item's object").clicked()
            && let (Some(kind), Some(object)) = (kind, row_object)
        {
            sandbox::send(Event::Take(Take {
                hand,
                object,
                kind,
                source,
            }));
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Dock").clicked() {
            sandbox::send(Event::Dock { hand, socket });
        }
        if ui.button("Draw").clicked() {
            sandbox::send(Event::Draw { socket, hand });
        }
        if ui.button("Hand over").clicked() {
            sandbox::send(Event::HandOver { from: hand });
        }
        pick(
            ui,
            "Release",
            &mut form.release,
            &labels(&Release::ALL, |r| format!("{r:?}")),
        );
        if ui.button("Let go").clicked() {
            let release = Release::ALL[form.release.min(Release::ALL.len() - 1)];
            sandbox::send(Event::LetGo { hand, release });
        }
    });
    ui.horizontal(|ui| {
        pick(
            ui,
            "Lost",
            &mut form.lost,
            &labels(&LostWhy::ALL, |w| w.label().to_owned()),
        );
        if ui.button("Lose the item").clicked()
            && let Some(object) = row_object
        {
            let why = LostWhy::ALL[form.lost.min(LostWhy::ALL.len() - 1)];
            sandbox::send(Event::Lost { object, why });
        }
        pick(
            ui,
            "Give",
            &mut form.give,
            &labels(&GiveSource::ALL, |g| g.label().to_owned()),
        );
        if ui.button("Give the kind").clicked()
            && let Some(ItemKind::Weapon(weapon)) = kinds.get(form.kind).map(|(_, kind)| *kind)
        {
            let source = GiveSource::ALL[form.give.min(GiveSource::ALL.len() - 1)];
            sandbox::send(Event::Given(Give {
                object: sandbox::object(),
                weapon,
                source,
            }));
        }
        if ui.button("Restore the kind on the socket").clicked()
            && let Some(ItemKind::Weapon(weapon)) = kinds.get(form.kind).map(|(_, kind)| *kind)
        {
            sandbox::send(Event::Restored(Restored {
                object: sandbox::object(),
                weapon,
                socket,
            }));
        }
    });
    ui.horizontal(|ui| {
        pick(
            ui,
            "Removal",
            &mut form.removal,
            &labels(&RemovalWhy::ALL, |w| w.label().to_owned()),
        );
        let why = RemovalWhy::ALL[form.removal.min(RemovalWhy::ALL.len() - 1)];
        if ui.button("Remove the object").clicked()
            && let Some(object) = row_object
        {
            sandbox::send(Event::Removed {
                target: Removal::Object(object),
                why,
            });
        }
        if ui.button("Remove the item").clicked()
            && let Some(item) = row_item
        {
            sandbox::send(Event::Removed {
                target: Removal::Item(item),
                why,
            });
        }
        if ui.button("Remove every weapon").clicked() {
            sandbox::send(Event::Removed {
                target: Removal::AllWeapons,
                why,
            });
        }
        if ui.button("The game drops the object").clicked()
            && let Some(object) = row_object
        {
            sandbox::send(Event::Dropped { object });
        }
        if ui.button("Spawn the item").clicked()
            && let Some(item) = row_item
        {
            sandbox::send(Event::Spawned {
                item,
                object: sandbox::object(),
            });
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Store").clicked() {
            sandbox::send(Event::StoreImminent);
        }
        pick(
            ui,
            "Cache",
            &mut form.cache,
            &labels(&CacheKind::ALL, |c| c.label().to_owned()),
        );
        let cache = CacheKind::ALL[form.cache.min(CacheKind::COUNT - 1)];
        if ui.button("Cache").clicked() {
            sandbox::send(Event::Cached(cache));
        }
        if ui.button("Restore").clicked() {
            sandbox::send(Event::CacheRestored {
                cache,
                slots: sandbox::slots_for_restore(cache),
            });
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Reset begins").clicked() {
            sandbox::send(Event::ResetBegan);
        }
        pick(
            ui,
            "Reset",
            &mut form.reset,
            &labels(&ResetKind::ALL, |k| k.label().to_owned()),
        );
        if ui.button("Reset completes").clicked() {
            let kind = ResetKind::ALL[form.reset.min(ResetKind::ALL.len() - 1)];
            sandbox::send(Event::ResetCompleted {
                kind,
                slots: sandbox::slots_after_reset(),
            });
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::Slider::new(&mut form.back, 0..=3).text("On the back"));
        ui.add(egui::Slider::new(&mut form.hips, 0..=2).text("Hips"));
        ui.checkbox(&mut form.chest, "Chest");
        let body = BodyLayout::new(hand, form.back, form.hips, form.chest);
        if ui.button("Change the body, the hand dominant").clicked() {
            sandbox::set_body(body);
        }
        if ui.button("Clear").clicked() {
            sandbox::clear(body);
        }
    });
    ui.data_mut(|d| d.insert_temp(id, form));
}

fn labels<T>(items: &[T], label: impl Fn(&T) -> String) -> Vec<String> {
    items.iter().map(label).collect()
}

/// A combo box over `options`, choosing an index.
fn pick(ui: &mut Ui, label: &str, index: &mut usize, options: &[String]) {
    let shown = options.get(*index).map_or("none", String::as_str);
    egui::ComboBox::from_label(label)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (i, option) in options.iter().enumerate() {
                ui.selectable_value(index, i, option);
            }
        });
}
