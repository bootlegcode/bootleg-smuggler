use std::process::Command;

#[test]
fn missing_application_shows_actionable_plain_guide_on_stderr() {
    for args in [vec![], vec!["--"], vec!["--name", "demo"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_smuggler"))
            .env_clear()
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(78));
        assert!(output.stdout.is_empty());
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(text.contains("Bootleg Smuggler"));
        assert!(text.contains("Let's give Smuggler an application"));
        assert!(text.contains("BOOTLEG_DISABLED=true cargo run -- --"));
        assert!(text.contains("BOOTLEG_PROJECT_TOKEN"));
        assert!(text.contains("Set BOOTLEG_API_URL"));
        assert!(text.contains("required for registration"));
        assert!(!text.contains('\x1b'));
    }
}

#[test]
fn explicit_help_is_successful_and_does_not_validate_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_smuggler"))
        .env_clear()
        .env("NO_COLOR", "1")
        .env("BOOTLEG_PROJECT_ID", "invalid")
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Ready to register a container?"));
    assert!(!text.contains("Let's give"));
    assert!(!text.contains('\x1b'));
}
