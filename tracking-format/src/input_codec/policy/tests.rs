use super::*;
use crate::input::policy::POLICY_A_VERSION;

fn fixture() -> PolicyEvaluation {
    PolicyEvaluation {
        version: POLICY_A_VERSION,
        stamp: InputStamp {
            epoch: 1,
            clock: ClockDomain::Synthetic,
            time_ns: 1,
            sync: 1,
            sequence: 1,
        },
        action_schema: 4,
        timing: Some(PolicyATiming {
            recenter_join_ns: 100_000_000,
            panel_join_ns: 350_000_000,
            recenter_hold_ns: 1_000_000_000,
            suppress_developer_chords_in_vehicles: true,
        }),
        context: 0,
        gameplay: true,
        outcome: PolicyOutcome::Evaluated,
        neutral: false,
        singles: [AcceptedSingle::None; 4],
        single_down: [false; 4],
        withheld: [false; 4],
        command: AcceptedCommand::None,
        raw: [0; 4],
        direct: Default::default(),
        claims: 0,
        commands: 0,
    }
}
fn bytes() -> Vec<u8> {
    let mut out = Encoder::default();
    encode(&mut out, &fixture());
    out.bytes
}

#[test]
fn hostile_policy_flags_enums_versions_and_schema_are_rejected() {
    // Offsets belong to this policy version's wire, including its present timing block.
    assert_eq!(bytes().len(), 118);
    for (offset, value) in [
        (0, 99),
        (10, 9),
        (39, 2),
        (64, 2),
        (73, 2),
        (74, 9),
        (75, 2),
        (76, 2),
        (83, 9),
        (84, 2),
        (85, 2),
        (86, 9),
        (87, 9),
        (88, 2),
        (89, 8),
        (92, 8),
        (97, 99),
        (117, 8),
    ] {
        let mut data = bytes();
        data[offset] = value;
        assert!(decode(&mut Decoder::new(&data)).is_err(), "offset {offset}");
    }
    let mut data = bytes();
    data[35..39].fill(0);
    assert!(decode(&mut Decoder::new(&data)).is_err());
}

#[test]
fn hostile_policy_timing_and_source_masks_are_rejected() {
    for (offset, value) in [
        (40, 0u64),
        (40, u64::MAX),
        (48, 0),
        (48, 1_000_000_001),
        (56, 0),
        (56, 60_000_000_001),
    ] {
        let mut data = bytes();
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(decode(&mut Decoder::new(&data)).is_err());
    }
    for offset in [93, 113] {
        let mut data = bytes();
        data[offset..offset + 4].copy_from_slice(&0x1_0000u32.to_le_bytes());
        assert!(decode(&mut Decoder::new(&data)).is_err());
    }
}

#[test]
fn boundary_records_cannot_carry_commands_or_accepted_edges() {
    let mut policy = fixture();
    policy.outcome = PolicyOutcome::NeutralBoundary;
    policy.neutral = true;
    assert!(policy.is_valid());
    policy.commands = 1;
    assert!(!policy.is_valid());
    policy.commands = 0;
    policy.raw[0] = 2;
    assert!(!policy.is_valid());
    policy.raw = [0; 4];
    policy.direct[0].acceptance = DirectAcceptance::Accepted;
    assert!(!policy.is_valid());
}

#[test]
fn version_4_decisions_decode_with_their_chords_unsuppressed() {
    // Version 4's timing block ends at the recenter hold, one byte before the version 5 flag.
    let mut current = fixture();
    current.timing = current.timing.map(|timing| PolicyATiming {
        suppress_developer_chords_in_vehicles: false,
        ..timing
    });
    let mut out = Encoder::default();
    encode(&mut out, &current);
    let mut legacy = out.bytes;
    legacy[0..2].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(legacy.remove(64), 0);
    let decoded = decode(&mut Decoder::new(&legacy)).unwrap();
    assert_eq!(decoded, current);

    let mut untimed = fixture();
    untimed.timing = None;
    untimed.outcome = PolicyOutcome::Cancelled;
    let mut out = Encoder::default();
    encode(&mut out, &untimed);
    let mut legacy = out.bytes;
    legacy[0..2].copy_from_slice(&4u16.to_le_bytes());
    assert_eq!(decode(&mut Decoder::new(&legacy)).unwrap(), untimed);
}

#[test]
fn version_3_decisions_are_rejected() {
    let mut data = bytes();
    data[0..2].copy_from_slice(&3u16.to_le_bytes());
    assert!(decode(&mut Decoder::new(&data)).is_err());
}
