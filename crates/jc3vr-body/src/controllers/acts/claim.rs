//! The arbiter's values: who claims an act, what the claim does with it, and the precedence table
//! that orders the owners.

/// An act's runtime id, as the event-id symbol table resolves an `ACT_*` name.
pub type ActId = i32;

/// The mod's subsystems that drop, replace, or queue the local player's acts.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Owner {
    /// The vehicle reversing look-behind acts, dropped: the player turns their own head.
    VehicleReverse,
    /// The mental-state cuts, which drop the automatic holsters under the controller scheme.
    MentalState,
    /// The stick locomotion's turn filter, which drops the turn-in-place acts and straightens the
    /// turning starts, stops, flicks, and stunt turns.
    Locomotion,
    /// The wingsuit evade, whose barrel roll is dropped while it is switched off.
    WingsuitEvade,
    /// The weapon reconcile, which aborts a draw, holster, or switch clip still playing.
    WeaponReconcile,
    /// The slide's run start, which queues the plain move act in place of the wind-up starts.
    SlideStart,
}

/// Where an owner takes part.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Role {
    /// It filters the acts queued at this entry.
    Filter(Entry),
    /// It submits acts of its own, queued at the next flush.
    Submit,
}

/// The game's two act queues. Every act queued through [`Character::QueueAct`] reaches
/// [`AnimatedModel::QueueAct`] as well, since the first forwards to the second, so an act passes the
/// character entry's filters first and then the model entry's; an act the engine queues on the
/// model directly (the hit reactions, the air physics) passes only the model entry's.
///
/// [`Character::QueueAct`]: jc3gi::character::character::Character::QueueAct
/// [`AnimatedModel::QueueAct`]: jc3gi::character::character::AnimatedModel::QueueAct
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Entry {
    Character,
    Model,
}

/// The owners in precedence order. At an entry the filters are asked in this order and the first
/// claim wins, so a later owner is never asked about an act an earlier one claimed; a flush queues
/// the submitted acts in this order, each owner's in the order they were submitted.
pub const PRECEDENCE: [(Owner, Role); 6] = [
    (Owner::VehicleReverse, Role::Filter(Entry::Character)),
    (Owner::MentalState, Role::Filter(Entry::Character)),
    (Owner::Locomotion, Role::Filter(Entry::Character)),
    (Owner::WingsuitEvade, Role::Filter(Entry::Model)),
    (Owner::WeaponReconcile, Role::Submit),
    (Owner::SlideStart, Role::Submit),
];

/// What a claim does with its act.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActAction {
    /// Drop the act.
    Drop,
    /// Queue `with` in the act's place when the animation state machine takes it
    /// ([`AnimatedModel::TryAct`](jc3gi::character::character::AnimatedModel::TryAct)), else
    /// `refused`.
    Replace { with: ActId, refused: Refusal },
    /// Queue the act.
    Queue(Guard),
}

/// What a replacement the state machine refused becomes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Queue the original act.
    QueueOriginal,
    /// Drop the act.
    Drop,
}

/// Whether a queued act waits for the state machine's pre-flight.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Guard {
    /// Queued as it is.
    Unguarded,
    /// Queued only when the state machine would take it from its current state, as the game's own
    /// dispatchers guard their queues.
    PreFlight,
}

/// How long a claim stands.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Expiry {
    /// A filter's answer to the one queue call it was asked about.
    ThisQueue,
    /// A submitted act, held until the next flush, which queues or refuses it.
    NextFlush,
}

/// One owner's claim on one act.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ActClaim {
    pub owner: Owner,
    pub act: ActId,
    pub action: ActAction,
}

impl ActClaim {
    /// How long the claim stands: a drop or a replacement answers a queue call, and a queue waits
    /// for the flush.
    pub const fn expiry(&self) -> Expiry {
        match self.action {
            ActAction::Drop | ActAction::Replace { .. } => Expiry::ThisQueue,
            ActAction::Queue(_) => Expiry::NextFlush,
        }
    }
}

/// What a filter entry does with an act.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ActDecision {
    /// Queue the act as the game asked.
    Pass,
    /// Drop the act.
    Drop,
    /// Queue this act id instead.
    Replace(ActId),
}

/// The first claim at `entry`, asking the filters there in precedence order and stopping at the
/// first that claims.
pub(crate) fn first_claim(
    entry: Entry,
    mut ask: impl FnMut(Owner) -> Option<ActClaim>,
) -> Option<ActClaim> {
    PRECEDENCE
        .iter()
        .filter(|(_, role)| *role == Role::Filter(entry))
        .find_map(|(owner, _)| ask(*owner))
}

/// The decision a filter claim comes to. `pre_flight` asks the state machine whether it takes a
/// replacement, and is asked only for one.
pub(crate) fn settle(
    claim: Option<ActClaim>,
    pre_flight: impl FnOnce(ActId) -> bool,
) -> ActDecision {
    match claim.map(|claim| claim.action) {
        None | Some(ActAction::Queue(_)) => ActDecision::Pass,
        Some(ActAction::Drop) => ActDecision::Drop,
        Some(ActAction::Replace { with, refused }) => match (pre_flight(with), refused) {
            (true, _) => ActDecision::Replace(with),
            (false, Refusal::QueueOriginal) => ActDecision::Pass,
            (false, Refusal::Drop) => ActDecision::Drop,
        },
    }
}

/// The position of `owner` in [`PRECEDENCE`], which orders a flush.
pub(crate) fn rank(owner: Owner) -> usize {
    PRECEDENCE
        .iter()
        .position(|(o, _)| *o == owner)
        .expect("every owner is in the precedence table")
}
