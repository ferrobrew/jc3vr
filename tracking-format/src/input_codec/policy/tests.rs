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
        }),
        context: 0,
        gameplay: true,
        outcome: PolicyOutcome::Evaluated,
        neutral: false,
        singles: [AcceptedSingle::None; 3],
        single_down: [false; 3],
        withheld: [false; 3],
        command: AcceptedCommand::None,
        raw: [0; 3],
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
    // Offsets belong to policy version 2's wire, including its present timing block.
    for (offset, value) in [
        (0, 99),
        (10, 9),
        (39, 2),
        (72, 2),
        (73, 9),
        (74, 2),
        (75, 2),
        (82, 9),
        (83, 9),
        (84, 2),
        (85, 8),
        (92, 99),
        (112, 4),
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
    for offset in [88, 108] {
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
    policy.raw = [0; 3];
    policy.direct[0].acceptance = DirectAcceptance::Accepted;
    assert!(!policy.is_valid());
}
