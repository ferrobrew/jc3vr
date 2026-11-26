use super::*;
use crate::controllers::weapons::dual_wield::single::grant::{Grant, GrantTick, Grants};

fn pending_request(request: u64, generation: u64, socket: Socket, hand: Option<Hand>) -> Pending {
    Pending {
        token: SpawnToken::Give { request },
        generation,
        socket,
        hand,
        name: "w999_test_weapon",
        tag_index: 1,
        at: Instant::now(),
        noted_loading: false,
    }
}

#[test]
fn requests_wait_for_readiness_then_dispatch_in_order_without_coalescing() {
    let mut queue = RequestQueue::default();
    queue.push(GiveRequest::Grenades(2));
    queue.push(GiveRequest::Grenades(3));
    queue.push(GiveRequest::Clear(Socket::Hip(Hand::Left)));
    assert!(queue.take(false).is_empty());
    assert_eq!(
        queue.take(true),
        vec![
            GiveRequest::Grenades(2),
            GiveRequest::Grenades(3),
            GiveRequest::Clear(Socket::Hip(Hand::Left)),
        ]
    );
    assert!(queue.take(true).is_empty());
}

#[test]
fn the_queue_preserves_admitted_edits_when_full() {
    let mut queue = RequestQueue::default();
    for count in 0..MAX_QUEUED + 1 {
        queue.push(GiveRequest::Grenades(count as i32));
    }
    assert_eq!(queue.requests.len(), MAX_QUEUED);
    assert_eq!(
        queue.take(true),
        (0..MAX_QUEUED)
            .map(|count| GiveRequest::Grenades(count as i32))
            .collect::<Vec<_>>()
    );
}

#[test]
fn pending_selection_survives_dispatch_and_latest_clear_wins() {
    let mut queue = RequestQueue::default();
    let name = weapons_for(Socket::Hip(Hand::Left))[0].name.as_str();
    let target = GiveTarget::Socket(Socket::Hip(Hand::Left));
    queue.push(GiveRequest::Weapon {
        socket: Socket::Hip(Hand::Left),
        name: name.to_owned(),
    });
    assert_eq!(queue.requested(target), Some(Requested::Weapon(name)));
    assert!(queue.take(false).is_empty());
    assert_eq!(queue.requested(target), Some(Requested::Weapon(name)));
    assert_eq!(queue.take(true).len(), 1);
    assert_eq!(queue.requested(target), Some(Requested::Weapon(name)));
    queue.push(GiveRequest::Clear(Socket::Hip(Hand::Left)));
    assert_eq!(queue.requested(target), Some(Requested::Nothing));
    assert_eq!(queue.requested(GiveTarget::Hand(Hand::Left)), None);
    queue.dispatching.clear();
    queue.take(true);
    assert_eq!(queue.requested(target), Some(Requested::Nothing));
    queue.dispatching.clear();
    assert_eq!(queue.requested(target), None);
}

#[test]
fn a_given_single_pistol_asks_for_no_twin() {
    let mut grants = Grants::new();
    for socket_free in [false, true] {
        for object in [11, 12, 11] {
            assert_eq!(
                grants.observe(GrantTick {
                    slot: Some(object),
                    single: true,
                    guns: true,
                    stashed: false,
                    socket_free,
                    dt: 0.01,
                }),
                Grant::Nothing,
            );
        }
    }
}

#[test]
fn spawn_identities_are_unique_and_never_wrap() {
    let sequence = AtomicU64::new(0);
    let first = next_token(&sequence).unwrap();
    let second = next_token(&sequence).unwrap();
    assert_ne!(first, second);
    assert_eq!(SpawnToken::decode(first.encode()), Some(first));
    assert_eq!(SpawnToken::decode(second.encode()), Some(second));
    let sequence = AtomicU64::new(SpawnToken::GENERATION_MASK - 1);
    assert!(next_token(&sequence).is_some());
    assert!(next_token(&sequence).is_none());
    assert!(next_token(&sequence).is_none());
}

#[test]
fn superseded_callbacks_cannot_consume_a_replacements_request() {
    let mut pending = vec![pending_request(1, 3, Socket::Hip(Hand::Left), None)];
    let removed = remove_target(&mut pending, Some(Socket::Hip(Hand::Left)), None);
    assert_eq!(removed.len(), 1);
    pending.push(pending_request(2, 3, Socket::Hip(Hand::Left), None));
    assert!(take_pending(&mut pending, SpawnToken::Give { request: 1 }, 3).is_none());
    assert!(take_pending(&mut pending, SpawnToken::Give { request: 2 }, 3).is_some());
    assert!(take_pending(&mut pending, SpawnToken::Give { request: 2 }, 3).is_none());
}

#[test]
fn clear_cancels_by_socket_or_hand_and_old_world_results_are_rejected() {
    let mut pending = vec![
        pending_request(1, 4, Socket::Hip(Hand::Left), Some(Hand::Left)),
        pending_request(2, 4, Socket::Shoulder(Hand::Right), Some(Hand::Right)),
    ];
    assert_eq!(remove_target(&mut pending, None, Some(Hand::Left)).len(), 1);
    assert!(take_pending(&mut pending, SpawnToken::Give { request: 2 }, 5).is_none());
    assert!(pending.is_empty());
    pending.push(pending_request(
        3,
        5,
        Socket::Hip(Hand::Left),
        Some(Hand::Left),
    ));
    assert_eq!(
        remove_target(&mut pending, Some(Socket::Hip(Hand::Left)), None).len(),
        1
    );
    assert!(pending.is_empty());
}

fn def(name: &str, category: &str, tags: &str) -> SpawnDefinition {
    SpawnDefinition {
        name: name.to_owned(),
        category: category.to_owned(),
        tags: tags.to_owned(),
        pool: "weapon".to_owned(),
        entity_hash: None,
    }
}

#[test]
fn hand_weapons_are_sorted_by_whether_a_hip_takes_them() {
    let pistol = def("w011_pistol_a", "Weapon", "weapon,bullet,one_handed,pistol");
    let handgun = def("w014_handgun_b", "Weapon", "weapon,golden");
    let rifle = def("w111_rifle_c", "Weapon", "weapon,two_handed,bullet,rifle");
    let minigun = def("w201_minigun", "Weapon", "weapon,unmounted,heavy,minigun");
    assert_eq!(giveable(&pistol), Some(true));
    assert_eq!(giveable(&handgun), Some(true));
    assert_eq!(giveable(&rifle), Some(false));
    assert_eq!(giveable(&minigun), Some(false));
}

#[test]
fn mounted_thrown_and_vehicle_weapons_are_not_given() {
    for d in [
        def("w201_mounted_minigun", "", "weapon,mounted,heavy"),
        def(
            "w301_frag_grenade",
            "Weapon",
            "weapon,thrown,explosive,grenade",
        ),
        def(
            "w401_explosives_c4",
            "",
            "weapon,thrown,planted_explosive,planted",
        ),
        def("w701_missile_strike", "Weapon", "weapon,fow,w701"),
        def("w501_turret_sam", "Weapon|Stationary", "weapon,turret,sam"),
        def("w601_helicopter_missile", "", "do_not_spawn"),
    ] {
        assert_eq!(giveable(&d), None, "{}", d.name);
    }
}

#[test]
fn a_give_waits_past_the_short_wait_only_while_its_entity_streams() {
    let past = GIVE_WAIT + Duration::from_secs(1);
    for streaming in [
        SpawnResourceDefStatus::LOADING,
        SpawnResourceDefStatus::UNLOADED,
    ] {
        assert_eq!(wait_verdict(Some(streaming), past), Verdict::Wait);
        assert_eq!(
            wait_verdict(Some(streaming), GIVE_WAIT_LOADING + Duration::from_secs(1)),
            Verdict::GiveUp
        );
    }
    for settled in [
        None,
        Some(SpawnResourceDefStatus::LOADED),
        Some(SpawnResourceDefStatus::UNLOADING),
        Some(SpawnResourceDefStatus::LOAD_ERROR),
    ] {
        assert_eq!(wait_verdict(settled, past), Verdict::GiveUp);
    }
}

#[test]
fn a_refused_definition_keeps_its_latest_reason() {
    let name = "w999_test_only_definition";
    assert_eq!(refusal(name), None);
    refuse(name, Refusal::NoDefinition);
    refuse(name, Refusal::LoadError);
    assert_eq!(refusal(name), Some(Refusal::LoadError.label()));
    assert_eq!(REFUSED.lock().iter().filter(|(n, _)| *n == name).count(), 1);
}

#[test]
fn a_hand_give_takes_the_first_free_socket_on_the_hands_side() {
    let none = |_: Socket| false;
    assert_eq!(hand_socket(Hand::Left, true, none), Socket::Hip(Hand::Left));
    assert_eq!(
        hand_socket(Hand::Right, false, none),
        Socket::Shoulder(Hand::Right)
    );
    let own_hip = |socket: Socket| socket == Socket::Hip(Hand::Left);
    assert_eq!(
        hand_socket(Hand::Left, true, own_hip),
        Socket::Hip(Hand::Right)
    );
    let both_hips = |socket: Socket| matches!(socket, Socket::Hip(_));
    assert_eq!(
        hand_socket(Hand::Left, true, both_hips),
        Socket::Hip(Hand::Left)
    );
    let back_full = |socket: Socket| !matches!(socket, Socket::Hip(_));
    assert_eq!(
        hand_socket(Hand::Left, false, back_full),
        Socket::Shoulder(Hand::Left)
    );
}

#[test]
fn a_hand_is_offered_every_hand_weapon() {
    let hand = weapons_for_hand();
    assert_eq!(
        hand.len(),
        weapons_for(Socket::Hip(Hand::Left)).len()
            + weapons_for(Socket::Shoulder(Hand::Right)).len()
    );
}

#[test]
fn every_shipped_socket_list_is_nonempty_and_disjoint() {
    let small = weapons_for(Socket::Hip(Hand::Left));
    let large = weapons_for(Socket::Shoulder(Hand::Right));
    assert!(!small.is_empty() && !large.is_empty());
    assert!(
        small
            .iter()
            .all(|s| !large.iter().any(|l| l.name == s.name))
    );
}
