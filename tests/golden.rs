use std::fs;
use std::process::Command;

#[test]
fn golden_tests() {
    let bin = env!("CARGO_BIN_EXE_incc");
    let dir = "tests/golden";
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("src") { continue; }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let expected_path = format!("{}/{}.expected", dir, stem);
        let expected: i32 = fs::read_to_string(&expected_path).unwrap().trim().parse().unwrap();
        let out = format!("/tmp/{}_out", stem);
        let cache = format!("/tmp/{}_cache", stem);
        let _ = fs::remove_dir_all(&cache);
        let status = Command::new(bin)
            .args(["build", path.to_str().unwrap(), "-o", &out, "--cache-dir", &cache])
            .status().unwrap();
        assert!(status.success(), "compile failed for {}", stem);
        let run = Command::new(&out).status().unwrap();
        assert_eq!(run.code(), Some(expected), "wrong output for {}", stem);
    }
}
