//! Real executable, children and Unix signals. No HTTP or WebSocket mock server.
#![cfg(unix)]
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
fn command(enabled: bool) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_smuggler"));
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stderr(Stdio::null());
    if enabled {
        c.env("BOOTLEG_PROJECT_ID", "project_6m8x91b0349vbrq30fxn997q5s")
            .env("BOOTLEG_PROJECT_TOKEN", "project_test-only")
            .env("BOOTLEG_API_URL", "http://127.0.0.1:1")
            .env("BOOTLEG_ALLOW_INSECURE", "true");
    }
    c
}
fn wait(child: &mut Child, budget: Duration) -> i32 {
    let deadline = Instant::now() + budget;
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s.code().unwrap_or(-1);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("supervisor exceeded test deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn child_starts_and_exit_is_preserved_during_control_plane_outage() {
    let mut child = command(true)
        .args(["--", "/bin/sh", "-c", "exit 23"])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child, Duration::from_secs(3)), 23);
}
#[test]
fn exec_through_preserves_exit_and_ignores_invalid_settings_when_disabled() {
    let mut child = command(false)
        .env("BOOTLEG_DISABLED", "true")
        .env("BOOTLEG_PROJECT_ID", "invalid")
        .env("BOOTLEG_API_URL", "invalid")
        .args(["--", "/bin/sh", "-c", "exit 17"])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child, Duration::from_secs(2)), 17);
    let mut child = command(false)
        .args(["--", "/bin/sh", "-c", "exit 19"])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child, Duration::from_secs(2)), 19);
}
#[test]
fn partial_credentials_reject_before_application_launch() {
    let mut child = command(false)
        .env("BOOTLEG_PROJECT_ID", "project_6m8x91b0349vbrq30fxn997q5s")
        .args(["--", "/bin/sh", "-c", "exit 23"])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child, Duration::from_secs(2)), 78);
}
#[test]
fn missing_application_is_127() {
    for enabled in [false, true] {
        let mut child = command(enabled)
            .args(["--", "/does-not-exist-smuggler-test"])
            .spawn()
            .unwrap();
        assert_eq!(wait(&mut child, Duration::from_secs(2)), 127);
    }
}
#[test]
fn term_is_forwarded_to_child_group_and_meaningful_status_preserved() {
    let mut child = command(true)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "trap 'exit 42' TERM; echo ready; while :; do sleep 1; done",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::BufRead;
    let mut ready = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    assert_eq!(wait(&mut child, Duration::from_secs(3)), 42);
}
#[test]
fn child_signal_exit_is_128_plus_signal() {
    let mut child = command(true)
        .args(["--", "/bin/sh", "-c", "kill -KILL $$"])
        .spawn()
        .unwrap();
    assert_eq!(wait(&mut child, Duration::from_secs(3)), 137);
}
#[test]
fn shutdown_escalates_uncooperative_child() {
    let mut child = command(true)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "trap '' TERM; echo ready; while :; do sleep 1; done",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::BufRead;
    let mut ready = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready, "ready\n");
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    assert_eq!(wait(&mut child, Duration::from_secs(13)), 137);
}
#[cfg(target_os = "linux")]
#[test]
fn adopted_orphan_is_reaped_while_main_child_runs() {
    let mut child = command(true)
        .args([
            "--",
            "/bin/sh",
            "-c",
            "(sleep 0.2 & echo $!) & wait; sleep 3",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::BufRead;
    let mut pid = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut pid)
        .unwrap();
    let pid: i32 = pid.trim().parse().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        -1,
        "orphan still exists (possibly zombie)"
    );
    assert_eq!(wait(&mut child, Duration::from_secs(4)), 0);
}
