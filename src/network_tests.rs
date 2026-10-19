use super::*;
use crate::config::{Overrides, Startup, resolve};

fn configuration() -> Configuration {
    let env = std::env::vars().collect();
    let Startup::Participate(c) = resolve(
        &env,
        &Overrides {
            name: Some(format!("integration-{}", std::process::id())),
            name_env: None,
        },
        None,
        ContainerId::new(),
    )
    .expect("set BOOTLEG_API_URL, BOOTLEG_PROJECT_ID and BOOTLEG_PROJECT_TOKEN") else {
        panic!("live server credentials required")
    };
    *c
}
#[test]
fn registration_projection_keeps_credentials_out_of_metadata() {
    let env = [
        ("BOOTLEG_API_URL", "https://example.com"),
        ("BOOTLEG_PROJECT_ID", "project_6m8x91b0349vbrq30fxn997q5s"),
        ("BOOTLEG_PROJECT_TOKEN", "project_test-secret"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    let Startup::Participate(c) = resolve(
        &env,
        &Overrides::default(),
        Some("test"),
        ContainerId::new(),
    )
    .unwrap() else {
        panic!()
    };
    let text = encode_agent_frame(&registration(&c)).unwrap();
    assert!(!text.contains("project_test-secret"));
    assert!(text.contains("\"project_id\":\"project_6m8x91b0349vbrq30fxn997q5s\""));
    let AgentFrame::Register {
        container_id,
        project_id,
        build_commit,
        build_dirty,
        agent_version,
        application_state,
        ..
    } = decode_agent_frame(&text).unwrap()
    else {
        panic!()
    };
    assert_eq!(container_id, c.container);
    assert_eq!(project_id, c.project);
    assert!(build_commit.is_none());
    assert!(build_dirty.is_none());
    assert_eq!(agent_version.as_str(), env!("CARGO_PKG_VERSION"));
    assert_eq!(application_state, ApplicationState::Running);
}

#[tokio::test]
#[ignore = "requires the real bootleg-web server and an eligible project token"]
async fn real_web_reconnect_identity_duplicate_label_and_heartbeat() {
    let c = configuration();
    let mut policy = Participation::new(c.container.clone());
    let mut notices = NoticeMemory::default();
    let mut socket = connect(&c)
        .await
        .unwrap_or_else(|_| panic!("real server connection failed"));
    assert!(
        timeout(
            Duration::from_secs(17),
            session(&mut socket, &c, &mut policy, &mut notices)
        )
        .await
        .is_err(),
        "session ended before a heartbeat could be acknowledged"
    );
    let Phase::Connected {
        session: first,
        next_heartbeat,
    } = &policy.phase
    else {
        panic!("heartbeat did not complete")
    };
    assert!(*next_heartbeat >= Duration::from_secs(30));
    let first = first.clone();
    socket.close(None).await.unwrap();
    policy.disconnected();
    let mut replacement = connect(&c)
        .await
        .unwrap_or_else(|_| panic!("reconnect failed"));
    assert!(
        timeout(
            Duration::from_secs(1),
            session(&mut replacement, &c, &mut policy, &mut notices)
        )
        .await
        .is_err()
    );
    let Phase::Connected {
        session: second, ..
    } = &policy.phase
    else {
        panic!("replacement registration failed")
    };
    assert_ne!(&first, second);
    assert_eq!(policy.container, c.container);
    // A second process lifetime with an identical display name is independently accepted.
    let mut c2 = c;
    c2.container = ContainerId::new();
    let mut p2 = Participation::new(c2.container.clone());
    let mut duplicate = connect(&c2)
        .await
        .unwrap_or_else(|_| panic!("duplicate label connection failed"));
    assert!(
        timeout(
            Duration::from_secs(1),
            session(&mut duplicate, &c2, &mut p2, &mut notices)
        )
        .await
        .is_err()
    );
    assert!(matches!(p2.phase, Phase::Connected { .. }));
    assert_ne!(p2.container, policy.container);
    duplicate.close(None).await.unwrap();
    replacement.close(None).await.unwrap();
}
#[tokio::test]
#[ignore = "requires the real bootleg-web server"]
async fn real_web_invalid_credential_disables_participation() {
    let mut c = configuration();
    c.credential = SecretString::new("project_invalid-integration-test".into());
    let error = match connect(&c).await {
        Err(error) => error,
        Ok(mut socket) => {
            send(&mut socket, registration(&c))
                .await
                .unwrap_or_else(|_| panic!("registration send failed"));
            match timeout(Duration::from_secs(5), receive(&mut socket)).await {
                Ok(Err(error)) => error,
                _ => panic!("invalid credential was not rejected"),
            }
        }
    };
    match error {
        Failure::Server(error) => {
            assert_eq!(error.code, bootleg_core::api::ErrorCode::Unauthorized)
        }
        Failure::Stop => {}
        _ => panic!("transport failure is not evidence of authentication rejection"),
    }
}
