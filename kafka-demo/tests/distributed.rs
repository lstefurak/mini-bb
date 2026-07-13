//! Spec 003 acceptance 2: sharding preserves results. Runs the real binary
//! against a real broker and asserts the merged fan-out result equals a
//! monolithic spec-001 index of the same tree. Self-skips (passes) when
//! KAFKA_BROKER is unset so CI needs no broker.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

fn http_get(path: &str) -> Option<String> {
    let mut s = TcpStream::connect("127.0.0.1:7878").ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    buf.split_once("\r\n\r\n").map(|(_, body)| body.to_string())
}

fn monolithic_paths(root: &Path, q: &str) -> Vec<String> {
    let (docs, _) = mini_bb::ingest::ingest(root.to_str().unwrap()).unwrap();
    let idx = mini_bb::index::build("truth".into(), docs);
    let plans = mini_bb::query::plan(&mini_bb::query::parse(q));
    let cand = mini_bb::search::candidates(&idx, &plans);
    let mut paths: Vec<String> = mini_bb::search::verify(&idx, &plans, &cand)
        .iter()
        .flat_map(|m| m.doc.paths.iter().cloned())
        .collect();
    paths.sort();
    paths
}

#[test]
fn distributed_search_equals_monolithic() {
    if std::env::var("KAFKA_BROKER").is_err() {
        eprintln!("KAFKA_BROKER unset — skipping broker-dependent test");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_kafka-demo"))
        .current_dir(&root)
        .spawn()
        .expect("spawn kafka-demo");

    // Wait for the HTTP surface, trigger a push, then poll until the
    // eventually-consistent shards agree with the monolithic truth.
    let deadline = Instant::now() + Duration::from_secs(60);
    while http_get("/push").is_none() {
        assert!(Instant::now() < deadline, "demo never came up");
        std::thread::sleep(Duration::from_millis(500));
    }
    for q in ["fn+main", "arguments%3F"] {
        let plain = q.replace('+', " ").replace("%3F", "?");
        let want = monolithic_paths(&root, &plain);
        let got = loop {
            let body = http_get(&format!("/search?q={q}")).unwrap_or_default();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let mut got: Vec<String> = v["merged"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            got.sort();
            if got == want || Instant::now() > deadline {
                break got;
            }
            std::thread::sleep(Duration::from_secs(1));
        };
        assert_eq!(got, want, "distributed result diverged for {plain:?}");
        assert!(!want.is_empty(), "sanity: {plain:?} should match something");
    }
    let _ = child.kill();
    let _ = child.wait(); // reap — a killed child still needs wait()ing
}
