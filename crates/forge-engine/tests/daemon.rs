//! The daemon without servers: sessions, failures as notes, idle shutdown.

use std::time::{Duration, Instant};

use forge_core::NoteKind;
use forge_engine::{Client, EngineConfig, Lang, PythonConfig, ensure, judge, serve};

fn up(config: &EngineConfig) -> Client {
    let c = config.clone();
    ensure(config, None, Duration::from_secs(10), move || {
        std::thread::spawn(move || serve(c).unwrap());
        Ok(())
    })
    .unwrap()
}

fn gone(client: &Client, within: Duration) -> bool {
    let until = Instant::now() + within;
    while Instant::now() < until {
        if client.ping().is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_server_that_cannot_start_is_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::new(dir.path(), dir.path().join("e.sock"));
    config.python = Some(PythonConfig {
        ty: vec!["/nonexistent/ty".into()],
        ..PythonConfig::default()
    });
    let client = up(&config);
    let answer = client.types(Lang::Python, &[]);
    assert!(answer.is_err());
    let m: forge_engine::Match = serde_json::from_str(
        r#"{"file": "a.py", "rule": "python/x", "line": 1, "bindings": {}, "where": {}}"#,
    )
    .unwrap();
    let (keep, notes) = judge(Lang::Python, &[m], &answer, false);
    assert_eq!(keep, [false]);
    assert_eq!(notes[0].kind, NoteKind::ForgeFailure);
    assert!(client.ping().unwrap().failed.contains_key(&Lang::Python));
    client.stop().unwrap();
    assert!(gone(&client, Duration::from_secs(2)));
}

#[test]
fn sessions_and_idle() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = EngineConfig::new(dir.path(), dir.path().join("s.sock"));
    config.want = "w1".into();
    let client = up(&config);
    client.register("a").unwrap();
    client.register("b").unwrap();
    assert_eq!(client.ping().unwrap().sessions, 2);
    assert!(!client.unregister("a").unwrap());
    assert!(client.unregister("b").unwrap());
    assert!(gone(&client, Duration::from_secs(5)));

    // another want: the running daemon is replaced
    let client = up(&config);
    config.want = "w2".into();
    let client2 = up(&config);
    assert_eq!(client2.ping().unwrap().want, "w2");
    client.stop().unwrap();

    config.idle_secs = 1;
    config.want = "w3".into();
    let client = up(&config);
    // a ping is a request too: none for longer than the idle time
    std::thread::sleep(Duration::from_millis(2000));
    assert!(client.ping().is_err(), "no idle exit");
}
