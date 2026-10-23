use std::process::Command;

#[test]
fn fuzz_audit() {
    // Run the Python fuzz harness for 20 iterations; it exits 0 if all pass.
    let status = Command::new("python3")
        .args(["bench/fuzz.py", "20", "777"])
        .status().unwrap();
    assert!(status.success());
}
