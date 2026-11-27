use bootleg_core::{ContainerId, SmugglerSessionId, api::ErrorCode};
use bootleg_smuggler::{config::*, lifecycle::*};
use std::time::Duration;
fn env(values: &[(&str, &str)]) -> Environment {
    values
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}
fn enabled() -> Environment {
    env(&[
        ("BOOTLEG_API_URL", "https://example.com"),
        ("BOOTLEG_PROJECT_ID", "project_6m8x91b0349vbrq30fxn997q5s"),
        ("BOOTLEG_PROJECT_TOKEN", "project_test-only"),
    ])
}
fn resolve_enabled(e: &Environment) -> Result<Startup, ConfigError> {
    resolve(e, &Overrides::default(), None, ContainerId::new())
}
#[test]
fn startup_noop_overrides_invalid_configuration() {
    assert!(matches!(
        resolve_enabled(&Environment::new()).unwrap(),
        Startup::ExecThrough
    ));
    let e = env(&[
        ("BOOTLEG_DISABLED", "true"),
        ("BOOTLEG_PROJECT_ID", "not-an-id"),
        ("BOOTLEG_API_URL", "bad"),
        ("BOOTLEG_NAME_ENV", "MISSING"),
    ]);
    assert!(matches!(resolve_enabled(&e).unwrap(), Startup::ExecThrough));
    for key in ["BOOTLEG_PROJECT_ID", "BOOTLEG_PROJECT_TOKEN"] {
        let mut e = enabled();
        e.remove(key);
        assert!(resolve_enabled(&e).is_err());
    }
}
#[test]
fn credentials_and_origins_are_validated_and_redacted() {
    let mut e = enabled();
    e.remove("BOOTLEG_API_URL");
    assert!(
        resolve_enabled(&e)
            .unwrap_err()
            .contains("BOOTLEG_API_URL is required for registration")
    );

    for (key, value) in [
        ("BOOTLEG_PROJECT_ID", "1"),
        ("BOOTLEG_PROJECT_ID", "01"),
        ("BOOTLEG_PROJECT_ID", "container_6m8x91b0349vbrq30fxn997q5s"),
        ("BOOTLEG_PROJECT_ID", "project_secret"),
        ("BOOTLEG_PROJECT_TOKEN", "hash"),
        ("BOOTLEG_PROJECT_TOKEN", "project_bad\nvalue"),
        ("BOOTLEG_API_URL", "http://example.com"),
        ("BOOTLEG_API_URL", "https://user:secret@example.com"),
        ("BOOTLEG_API_URL", "https://example.com/?token=bad"),
        ("BOOTLEG_API_URL", "https://example.com/#bad"),
        ("BOOTLEG_API_URL", "https://example.com/prefix"),
    ] {
        let mut e = enabled();
        e.insert(key.into(), value.into());
        assert!(resolve_enabled(&e).is_err(), "{key}");
    }
    let e = enabled();
    let configuration = resolve_enabled(&e).unwrap();
    assert!(!format!("{configuration:?}").contains("project_test-only"));
    let mut e = enabled();
    e.insert("BOOTLEG_API_URL".into(), "http://localhost:3000".into());
    e.insert("BOOTLEG_ALLOW_INSECURE".into(), "true".into());
    let Startup::Participate(c) = resolve_enabled(&e).unwrap() else {
        panic!()
    };
    assert_eq!(
        c.endpoint.as_str(),
        "ws://localhost:3000/api/v1/agent/connect"
    );
}
#[test]
fn deterministic_name_precedence_and_explicit_missing_errors() {
    let id = ContainerId::new();
    let mut e = env(&[
        ("BOOTLEG_INSTANCE_NAME", "explicit"),
        ("BOOTLEG_NAME_ENV", "CUSTOM"),
        ("CUSTOM", "custom"),
        ("POD_NAME", "pod"),
        ("KUBERNETES_POD_NAME", "kube"),
        ("HOSTNAME", "host"),
        ("BOOTLEG_CONTAINER_NAME", "app"),
    ]);
    let flags = Overrides {
        name: Some("flag".into()),
        name_env: None,
    };
    assert_eq!(
        resolve_name(&e, &flags, Some("os"), &id).unwrap().as_str(),
        "flag"
    );
    let flags = Overrides::default();
    for (key, expected) in [
        ("BOOTLEG_INSTANCE_NAME", "explicit"),
        ("BOOTLEG_NAME_ENV", "custom"),
        ("POD_NAME", "pod/app"),
        ("KUBERNETES_POD_NAME", "kube/app"),
        ("HOSTNAME", "host"),
    ] {
        assert_eq!(
            resolve_name(&e, &flags, Some("os"), &id).unwrap().as_str(),
            expected
        );
        e.remove(key);
    }
    assert_eq!(
        resolve_name(&e, &flags, Some("os"), &id).unwrap().as_str(),
        "os"
    );
    assert!(
        resolve_name(&e, &flags, None, &id)
            .unwrap()
            .as_str()
            .starts_with("container-")
    );
    e.insert("BOOTLEG_NAME_ENV".into(), "MISSING".into());
    assert!(resolve_name(&e, &flags, Some("os"), &id).is_err());
    for value in ["", " trailing ", "bad\nlabel"] {
        e.insert("MISSING".into(), value.into());
        assert!(resolve_name(&e, &flags, None, &id).is_err());
    }
    e.insert("MISSING".into(), "x".repeat(129));
    assert!(resolve_name(&e, &flags, None, &id).is_err());
    let flags = Overrides {
        name: None,
        name_env: Some("CUSTOM".into()),
    };
    assert_eq!(
        resolve_name(&e, &flags, None, &id).unwrap().as_str(),
        "custom"
    );
}
#[test]
fn baseline_unknown_and_full_hash_only() {
    let Startup::Participate(c) = resolve_enabled(&enabled()).unwrap() else {
        panic!()
    };
    assert!(c.baseline.commit.is_none());
    assert!(c.baseline.dirty.is_none());
    for length in [40, 64] {
        let mut e = enabled();
        e.insert("BOOTLEG_BUILD_COMMIT".into(), "a".repeat(length));
        e.insert("BOOTLEG_BUILD_DIRTY".into(), "true".into());
        let Startup::Participate(c) = resolve_enabled(&e).unwrap() else {
            panic!()
        };
        assert_eq!(c.baseline.dirty, Some(true));
        assert!(c.baseline.commit.is_some());
    }
    for value in ["abc123", &"A".repeat(40)] {
        let mut e = enabled();
        e.insert("BOOTLEG_BUILD_COMMIT".into(), value.into());
        assert!(resolve_enabled(&e).is_err());
    }
}
fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}
#[test]
fn session_transitions_fence_acks_and_preserve_container() {
    let container = ContainerId::new();
    let mut p = Participation::new(container.clone());
    let s1 = SmugglerSessionId::new();
    let s2 = SmugglerSessionId::new();
    assert!(p.acknowledged(&s1, secs(0)).is_err());
    assert!(p.heartbeat(secs(0)).is_err());
    p.begin(secs(0)).unwrap();
    assert!(p.begin(secs(0)).is_err());
    assert!(p.registered(s1.clone(), secs(5), 15, 45).is_err());
    assert!(p.registered(s1.clone(), secs(1), 0, 45).is_err());
    p.registered(s1.clone(), secs(1), 15, 45).unwrap();
    assert!(p.registered(s2.clone(), secs(2), 15, 45).is_err());
    assert!(p.heartbeat(secs(15)).is_err());
    assert_eq!(p.heartbeat(secs(16)).unwrap(), s1);
    assert!(p.heartbeat(secs(17)).is_err());
    assert!(p.acknowledged(&s2, secs(17)).is_err());
    assert!(p.acknowledged(&s1, secs(31)).is_err());
    p.disconnected();
    p.begin(secs(32)).unwrap();
    p.registered(s2.clone(), secs(33), 15, 45).unwrap();
    assert_eq!(p.container, container);
    assert_eq!(p.heartbeat(secs(48)).unwrap(), s2);
    assert!(p.acknowledged(&s1, secs(49)).is_err());
    p.acknowledged(&s2, secs(49)).unwrap();
}
#[test]
fn retries_stop_for_permanent_errors_and_reset_only_on_heartbeat() {
    let mut p = Participation::new(ContainerId::new());
    for expected in [1, 2, 4, 8, 16, 30, 30] {
        assert_eq!(p.retry_delay(1.0, 0), secs(expected));
    }
    assert_eq!(p.retry_delay(0.0, 0), secs(0));
    assert_eq!(p.retry_delay(0.5, 60), secs(60));
    let id = SmugglerSessionId::new();
    p.begin(secs(0)).unwrap();
    p.registered(id.clone(), secs(1), 15, 45).unwrap();
    assert_eq!(p.retry_delay(1.0, 0), secs(30));
    p.heartbeat(secs(16)).unwrap();
    p.acknowledged(&id, secs(17)).unwrap();
    assert_eq!(p.retry_delay(1.0, 0), secs(1));
    for code in [
        ErrorCode::Unauthorized,
        ErrorCode::Forbidden,
        ErrorCode::NotFound,
        ErrorCode::Validation,
        ErrorCode::ProtocolIncompatible,
    ] {
        let mut p = Participation::new(ContainerId::new());
        p.failure(code);
        p.disconnected();
        assert_eq!(p.phase, Phase::Disabled);
        assert!(p.begin(secs(0)).is_err());
    }
    for code in [
        ErrorCode::Conflict,
        ErrorCode::ServiceUnavailable,
        ErrorCode::RateLimited,
        ErrorCode::QuotaExceeded,
    ] {
        assert!(!terminal(code));
    }
}
#[test]
fn notices_are_bounded_diagnostics_not_lifecycle_events() {
    let p = Participation::new(ContainerId::new());
    let mut memory = NoticeMemory::default();
    assert!(memory.first_seen("a"));
    assert!(!memory.first_seen("a"));
    for n in 0..128 {
        assert!(memory.first_seen(&n.to_string()));
    }
    assert!(memory.first_seen("a"));
    assert_eq!(p.phase, Phase::Disconnected);
    assert_eq!(
        diagnostic_text("server wording\u{202e}\u{001b}"),
        "server wording"
    );
}
#[test]
fn typed_lifetime_identity_roundtrips_and_duplicate_names_are_valid() {
    let first = ContainerId::new();
    let second = ContainerId::new();
    assert_ne!(first, second);
    assert_eq!(first.to_string().parse::<ContainerId>().unwrap(), first);
    assert!(first.to_string().parse::<SmugglerSessionId>().is_err());
    assert!(
        "container_00000000000000000000000000"
            .parse::<ContainerId>()
            .is_err()
    );
    let session = SmugglerSessionId::new();
    assert_eq!(
        session.to_string().parse::<SmugglerSessionId>().unwrap(),
        session
    );
    let e = env(&[("BOOTLEG_INSTANCE_NAME", "same label")]);
    assert_eq!(
        resolve_name(&e, &Overrides::default(), None, &first).unwrap(),
        resolve_name(&e, &Overrides::default(), None, &second).unwrap()
    );
}
