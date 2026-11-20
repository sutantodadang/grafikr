use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

struct Options {
    native: PathBuf,
    host: PathBuf,
    fixtures: PathBuf,
    output: PathBuf,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("grafikr-check-parity: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let options = parse_args()?;
    let fixtures = fs::read(&options.fixtures)?;
    let request_ids = validate_frames(&fixtures)?;
    let native = run_native(&options.native, &options.fixtures)?;
    let host = run_host(&options.host, &fixtures)?;
    if !has_responses(&native, &request_ids) || !has_responses(&host, &request_ids) {
        return Err("one transport omitted a fixture response".into());
    }
    let matched = transcripts_equal(&native, &host);
    let evidence = json!({
        "format": "grafikr-native-ipc-parity-v1",
        "fixtures": options.fixtures,
        "native_executable": options.native,
        "host_executable": options.host,
        "matched": matched,
        "native": native,
        "host": host,
    });
    if let Some(parent) = options.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.output, serde_json::to_vec_pretty(&evidence)?)?;
    if !matched {
        return Err(format!(
            "transports differ; evidence written to {}",
            options.output.display()
        )
        .into());
    }
    println!("native/IPC parity passed: {}", options.output.display());
    Ok(())
}

fn parse_args() -> Result<Options, Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut native = None;
    let mut host = None;
    let mut fixtures = None;
    let mut output = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value after {flag}"))?;
        match flag.as_str() {
            "--native" => native = Some(PathBuf::from(value)),
            "--host" => host = Some(PathBuf::from(value)),
            "--fixtures" => fixtures = Some(PathBuf::from(value)),
            "--out" => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown argument: {flag}").into()),
        }
    }
    Ok(Options {
        native: native.ok_or("--native is required")?,
        host: host.ok_or("--host is required")?,
        fixtures: fixtures.ok_or("--fixtures is required")?,
        output: output.ok_or("--out is required")?,
    })
}

fn validate_frames(bytes: &[u8]) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if bytes.is_empty() || !bytes.ends_with(b"\n") {
        return Err("fixture must contain complete LF-terminated JSONL frames".into());
    }
    let mut ids = Vec::new();
    for line in bytes
        .strip_suffix(b"\n")
        .unwrap()
        .split(|byte| *byte == b'\n')
    {
        if line.is_empty() {
            return Err("fixture contains an empty JSONL frame".into());
        }
        let request: Value = serde_json::from_slice(line)?;
        if !request.is_object() || !request["op"].is_string() {
            return Err("fixture lines must be JSON objects".into());
        }
        ids.push(
            request["id"]
                .as_str()
                .ok_or("fixture request id must be a string")?
                .to_owned(),
        );
    }
    if ids.is_empty() {
        return Err("fixture contains no JSONL requests".into());
    }
    Ok(ids)
}

fn has_responses(transcript: &[Value], request_ids: &[String]) -> bool {
    request_ids.iter().all(|id| {
        transcript.iter().any(|message| {
            message["kind"] == "response" && message["id"].as_str() == Some(id.as_str())
        })
    })
}

fn run_native(
    executable: &Path,
    fixtures: &Path,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let output = Command::new(executable)
        .arg("--scenario")
        .arg(fixtures)
        .stdin(Stdio::null())
        .output()?;
    parse_output("native", output)
}

fn run_host(executable: &Path, fixtures: &[u8]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .ok_or("host stdin was not piped")?
        .write_all(fixtures)?;
    drop(child.stdin.take());
    parse_output("host", child.wait_with_output()?)
}

fn parse_output(
    label: &str,
    output: std::process::Output,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    if !output.status.success() {
        return Err(format!(
            "{label} exited with {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    if !output.stderr.is_empty() {
        eprintln!(
            "{label} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout)?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn transcripts_equal(left: &[Value], right: &[Value]) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_requires_same_order_values_and_errors() {
        let first = json!([{"kind":"response","id":"1","ok":true,"result":{}},
                           {"kind":"event","event":"click","source":"2","payload":{}}]);
        assert!(transcripts_equal(
            first.as_array().unwrap(),
            first.as_array().unwrap()
        ));

        let reversed = json!([{"kind":"event","event":"click","source":"2","payload":{}},
                              {"kind":"response","id":"1","ok":true,"result":{}}]);
        assert!(!transcripts_equal(
            first.as_array().unwrap(),
            reversed.as_array().unwrap()
        ));
        let changed_error =
            json!([{"kind":"response","id":"1","ok":false,"error":{"code":"invalid_handle"}}]);
        let other_error =
            json!([{"kind":"response","id":"1","ok":false,"error":{"code":"wrong_handle_kind"}}]);
        assert!(!transcripts_equal(
            changed_error.as_array().unwrap(),
            other_error.as_array().unwrap()
        ));
    }

    #[test]
    fn fixture_requires_lf_terminated_json_objects() {
        assert!(validate_frames(b"{\"id\":\"1\",\"op\":\"shutdown\"}\n").is_ok());
        assert!(validate_frames(b"{\"id\":\"1\",\"op\":\"shutdown\"}").is_err());
        assert!(validate_frames(b"[]\n").is_err());
        assert!(validate_frames(b"{\"id\":\"1\",\"op\":\"shutdown\"}\n\n").is_err());
    }

    #[test]
    fn parity_requires_every_fixture_response() {
        let ids = vec!["1".to_owned(), "2".to_owned()];
        let response = json!([{"kind":"response","id":"1"}]);
        assert!(!has_responses(response.as_array().unwrap(), &ids));
    }
}
