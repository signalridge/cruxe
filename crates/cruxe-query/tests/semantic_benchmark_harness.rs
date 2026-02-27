use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

#[test]
fn semantic_benchmark_harness_generates_report() {
    let root = repo_root();
    let script = root
        .join("benchmarks")
        .join("semantic")
        .join("run_semantic_benchmarks.sh");
    assert!(
        script.exists(),
        "benchmark harness script is missing: {}",
        script.display()
    );

    let output_dir = root.join("target").join("semantic-benchmark-test");
    std::fs::create_dir_all(&output_dir).expect("failed to create harness output dir");

    let output = Command::new("bash")
        .arg(&script)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("failed to execute semantic benchmark harness");
    assert!(
        output.status.success(),
        "harness failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let first_report_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !first_report_path.is_empty(),
        "harness must print report path to stdout"
    );

    let reports: Vec<PathBuf> = std::fs::read_dir(&output_dir)
        .expect("failed to list harness output dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("report-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    assert!(
        !reports.is_empty(),
        "expected at least one report file in {}",
        output_dir.display()
    );

    let report = std::fs::read_to_string(&reports[0])
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", reports[0].display()));
    let parsed: serde_json::Value = serde_json::from_str(&report)
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", reports[0].display()));

    assert_eq!(
        parsed["version"].as_str(),
        Some("semantic-benchmark-report-v1")
    );
    assert!(
        parsed["inputs"]["run_key"].as_str().is_some(),
        "report must include deterministic run_key"
    );
    assert!(
        parsed["inputs"]["fixtures_sha256"].as_str().is_some(),
        "report must include fixtures hash"
    );
    assert!(
        parsed["inputs"]["query_pack_sha256"].as_str().is_some(),
        "report must include query pack hash"
    );
    assert!(
        parsed["inputs"]["git_revision"].as_str().is_some(),
        "report must include git revision"
    );

    // Re-running with identical inputs should keep report path and content stable.
    std::thread::sleep(Duration::from_millis(1100));
    let rerun = Command::new("bash")
        .arg(&script)
        .arg("--output")
        .arg(&output_dir)
        .output()
        .expect("failed to re-execute semantic benchmark harness");
    assert!(
        rerun.status.success(),
        "rerun failed: stdout={} stderr={}",
        String::from_utf8_lossy(&rerun.stdout),
        String::from_utf8_lossy(&rerun.stderr)
    );
    let second_report_path = String::from_utf8_lossy(&rerun.stdout).trim().to_string();
    assert_eq!(
        first_report_path, second_report_path,
        "rerun should keep deterministic report path"
    );

    let rerun_report = std::fs::read_to_string(&reports[0])
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", reports[0].display()));
    assert_eq!(
        report, rerun_report,
        "report content should remain stable when inputs are unchanged"
    );
}
