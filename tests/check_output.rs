use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn dataset_inputs() -> Vec<PathBuf> {
    let datasets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("datasets");
    let mut inputs = Vec::new();
    for entry in fs::read_dir(&datasets).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if name.ends_with(".csv") && name != "manifest.csv" && !name.ends_with(".expected.csv") {
            inputs.push(path);
        }
    }
    inputs.sort();
    inputs
}

fn client_key(line: &str) -> (u8, u128, String) {
    let field = line.split(',').next().unwrap_or("").trim();
    match field.parse::<u128>() {
        Ok(id) => (0, id, String::new()),
        Err(_) => (1, 0, field.to_owned()),
    }
}

fn ordered_report(text: &str) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let header = lines.remove(0).to_owned();
    lines.sort_by_key(|line| client_key(line));
    let mut ordered = vec![header];
    ordered.extend(lines.into_iter().map(str::to_owned));
    ordered
}

fn assert_reports_match(name: &str, expected: &str, actual: &str) {
    let expected = ordered_report(expected);
    let actual = ordered_report(actual);
    let mut mismatches = Vec::new();
    for index in 0..expected.len().max(actual.len()) {
        let expected_line = expected.get(index).map(String::as_str);
        let actual_line = actual.get(index).map(String::as_str);
        if expected_line != actual_line {
            mismatches.push(format!(
                "{name}:{} expected: {} actual: {}",
                index + 1,
                expected_line.unwrap_or("<no line>"),
                actual_line.unwrap_or("<no line>"),
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn dataset_reports_match_expected_output() {
    let binary = env!("CARGO_BIN_EXE_ledger");
    let inputs = dataset_inputs();
    assert!(!inputs.is_empty(), "no dataset inputs found");
    for input in inputs {
        let stem = input.file_stem().unwrap().to_str().unwrap();
        let expected_path = input.with_file_name(format!("{stem}.expected.csv"));
        let expected = fs::read_to_string(&expected_path)
            .unwrap_or_else(|err| panic!("{}: {err}", expected_path.display()));
        let output = Command::new(binary)
            .arg(&input)
            .output()
            .unwrap_or_else(|err| panic!("{stem}: failed to run {binary}: {err}"));
        assert!(
            output.status.success(),
            "{stem} exited {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        assert_reports_match(stem, &expected, &actual);
    }
}
