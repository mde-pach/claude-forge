//! TypeSafe's Jev, the capability engine (§20.3): one `POST /v1/systemone` per prompt, with one
//! `noul` question per statement. A Noul answer is the probability that the statement is true of
//! the state, from 0 to 1 (https://docs.typesafe.ai/primitives/noul).
//!
//! The key goes in the `Authorization: Bearer` header and nowhere else: never on a command line,
//! in a log or in a note. No retry: a prompt waits for one request at most, bounded by
//! [`TIMEOUT`].

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::digest::Digest;
use crate::engine::{Engine, EngineError};

/// The API's base URL.
pub const BASE_URL: &str = "https://api.typesafe.ai";
/// The longest a prompt waits for the engine.
pub const TIMEOUT: Duration = Duration::from_millis(1500);
/// The sentence each question's instructions start with: what "the task" is.
pub const ABOUT: &str =
    "The task: the request, read in the light of the previous answer and the recent actions.";

pub struct Jev {
    key: String,
    model: String,
    base: String,
    agent: ureq::Agent,
}

impl Jev {
    /// A client with `key`, asking `model` (a versioned id) at `base` (the API's URL; another
    /// one only in tests).
    pub fn new(key: String, model: &str, base: &str) -> Jev {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .user_agent(concat!("claude-forge/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Jev {
            key,
            model: model.to_string(),
            base: base.trim_end_matches('/').to_string(),
            agent,
        }
    }

    /// The request body: the digest as the state, one noul question per statement.
    pub fn body(&self, digest: &Digest, questions: &[(String, &str)]) -> Value {
        let qs: Map<String, Value> = questions
            .iter()
            .map(|(k, s)| {
                (
                    k.clone(),
                    json!({"type": "noul", "instructions": {"about": ABOUT, "statement": s}}),
                )
            })
            .collect();
        json!({
            "model": self.model,
            "state": digest,
            "questions": qs,
        })
    }
}

/// The values of an answer, by question key; one that is missing or not a noul is left out.
pub fn read_answer(v: &Value) -> BTreeMap<String, f64> {
    v.get("answers")
        .and_then(Value::as_object)
        .map(|a| {
            a.iter()
                .filter_map(|(k, ans)| {
                    let n = ans.get("noul").and_then(Value::as_f64)?;
                    (0.0..=1.0).contains(&n).then(|| (k.clone(), n))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Engine for Jev {
    fn values(
        &self,
        digest: &Digest,
        questions: &[(String, &str)],
    ) -> Result<BTreeMap<String, f64>, EngineError> {
        if questions.is_empty() {
            return Ok(BTreeMap::new());
        }
        let body = self.body(digest, questions);
        let resp = self
            .agent
            .post(format!("{}/v1/systemone", self.base))
            .header("Authorization", format!("Bearer {}", self.key))
            .header("Content-Type", "application/json")
            .send(body.to_string());
        let mut resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => {
                return Err(EngineError::Environment(format!(
                    "the capability engine gave no answer within {} s",
                    TIMEOUT.as_secs_f64()
                )));
            }
            Err(e) => {
                return Err(EngineError::Environment(format!(
                    "the capability engine could not be reached ({e})"
                )));
            }
        };
        match resp.status().as_u16() {
            200 => {}
            401 => return Err(EngineError::Unauthorized),
            429 => {
                return Err(EngineError::Environment(
                    "the capability engine's rate limit was reached (429)".into(),
                ));
            }
            529 => {
                return Err(EngineError::Environment(
                    "the capability engine is overloaded (529)".into(),
                ));
            }
            s => {
                return Err(EngineError::Failure(format!(
                    "the capability engine refused the request ({s})"
                )));
            }
        }
        let text = resp.body_mut().read_to_string().map_err(|e| {
            EngineError::Environment(format!("the capability engine's answer was cut ({e})"))
        })?;
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            EngineError::Failure(format!("the capability engine's answer is not JSON ({e})"))
        })?;
        Ok(read_answer(&v))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    /// A one-request HTTP server: answers `status` and `body` after `delay`, and hands back the
    /// request it read.
    fn serve(status: u16, body: &'static str, delay: Duration) -> (String, mpsc::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let mut req = Vec::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&req);
                if let Some(i) = t.find("\r\n\r\n") {
                    let len = t[..i]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if req.len() >= i + 4 + len {
                        break;
                    }
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&req).into_owned());
            std::thread::sleep(delay);
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        (url, rx)
    }

    fn digest() -> Digest {
        Digest {
            request: "Fix the login page".into(),
            previous_answer: None,
            recent_actions: Vec::new(),
        }
    }

    // One request: the pinned model, the digest as the state, one noul question per statement
    // keyed `<id>#<n>` with the fixed `about` sentence; the key in the Authorization header.
    #[test]
    fn one_request_with_a_noul_question_per_statement() {
        let (url, req) = serve(
            200,
            r#"{"model":"jev-1.14.0","answers":{"a/web#0":{"type":"noul","noul":0.92},"a/web#1":{"type":"choice","choice":"x"}},"usage":{"input_tokens":3,"output_tokens":1}}"#,
            Duration::ZERO,
        );
        let jev = Jev::new("sk-test".into(), "jev-1.14.0", &url);
        let v = jev
            .values(
                &digest(),
                &[
                    ("a/web#0".into(), "The task changes a page."),
                    ("a/web#1".into(), "Other."),
                ],
            )
            .unwrap();
        assert_eq!(v, BTreeMap::from([("a/web#0".to_string(), 0.92)]));
        let req = req.recv().unwrap();
        assert!(req.starts_with("POST /v1/systemone HTTP/1.1\r\n"), "{req}");
        assert!(
            req.to_ascii_lowercase()
                .contains("authorization: bearer sk-test\r\n"),
            "{req}"
        );
        let body: Value = serde_json::from_str(req.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(
            body,
            json!({"model": "jev-1.14.0", "state": {"request": "Fix the login page"},
                "questions": {
                    "a/web#0": {"type": "noul", "instructions": {"about": ABOUT, "statement": "The task changes a page."}},
                    "a/web#1": {"type": "noul", "instructions": {"about": ABOUT, "statement": "Other."}}}})
        );
    }

    // 401: unauthorized; 429 and 529: the environment; another status: a failure; no answer in
    // 1.5 s: the environment. No retry.
    #[test]
    fn failures_by_kind() {
        let q = [("a/web#0".to_string(), "S.")];
        let ask = |status, delay| {
            let (url, _rx) = serve(status, "{}", delay);
            Jev::new("k".into(), "jev-1.14.0", &url).values(
                &digest(),
                &q.iter().map(|(k, s)| (k.clone(), *s)).collect::<Vec<_>>(),
            )
        };
        assert_eq!(ask(401, Duration::ZERO), Err(EngineError::Unauthorized));
        assert!(
            matches!(ask(429, Duration::ZERO), Err(EngineError::Environment(e)) if e.contains("429"))
        );
        assert!(
            matches!(ask(529, Duration::ZERO), Err(EngineError::Environment(e)) if e.contains("529"))
        );
        assert!(
            matches!(ask(422, Duration::ZERO), Err(EngineError::Failure(e)) if e.contains("422"))
        );
        let started = std::time::Instant::now();
        assert!(
            matches!(ask(200, Duration::from_secs(3)), Err(EngineError::Environment(e)) if e.contains("within 1.5 s"))
        );
        assert!(started.elapsed() < Duration::from_millis(2500));
        // nothing listening: the environment
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        assert!(matches!(
            Jev::new("k".into(), "m", &url).values(&digest(), &[("a#0".into(), "S.")]),
            Err(EngineError::Environment(_))
        ));
    }
}
