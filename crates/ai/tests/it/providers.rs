//! Models and providers for agents (SPEC-0350 AK-1 to AK-4), without a real provider: fake
//! servers on the loopback address.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;

use ferris_idea_ai::fake;
use ferris_idea_ai::providers::{
    self as prov, Booking, GroupBy, KeySource, Kind, Price, Provider, Register,
};
use ferris_idea_database::{MemoryStore, Secret, SecretStore};

const SECRET: &str = "sk-ant-abcdefghijklmnopqrstuvwxyz0123";

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "ferris-idea-providers-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(p.join("settings")).unwrap();
    std::fs::create_dir_all(p.join("workspace")).unwrap();
    p
}

fn provider(id: &str, kind: Kind, endpoint: &str) -> Provider {
    Provider {
        id: id.into(),
        kind,
        endpoint: endpoint.into(),
        key: KeySource::None,
        models: Vec::new(),
        enabled: true,
    }
}

fn price(provider: &str, model: &str, input: &str, output: &str) -> Price {
    Price {
        provider: provider.into(),
        model: model.into(),
        currency: "USD".into(),
        input_per_mtok: input.into(),
        output_per_mtok: output.into(),
    }
}

/// SPEC-0350 AK-1.
#[test]
fn register_and_its_bounds() {
    let root = scratch("bounds");
    let ws = root.join("workspace");
    let (file, ledger) = prov::files(&root.join("settings/agent-permissions.toml"));
    assert_eq!(file, root.join("settings/providers.toml"));
    assert_eq!(ledger, root.join("settings/model-usage.jsonl"));
    // A missing file is an empty register.
    let empty = prov::load(&file, &ws);
    assert!(empty.problem.is_none() && empty.register.providers.is_empty());

    let mut r = Register::default();
    let mut cloud = provider("cloud", Kind::Anthropic, "https://api.anthropic.com/v1");
    cloud.key = KeySource::Keychain;
    cloud.models = vec!["model-a".into(), "model-b".into()];
    r.set_provider(cloud).unwrap();
    r.set_provider(provider("local", Kind::Ollama, "http://127.0.0.1:11434/v1"))
        .unwrap();
    r.set_provider(provider(
        "lan-free",
        Kind::Openai,
        "http://localhost:8000/v1",
    ))
    .unwrap();
    r.set_price(price("cloud", "model-a", "3", "15.000001"))
        .unwrap();
    r.assign("default", "cloud", "model-a", 100).unwrap();
    prov::save(&file, &ws, &r).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("key = \"keychain\""), "{text}");
    let back = prov::load(&file, &ws);
    assert!(back.problem.is_none(), "{:?}", back.problem);
    assert_eq!(back.register, r);

    // Rules of the register.
    let refuse = |p: Provider, part: &str| {
        let e = r.clone().set_provider(p).unwrap_err();
        assert!(e.contains(part), "{e}");
        assert!(!e.contains(SECRET), "{e}");
    };
    refuse(
        provider("Bad Id", Kind::Openai, "https://x.example/v1"),
        "provider id",
    );
    refuse(
        provider("p", Kind::Openai, "http://api.example.com/v1"),
        "https",
    );
    refuse(
        provider("p", Kind::Ollama, "https://gpu.example.com/v1"),
        "local only",
    );
    refuse(
        provider("p", Kind::Llamacpp, "http://10.0.0.5:8080/v1"),
        "local only",
    );
    refuse(
        provider("p", Kind::Openai, "https://user:pw@api.example.com/v1"),
        "user name",
    );
    refuse(
        provider("p", Kind::Openai, "https://api.example.com/v1?key=1"),
        "query",
    );
    refuse(
        provider("p", Kind::Openai, "ftp://api.example.com"),
        "http or https",
    );
    let mut secret_model = provider("p", Kind::Openai, "https://api.example.com/v1");
    secret_model.models = vec![SECRET.into()];
    refuse(secret_model, "secret");
    let mut many = provider("p", Kind::Openai, "https://api.example.com/v1");
    many.models = (0..=prov::MAX_MODELS).map(|i| format!("m{i}")).collect();
    refuse(many, "at most");
    let mut full = Register::default();
    for i in 0..prov::MAX_PROVIDERS {
        full.set_provider(provider(
            &format!("p{i}"),
            Kind::Openai,
            "https://a.example/v1",
        ))
        .unwrap();
    }
    assert!(
        full.set_provider(provider("one-more", Kind::Openai, "https://a.example/v1"))
            .unwrap_err()
            .contains("at most")
    );

    // Prices out of bounds and for unknown providers.
    for (input, part) in [
        ("1.1234567", "6 decimal places"),
        ("-1", "decimal number"),
        ("1000001", "decimal number"),
        ("1.", "decimal number"),
        ("abc", "decimal number"),
    ] {
        let e = r
            .clone()
            .set_price(price("cloud", "*", input, "1"))
            .unwrap_err();
        assert!(e.contains(part), "{input}: {e}");
    }
    assert!(
        r.clone()
            .set_price(price("nobody", "*", "1", "1"))
            .unwrap_err()
            .contains("no such provider")
    );
    let mut eur = price("cloud", "*", "1", "1");
    eur.currency = "euro".into();
    assert!(r.clone().set_price(eur).unwrap_err().contains("currency"));
    assert_eq!(prov::parse_price("2.5").unwrap(), 2_500_000);
    assert_eq!(prov::show_micro(2_500_000), "2.500000");

    // Unknown keys, a vault reference and a secret in the file are problems, not repeated.
    for (bad, part) in [
        (
            "[[provider]]\nid = \"a\"\nkind = \"openai\"\nendpoint = \"https://a.example/v1\"\ncolour = 1\n",
            "unknown field",
        ),
        (
            "[[provider]]\nid = \"a\"\nkind = \"openai\"\nendpoint = \"https://a.example/v1\"\nkey = \"vault:main\"\n",
            "variant",
        ),
        (
            "[[provider]]\nid = \"a\"\nkind = \"robot\"\nendpoint = \"https://a.example/v1\"\n",
            "variant",
        ),
        (
            &format!(
                "[[provider]]\nid = \"a\"\nkind = \"openai\"\nendpoint = \"https://a.example/v1\"\nmodels = [\"{SECRET}\"]\n"
            ),
            "secret",
        ),
    ] {
        std::fs::write(&file, bad).unwrap();
        let loaded = prov::load(&file, &ws);
        let problem = loaded.problem.expect(bad);
        assert!(problem.contains(part), "{problem}");
        assert!(!problem.contains(SECRET), "{problem}");
        assert!(loaded.register.providers.is_empty());
    }

    // Inside the workspace: refused.
    let inside = ws.join("providers.toml");
    assert!(
        prov::save(&inside, &ws, &r)
            .unwrap_err()
            .contains("inside the workspace")
    );
    assert!(
        prov::load(&inside, &ws)
            .problem
            .unwrap()
            .contains("inside the workspace")
    );
    assert!(!inside.exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// A fake local server answering `GET` paths with fixed JSON.
fn fake_server(routes: &[(&str, &str)]) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes: Vec<(String, String)> = routes
        .iter()
        .map(|(p, b)| ((*p).to_owned(), (*b).to_owned()))
        .collect();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                    break;
                }
            }
            let path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_owned();
            let (status, body) = routes
                .iter()
                .find(|(p, _)| *p == path)
                .map_or((404, "{}".to_owned()), |(_, b)| (200, b.clone()));
            let mut stream = reader.into_inner();
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// SPEC-0350 AK-2.
#[test]
fn local_servers_are_detected_only() {
    let ollama = fake_server(&[(
        "/api/tags",
        r#"{"models":[{"name":"qwen3:8b"},{"name":"llama3.2:3b"}]}"#,
    )]);
    let llama = fake_server(&[("/v1/models", r#"{"data":[{"id":"local-gguf"}]}"#)]);
    let closed = closed_port();
    let mut r = Register::default();
    r.set_provider(provider(
        "mine",
        Kind::Ollama,
        &format!("http://localhost:{ollama}/v1/"),
    ))
    .unwrap();
    let before = r.clone();
    let found = prov::detect_local(&[ollama, llama, closed], &r).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].kind, Kind::Ollama);
    assert_eq!(found[0].models, ["llama3.2:3b", "qwen3:8b"]);
    assert_eq!(found[0].endpoint, format!("http://127.0.0.1:{ollama}/v1"));
    assert_eq!(found[0].registered.as_deref(), Some("mine"));
    assert_eq!(found[1].kind, Kind::Llamacpp);
    assert_eq!(found[1].models, ["local-gguf"]);
    assert_eq!(found[1].registered, None);
    assert_eq!(r, before, "detection changed the register");
    assert!(prov::detect_local(&[0], &r).is_err());
    assert!(prov::detect_local(&[1; 17], &r).is_err());
    assert_eq!(prov::LOCAL_PORTS, [11434, 8080]);
}

/// SPEC-0350 AK-3.
#[test]
fn models_and_assignments() {
    // The key goes to its own endpoint, and only with `keychain`.
    let server = fake::start("unused", 200).unwrap();
    let store = MemoryStore::default();
    let mut p = provider("srv", Kind::Openai, &server.endpoint);
    p.key = KeySource::Keychain;
    let entry = prov::key_entry(&p).unwrap();
    store.set(&entry, &Secret::new(SECRET)).unwrap();
    assert_eq!(
        prov::provider_models(&p, &store).unwrap(),
        ["model-a", "model-b"]
    );
    p.key = KeySource::None;
    prov::provider_models(&p, &store).unwrap();
    let seen = server.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some(&*format!("Bearer {SECRET}"))
    );
    assert_eq!(seen[1].authorization, None);
    assert!(seen.iter().all(|s| s.line.starts_with("GET /v1/models ")));
    assert_eq!(prov::key_reference(&p), "none");
    p.key = KeySource::Keychain;
    assert_eq!(
        prov::key_reference(&p),
        format!("keychain:{}", entry.account)
    );
    assert!(!prov::key_reference(&p).contains(SECRET));

    // An endpoint error that repeats the key is masked.
    let failing = fake::start(&format!("invalid key {SECRET}"), 401).unwrap();
    let mut q = provider("bad", Kind::Openai, &failing.endpoint);
    q.key = KeySource::Keychain;
    store
        .set(&prov::key_entry(&q).unwrap(), &Secret::new(SECRET))
        .unwrap();
    let e = prov::provider_models(&q, &store).unwrap_err().to_string();
    assert!(e.contains("401") && !e.contains(SECRET), "{e}");

    // Assignments: set, change with history, fall back to default, refuse unknown models.
    let mut r = Register::default();
    let mut cloud = provider("cloud", Kind::Openai, "https://api.example.com/v1");
    cloud.models = vec!["model-a".into(), "model-b".into()];
    r.set_provider(cloud).unwrap();
    r.set_provider(provider(
        "local",
        Kind::Llamacpp,
        "http://127.0.0.1:8080/v1",
    ))
    .unwrap();
    assert!(r.model_for("review").is_none());
    assert_eq!(r.assign("default", "local", "anything", 10).unwrap(), None);
    assert_eq!(r.model_for("review").unwrap().provider, "local");
    r.assign("review", "cloud", "model-a", 20).unwrap();
    let earlier = r.assign("review", "cloud", "model-b", 30).unwrap().unwrap();
    assert_eq!(
        (earlier.model.as_str(), earlier.changed_at),
        ("model-a", 20)
    );
    let a = r.model_for("review").unwrap();
    assert_eq!(
        (a.model.as_str(), a.changed_at, a.previous.len()),
        ("model-b", 30, 1)
    );
    for i in 0..25 {
        r.assign(
            "review",
            "cloud",
            if i % 2 == 0 { "model-a" } else { "model-b" },
            40 + i,
        )
        .unwrap();
    }
    assert_eq!(r.model_for("review").unwrap().previous.len(), prov::HISTORY);
    assert!(
        r.clone()
            .assign("review", "cloud", "model-z", 99)
            .unwrap_err()
            .contains("lists no model")
    );
    assert!(r.clone().assign("Bad Task", "local", "m", 99).is_err());
    assert!(r.clone().assign("t", "nobody", "m", 99).is_err());
    let mut off = r.clone();
    let mut disabled = provider("local", Kind::Llamacpp, "http://127.0.0.1:8080/v1");
    disabled.enabled = false;
    off.set_provider(disabled).unwrap();
    assert!(
        off.assign("t2", "local", "m", 99)
            .unwrap_err()
            .contains("switched off")
    );

    // A provider in use cannot be removed; its tasks are named.
    let e = r.clone().remove_provider("cloud").unwrap_err();
    assert!(e.contains("review"), "{e}");
    r.unassign("review").unwrap();
    r.set_price(price("cloud", "*", "1", "2")).unwrap();
    r.remove_provider("cloud").unwrap();
    assert!(r.prices.is_empty());
}

/// SPEC-0350 AK-4.
#[test]
fn costs_from_reported_tokens() {
    let root = scratch("costs");
    let ws = root.join("workspace");
    let (_, ledger) = prov::files(&root.join("settings/agent-permissions.toml"));
    let mut r = Register::default();
    let mut cloud = provider("cloud", Kind::Openai, "https://api.example.com/v1");
    cloud.key = KeySource::Keychain;
    r.set_provider(cloud).unwrap();
    r.set_provider(provider("local", Kind::Ollama, "http://127.0.0.1:11434/v1"))
        .unwrap();
    r.set_price(price("cloud", "big", "3", "15")).unwrap();
    r.set_price(price("cloud", "*", "0.15", "0.6")).unwrap();
    let booking =
        |provider: &'static str, model: &'static str, task: Option<&'static str>, i, o, at| {
            Booking {
                provider,
                model,
                task,
                input_tokens: i,
                output_tokens: o,
                at,
            }
        };
    // Exact price: 1M input at 3 + 200k output at 15 = 3 + 3 = 6.
    let exact = prov::price_booking(
        &r,
        &booking("cloud", "big", Some("review"), 1_000_000, 200_000, 10),
    )
    .unwrap();
    assert_eq!(
        (exact.currency.as_deref(), exact.cost_micro),
        (Some("USD"), Some(6_000_000))
    );
    assert!(
        exact.key.starts_with("keychain:api.example.com:443"),
        "{}",
        exact.key
    );
    // `*` price: 10k input at 0.15 + 1k output at 0.6 = 0.0015 + 0.0006.
    let star =
        prov::price_booking(&r, &booking("cloud", "small", None, 10_000, 1_000, 20)).unwrap();
    assert_eq!(star.cost_micro, Some(2_100));
    assert_eq!(star.task, "default");
    // No price: unknown, never guessed.
    let local = prov::price_booking(
        &r,
        &booking("local", "qwen3:8b", Some("review"), 500, 50, 30),
    )
    .unwrap();
    assert_eq!((local.currency.as_deref(), local.cost_micro), (None, None));
    assert_eq!(local.key, "none");
    assert!(prov::price_booking(&r, &booking("nobody", "m", None, 1, 1, 1)).is_err());
    assert!(
        prov::price_booking(&r, &booking("cloud", "m", None, prov::MAX_TOKENS + 1, 0, 1)).is_err()
    );

    for rec in [&exact, &star, &local] {
        prov::book(&ledger, &ws, rec, prov::MAX_RECORDS).unwrap();
    }
    // A later price change leaves the booked records alone.
    r.set_price(price("cloud", "big", "30", "150")).unwrap();
    let read = prov::read_ledger(&ledger);
    assert_eq!(
        read.records,
        vec![exact.clone(), star.clone(), local.clone()]
    );
    assert!(!std::fs::read_to_string(&ledger).unwrap().contains(SECRET));

    let by_provider = prov::summarize(&read.records, GroupBy::Provider, 0);
    assert_eq!(by_provider.len(), 2);
    assert_eq!(by_provider[0].group, "cloud");
    assert_eq!(by_provider[0].calls, 2);
    assert_eq!(
        by_provider[0].cost.get("USD").map(String::as_str),
        Some("6.002100")
    );
    assert_eq!(by_provider[1].unpriced_calls, 1);
    assert!(by_provider[1].cost.is_empty());
    let by_task = prov::summarize(&read.records, GroupBy::Task, 0);
    assert_eq!(
        by_task
            .iter()
            .map(|g| (g.group.as_str(), g.calls))
            .collect::<Vec<_>>(),
        [("default", 1), ("review", 2)]
    );
    let by_key = prov::summarize(&read.records, GroupBy::Key, 0);
    assert_eq!(by_key.len(), 2);
    let by_model = prov::summarize(&read.records, GroupBy::Model, 15);
    assert_eq!(by_model.len(), 2, "since drops the first record");

    // The bound drops the oldest records and says so.
    let small = root.join("settings/small.jsonl");
    for at in 0..5 {
        let mut rec = star.clone();
        rec.at = at;
        prov::book(&small, &ws, &rec, 3).unwrap();
    }
    let bounded = prov::read_ledger(&small);
    assert_eq!(bounded.records.len(), 3);
    assert_eq!(bounded.dropped, 2);
    assert_eq!(bounded.records[0].at, 2);
    assert!(prov::book(&ws.join("x.jsonl"), &ws, &star, 3).is_err());
    let _ = std::fs::remove_dir_all(&root);
}
