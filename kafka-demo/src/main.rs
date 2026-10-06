//! Distributed mini-bb (spec 003): the Blackbird pipeline in miniature.
//!
//! Ingest is ASYNC via real Kafka messages: a push event on the `pushes`
//! topic wakes the crawler, which produces one document message per file to
//! the `docs` topic, partitioned by content hash — and each of the three
//! shards consumes exactly one partition, building its own spec-001 index.
//! Queries are SYNC: HTTP fan-out to every shard, results merged. That
//! ingest/query asymmetry is the whole lesson (see the spec's blog quotes).

use mini_bb::{fnv1a, index, ingest, query, search, Doc, Index};
use rskafka::client::partition::{Compression, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder};
use rskafka::record::Record;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};

const SHARDS: usize = 3;

// [ASYNC] Everything below runs on tokio, Rust's async runtime. Unlike
// Python's asyncio (built into the interpreter) or C#'s Task machinery
// (built into the CLR), Rust ships *no* runtime — async/await compiles to
// state machines and a library crate schedules them. `#[tokio::main]`
// installs that scheduler around main.
#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Every pipeline step is emitted as one JSON line: to SSE subscribers
/// (DEMO-5) and, with --record, to the replay file used by the web UI.
struct Emitter {
    tx: broadcast::Sender<String>,
    rec: Option<Mutex<std::fs::File>>,
}

impl Emitter {
    fn emit(&self, ev: &str, mut data: Value) {
        data["ev"] = ev.into();
        data["ts"] = chrono::Utc::now().timestamp_millis().into();
        let line = data.to_string();
        // [ERRORS] Both sends are allowed to fail silently: no SSE
        // subscriber and a full channel are normal, not bugs. The explicit
        // `let _ =` is Rust's way of saying "I considered this Result and
        // chose to drop it" — an unused Result is a compiler warning.
        let _ = self.tx.send(line.clone());
        if let Some(f) = &self.rec {
            use std::io::Write;
            if let Ok(mut f) = f.lock() {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// One shard = one Kafka partition = one independent spec-001 index (DEMO-3).
#[derive(Default)]
struct Shard {
    index: Option<Index>,
}

type Shards = Arc<Vec<RwLock<Shard>>>;

async fn run() -> mini_bb::Result<()> {
    let broker = std::env::var("KAFKA_BROKER").unwrap_or_else(|_| "localhost:9092".into());
    let record = std::env::args()
        .skip_while(|a| a != "--record")
        .nth(1)
        .map(std::fs::File::create)
        .transpose()?;
    let emitter = Arc::new(Emitter {
        tx: broadcast::channel(512).0,
        rec: record.map(Mutex::new),
    });

    let client = Arc::new(ClientBuilder::new(vec![broker.clone()]).build().await?);
    ensure_topic(&client, "pushes", 1).await?;
    ensure_topic(&client, "docs", SHARDS as i32).await?;
    let pushes = Arc::new(partition(&client, "pushes", 0).await?);
    let mut docs_parts = Vec::new();
    for p in 0..SHARDS {
        docs_parts.push(Arc::new(partition(&client, "docs", p as i32).await?));
    }

    let shards: Shards = Arc::new((0..SHARDS).map(|_| RwLock::new(Shard::default())).collect());

    // [OWNERSHIP] Each spawned task gets its own Arc clone — shared
    // ownership with an atomic refcount (like Python objects or C#
    // references, but opt-in and visible). The compiler rejects sharing
    // plain `&` references across tasks that may outlive this function.
    tokio::spawn(crawler(pushes.clone(), docs_parts.clone(), emitter.clone()));
    for (i, pc) in docs_parts.iter().enumerate() {
        tokio::spawn(shard_consumer(
            i,
            pc.clone(),
            shards.clone(),
            emitter.clone(),
        ));
    }

    println!("kafka-demo up: broker {broker}, http://localhost:7878 (/push /search?q= /events)");
    serve(pushes, shards, emitter).await
}

async fn ensure_topic(client: &Client, name: &str, partitions: i32) -> mini_bb::Result<()> {
    match client
        .controller_client()?
        .create_topic(name, partitions, 1, 5_000)
        .await
    {
        Ok(()) => Ok(()),
        // A pre-existing topic (previous run) is fine; anything else is not.
        Err(e) if e.to_string().contains("already exists") => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn partition(client: &Client, topic: &str, p: i32) -> mini_bb::Result<PartitionClient> {
    Ok(client
        .partition_client(topic, p, UnknownTopicHandling::Retry)
        .await?)
}

fn record(value: Vec<u8>) -> Record {
    Record {
        key: None,
        value: Some(value),
        headers: Default::default(),
        timestamp: chrono::Utc::now(),
    }
}

/// DEMO-2: consume `pushes`, crawl the repo with the spec-001 ingester, and
/// produce every doc to the partition its *content hash* selects — the same
/// content-addressed sharding Blackbird does with git blob SHAs.
async fn crawler(
    pushes: Arc<PartitionClient>,
    docs_parts: Vec<Arc<PartitionClient>>,
    emitter: Arc<Emitter>,
) {
    let mut offset = 0;
    loop {
        // Long-poll fetch: returns early when a record arrives.
        let Ok((records, _)) = pushes.fetch_records(offset, 1..1_000_000, 5_000).await else {
            continue;
        };
        for r in records {
            offset = r.offset + 1;
            emitter.emit("push", json!({ "repo": "." }));
            // [ASYNC] File I/O is blocking; `spawn_blocking` moves it to a
            // thread pool so the async scheduler isn't stalled — the same
            // reason Python has loop.run_in_executor. The error is mapped
            // to String because `Box<dyn Error>` isn't `Send` (thread-safe
            // to move) — a constraint Python/C# never surface.
            let Ok(Ok((docs, stats))) =
                tokio::task::spawn_blocking(|| ingest::ingest(".").map_err(|e| e.to_string()))
                    .await
            else {
                continue;
            };
            emitter.emit(
                "crawl",
                json!({ "docs": stats.indexed, "merged": stats.merged }),
            );
            // Batch: one produce request per partition, not per doc —
            // amortizing broker round-trips is the whole point of Kafka
            // producers (real clients batch with linger.ms for the same
            // reason).
            let mut batches: Vec<Vec<Record>> = (0..SHARDS).map(|_| Vec::new()).collect();
            for doc in &docs {
                let part = (fnv1a(doc.content.as_bytes()) % SHARDS as u64) as usize;
                let Ok(value) = serde_json::to_vec(&doc) else {
                    continue;
                };
                batches[part].push(record(value));
                emitter.emit(
                    "produce",
                    json!({ "path": doc.paths[0], "partition": part }),
                );
            }
            for (part, batch) in batches.into_iter().enumerate() {
                if !batch.is_empty() {
                    let _ = docs_parts[part]
                        .produce(batch, Compression::NoCompression)
                        .await;
                }
            }
        }
    }
}

/// DEMO-3: shard i consumes ONLY partition i and rebuilds its own index
/// after each batch — "each shard consumes a single Kafka partition".
async fn shard_consumer(i: usize, pc: Arc<PartitionClient>, shards: Shards, emitter: Arc<Emitter>) {
    let mut offset = 0;
    loop {
        let Ok((records, _)) = pc.fetch_records(offset, 1..1_000_000, 5_000).await else {
            continue;
        };
        if records.is_empty() {
            continue;
        }
        let mut shard = shards[i].write().await;
        // Reclaim the docs from the previous index build; ids are LOCAL to
        // the shard (docs[id].id == id must hold within each shard's index).
        let mut docs = shard.index.take().map(|ix| ix.docs).unwrap_or_default();
        for r in records {
            offset = r.offset + 1;
            let Some(value) = r.record.value else {
                continue;
            };
            let Ok(mut doc) = serde_json::from_slice::<Doc>(&value) else {
                continue;
            };
            // Re-pushes re-produce every doc; content addressing makes the
            // dedupe trivial (ingest's FR-3 trick again, at shard level).
            if docs.iter().any(|d| d.hash == doc.hash) {
                continue;
            }
            doc.id = docs.len() as u32;
            docs.push(doc);
        }
        let ix = index::build(format!("shard-{i}"), docs);
        emitter.emit(
            "indexed",
            json!({ "shard": i, "docs": ix.docs.len(), "grams": ix.grams.len() }),
        );
        shard.index = Some(ix);
    }
}

/// DEMO-4: queries do NOT go through Kafka. Parse+plan once, fan out to all
/// shards, merge — the synchronous half of Blackbird's architecture.
async fn fan_out(q: &str, shards: &Shards, emitter: &Emitter) -> Value {
    let plans = query::plan(&query::parse(q));
    emitter.emit("fanout", json!({ "q": q, "shards": SHARDS }));
    let mut merged = Vec::new();
    let mut per_shard = Vec::new();
    for (i, sh) in shards.iter().enumerate() {
        let guard = sh.read().await;
        let mut paths: Vec<String> = guard
            .index
            .as_ref()
            .map(|ix| {
                let cand = search::candidates(ix, &plans);
                search::verify(ix, &plans, &cand)
                    .iter()
                    .flat_map(|m| m.doc.paths.iter().cloned())
                    .collect()
            })
            .unwrap_or_default();
        paths.sort();
        emitter.emit(
            "shard_result",
            json!({ "shard": i, "matches": paths.len() }),
        );
        per_shard.push(json!({ "shard": i, "paths": paths.clone() }));
        merged.extend(paths);
    }
    merged.sort();
    emitter.emit("merged", json!({ "q": q, "total": merged.len() }));
    json!({ "q": q, "shards": per_shard, "merged": merged })
}

/// DEMO-6: the tiny HTTP surface — /push, /search?q=, /events (SSE).
async fn serve(
    pushes: Arc<PartitionClient>,
    shards: Shards,
    emitter: Arc<Emitter>,
) -> mini_bb::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:7878").await?;
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle(
            stream,
            pushes.clone(),
            shards.clone(),
            emitter.clone(),
        ));
    }
}

async fn handle(
    mut s: TcpStream,
    pushes: Arc<PartitionClient>,
    shards: Shards,
    emitter: Arc<Emitter>,
) {
    let mut buf = [0u8; 2048];
    let Ok(n) = s.read(&mut buf).await else {
        return;
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    let target = head.split_whitespace().nth(1).unwrap_or("/").to_string();
    const OK: &str = "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\n";
    if target == "/events" {
        let hdr = format!("{OK}Content-Type: text/event-stream\r\nCache-Control: no-cache\r\n\r\n");
        if s.write_all(hdr.as_bytes()).await.is_err() {
            return;
        }
        let mut rx = emitter.tx.subscribe();
        // [ENUMS+MATCH] recv() yields Result; Lagged (slow consumer) is a
        // recoverable variant we skip past, Closed ends the stream. The
        // enum forces the "what if I'm too slow?" question at compile time.
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if s.write_all(format!("data: {msg}\n\n").as_bytes())
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }
    let body = if target == "/push" {
        match pushes
            .produce(vec![record(b"push".to_vec())], Compression::NoCompression)
            .await
        {
            Ok(_) => json!({ "ok": true }).to_string(),
            Err(e) => json!({ "ok": false, "error": e.to_string() }).to_string(),
        }
    } else if let Some(qs) = target.strip_prefix("/search?q=") {
        fan_out(&percent_decode(qs), &shards, &emitter)
            .await
            .to_string()
    } else {
        let _ = s
            .write_all(b"HTTP/1.1 404 Not Found\r\nAccess-Control-Allow-Origin: *\r\n\r\n")
            .await;
        return;
    };
    let resp = format!(
        "{OK}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = s.write_all(resp.as_bytes()).await;
}

/// Minimal URL decoding: `+` → space, `%XX` → byte. Enough for query text.
fn percent_decode(s: &str) -> String {
    let mut out = Vec::new();
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        match b {
            b'+' => out.push(b' '),
            b'%' => {
                let hi = bytes.next().unwrap_or(b'0');
                let lo = bytes.next().unwrap_or(b'0');
                let hex = [hi, lo];
                let hex = std::str::from_utf8(&hex).unwrap_or("30");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(b'?'));
            }
            b => out.push(b),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("fn+main"), "fn main");
        assert_eq!(percent_decode("%22fn%20main%22"), "\"fn main\"");
        assert_eq!(percent_decode("colo(u%7C)r"), "colo(u|)r");
    }

    #[test]
    fn partitioning_is_stable_and_in_range() {
        for content in ["hello", "world", ""] {
            let p = (fnv1a(content.as_bytes()) % SHARDS as u64) as usize;
            assert!(p < SHARDS);
            assert_eq!(p, (fnv1a(content.as_bytes()) % SHARDS as u64) as usize);
        }
    }
}
