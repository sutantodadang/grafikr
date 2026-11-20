use std::process::Command;

#[test]
fn host_version_is_reported_without_entering_protocol_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_grafikr-host"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("grafikr-host 1.1.0 protocol 1.0"));
    assert!(output.stderr.is_empty());
}
