//! Give records and retained arrivals share one inventory/world identity.

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(super) struct Identity {
    inventory: usize,
    generation: u64,
}

pub(super) struct Authority {
    inventory: usize,
    generation: Option<u64>,
}

impl Authority {
    pub(super) const fn new() -> Self {
        Self {
            inventory: 0,
            generation: Some(0),
        }
    }

    pub(super) fn observe(&mut self, inventory: usize) -> bool {
        if self.inventory == inventory {
            return false;
        }
        self.invalidate();
        self.inventory = inventory;
        true
    }

    pub(super) fn invalidate(&mut self) {
        self.generation = self
            .generation
            .and_then(|generation| generation.checked_add(1));
    }

    pub(super) fn identity(&self, inventory: usize) -> Option<Identity> {
        (inventory != 0 && inventory == self.inventory).then_some(Identity {
            inventory,
            generation: self.generation?,
        })
    }

    pub(super) fn current(&self, identity: Identity, inventory: usize) -> bool {
        self.identity(inventory) == Some(identity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_requires_the_current_inventory_and_world() {
        let mut authority = Authority::new();
        assert!(authority.observe(11));
        let old = authority.identity(11).unwrap();
        let mut calls = Vec::new();
        let dispatch = |authority: &Authority, identity, inventory, calls: &mut Vec<_>| {
            if authority.current(identity, inventory) {
                calls.push(inventory);
            }
        };
        dispatch(&authority, old, 11, &mut calls);
        authority.invalidate();
        dispatch(&authority, old, 11, &mut calls);
        let reset = authority.identity(11).unwrap();
        dispatch(&authority, reset, 11, &mut calls);
        authority.observe(22);
        dispatch(&authority, reset, 11, &mut calls);
        dispatch(&authority, reset, 22, &mut calls);
        assert_eq!(calls, [11, 11]);
        assert!(!authority.observe(22));
    }

    #[test]
    fn exhausted_generations_never_reopen_admission() {
        let mut authority = Authority {
            inventory: 11,
            generation: Some(u64::MAX),
        };
        authority.invalidate();
        assert_eq!(authority.identity(11), None);
        authority.observe(22);
        assert_eq!(authority.identity(22), None);
    }
}
