use super::*;
use crate::{
    encode_input,
    input::{
        CapturedInput, ClockDomain, GroupOrder, INPUT_VERSION, InputGroup, InputRecord, InputRoute,
        InputStamp, RouteSample, SourceQuery, ValueBits,
        policy::{
            AcceptedCommand, AcceptedSingle, POLICY_A_VERSION, PolicyATiming, PolicyEvaluation,
            PolicyOutcome,
        },
    },
};

#[test]
fn input_order_and_decision_wire_are_independent_of_the_frame_container_version() {
    assert_eq!(INPUT_VERSION, 2);
    assert_eq!(POLICY_A_VERSION, 3);
    let stamp = InputStamp {
        epoch: 13,
        clock: ClockDomain::Synthetic,
        time_ns: 123,
        sync: 11,
        sequence: 17,
    };
    let group = CapturedInput {
        capture_time_ns: 91,
        frame_sequence: Some(41),
        record: InputRecord::Group(InputGroup {
            stamp,
            order: GroupOrder::ObservedGroup,
            source_generation: 2,
            binding_generation: 3,
            action_schema: 4,
            context: 8,
            focused: true,
            samples: vec![RouteSample {
                route: InputRoute::Raw(0),
                value: ValueBits::Vec2([0x8000_0000, 0x7fc1_2345]),
                active: true,
                down: false,
                change: None,
                sources: SourceQuery::Paths(vec!["/user/hand/left/input/trigger".into()].into()),
            }],
        }),
    };
    let decision = CapturedInput {
        capture_time_ns: 92,
        frame_sequence: Some(41),
        record: InputRecord::Policy(PolicyEvaluation {
            version: POLICY_A_VERSION,
            stamp,
            action_schema: 4,
            timing: Some(PolicyATiming {
                recenter_join_ns: 100_000_000,
                panel_join_ns: 350_000_000,
                recenter_hold_ns: 1_000_000_000,
            }),
            context: 8,
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
        }),
    };
    let input_wire = [
        encode_input(&group).unwrap(),
        encode_input(&decision).unwrap(),
    ]
    .concat();
    for (version, chunked) in [(2, false), (3, true), (4, false), (4, true)] {
        let schema = FrameSchema::for_version(version).unwrap();
        let frame_wire = encode_frame_with_schema(&frame(41), schema);
        let mut batch = input_wire.clone();
        batch.extend_from_slice(&frame_wire);
        let mut bytes = match version {
            2 => encode_input_preamble(&header()),
            3 => encode_chunked_preamble(&header()),
            4 => encode_latest_preamble(&header()),
            _ => unreachable!(),
        };
        bytes.extend(if chunked {
            encode_chunk(&batch).unwrap()
        } else {
            batch.clone()
        });
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.input, vec![group.clone(), decision.clone()]);
        let mut reencoded = Vec::new();
        for record in &decoded.input {
            reencoded.extend(encode_input(record).unwrap());
        }
        assert_eq!(reencoded, input_wire);
        reencoded.extend(encode_frame_with_schema(&decoded.frames[0], schema));
        assert_eq!(reencoded, batch);
    }
}
