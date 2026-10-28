//! Socket admission for restored weapons, including reservations owned by awaiting items.

use crate::controllers::loadout::{
    Socket,
    binding::{Awaiting, Binding},
    item::{ItemId, ItemKind},
    table::{Docked, Item, Loadout, Place},
    transition::{Cause, Change, Restored, Transition},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) struct RestoreAdmission {
    pub restored: Restored,
    awaiting: Option<ItemId>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum RestoreRefusal {
    NotReady,
    AlreadyPresent,
    NoSocket,
    Changed,
}

#[cfg(test)]
mod tests;

impl Loadout {
    pub(crate) fn admit_restore(
        &self,
        mut restored: Restored,
    ) -> Result<RestoreAdmission, RestoreRefusal> {
        if self
            .items()
            .any(|item| item.binding == Binding::Live(restored.object))
        {
            return Err(RestoreRefusal::AlreadyPresent);
        }
        let waiting = self
            .items()
            .filter_map(|item| {
                let Place::Socket(socket) = item.place else {
                    return None;
                };
                (item.binding == Binding::Awaiting(Awaiting::Respawn)
                    && item.kind == ItemKind::Weapon(restored.weapon)
                    && self.body().has(socket)
                    && socket.fits(restored.weapon.size()))
                .then_some((item.id, socket))
            })
            .min_by_key(|&(_, socket)| socket != restored.socket);
        let awaiting = waiting.map(|(id, socket)| {
            restored.socket = socket;
            id
        });
        if awaiting.is_none() {
            let free = |socket: Socket| {
                self.body().has(socket)
                    && socket.fits(restored.weapon.size())
                    && !self.items().any(|item| item.place == Place::Socket(socket))
            };
            restored.socket = Some(restored.socket)
                .filter(|&socket| free(socket))
                .or_else(|| {
                    self.body()
                        .preference(restored.weapon)
                        .find(|&socket| free(socket))
                })
                .ok_or(RestoreRefusal::NoSocket)?;
        }
        Ok(RestoreAdmission { restored, awaiting })
    }

    /// Recheck the exact admission before changing either an awaiting identity or a new socket.
    pub(crate) fn commit_restore(
        &mut self,
        admission: RestoreAdmission,
    ) -> Result<Vec<Transition>, RestoreRefusal> {
        if self.admit_restore(admission.restored)? != admission {
            return Err(RestoreRefusal::Changed);
        }
        let restored = admission.restored;
        if let Some(id) = admission.awaiting {
            return self
                .spawned(id, restored.object)
                .map_err(|_| RestoreRefusal::Changed);
        }
        let item = Item {
            id: self.allocate(),
            kind: ItemKind::Weapon(restored.weapon),
            binding: Binding::Live(restored.object),
        };
        let docked = Docked::new(item, restored.socket).expect("admitted restore socket fits");
        let transition = Transition {
            item: docked.item.id,
            kind: docked.item.kind,
            change: Change::Entered {
                to: Place::Socket(restored.socket),
            },
            cause: Cause::Restored,
        };
        self.sockets[restored.socket.index()] = Some(docked);
        Ok(vec![transition])
    }
}
