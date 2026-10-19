//! Shared golden fixtures are serialization examples, not mock network services.
use bootleg_core::protocol::*;
use bootleg_smuggler::lifecycle::{Participation, Phase};
use std::time::Duration;
#[test]
fn frozen_registration_and_heartbeat_examples_map_to_lifecycle() {
    let AgentFrame::Register { container_id, .. } =
        decode_agent_frame(include_str!("../../bootleg-core/fixtures/v1/register.json")).unwrap()
    else {
        panic!()
    };
    let mut policy = Participation::new(container_id);
    policy.begin(Duration::ZERO).unwrap();
    let ServerFrame::Registered {
        smuggler_session_id,
        heartbeat_interval_seconds,
        lease_timeout_seconds,
        ..
    } = decode_server_frame(include_str!(
        "../../bootleg-core/fixtures/v1/registered.json"
    ))
    .unwrap()
    else {
        panic!()
    };
    policy
        .registered(
            smuggler_session_id,
            Duration::from_secs(1),
            heartbeat_interval_seconds,
            lease_timeout_seconds,
        )
        .unwrap();
    let session = policy.heartbeat(Duration::from_secs(16)).unwrap();
    let AgentFrame::Heartbeat {
        smuggler_session_id,
        ..
    } = decode_agent_frame(include_str!(
        "../../bootleg-core/fixtures/v1/heartbeat.json"
    ))
    .unwrap()
    else {
        panic!()
    };
    assert_eq!(session, smuggler_session_id);
    let ServerFrame::HeartbeatAck {
        smuggler_session_id,
        ..
    } = decode_server_frame(include_str!(
        "../../bootleg-core/fixtures/v1/heartbeat-ack.json"
    ))
    .unwrap()
    else {
        panic!()
    };
    policy
        .acknowledged(&smuggler_session_id, Duration::from_secs(17))
        .unwrap();
    assert!(matches!(policy.phase, Phase::Connected { .. }));
}
#[test]
fn malformed_unknown_commands_and_oversized_frames_are_rejected() {
    for frame in [
        "{}",
        r#"{"type":"execute","command":"touch /tmp/bad"}"#,
        &"x".repeat(16385),
    ] {
        assert!(decode_server_frame(frame).is_err());
    }
    let ServerFrame::Error { error } =
        decode_server_frame(include_str!("../../bootleg-core/fixtures/v1/ws-error.json")).unwrap()
    else {
        panic!()
    };
    assert!(!error.message.as_str().is_empty());
}
