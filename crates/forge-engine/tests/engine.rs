//! The types engine against the real servers: ty (the pinned 0.0.86, through uvx) and the project's
//! TypeScript (5, 6 and 7, installed with bun), on the pack examples of rules with `where`.
//! Skipped (with a message) when uvx or bun is missing or offline.
//!
//! Run with `--nocapture` to see the timings.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use forge_engine::{
    Binding, Client, Condition, DiagFile, EngineConfig, Lang, Match, PythonConfig,
    TypeScriptConfig, Verdict, ensure,
};

const TY: &str = "0.0.86";

fn cache() -> PathBuf {
    let d = std::env::var_os("FORGE_ENGINE_TEST_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("forge-engine"));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(cmd: &mut Command) -> Option<String> {
    let out = cmd.output().ok()?;
    if !out.status.success() {
        eprintln!(
            "{:?} failed: {}",
            cmd,
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        );
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// The pinned ty's executable (uvx's environment for it).
fn ty() -> Option<PathBuf> {
    let dir = run(Command::new("uvx").args([
        "-q",
        "--from",
        &format!("ty=={TY}"),
        "python",
        "-c",
        "import os, sys; print(os.path.dirname(sys.executable))",
    ]))?;
    let ty = Path::new(&dir).join("ty");
    ty.is_file().then_some(ty)
}

/// A Python project with a `.venv` holding SQLAlchemy 2.0.44.
fn python_project() -> Option<PathBuf> {
    let d = cache().join("py-project");
    if !d.join(".venv/pyvenv.cfg").exists() {
        std::fs::create_dir_all(&d).ok()?;
        run(Command::new("uv")
            .args(["venv", "-q", "--python", "3.12", ".venv"])
            .current_dir(&d))?;
        run(Command::new("uv")
            .args([
                "pip",
                "install",
                "-q",
                "--python",
                ".venv",
                "sqlalchemy==2.0.44",
            ])
            .current_dir(&d))?;
    }
    std::fs::canonicalize(d).ok()
}

/// A TypeScript project with `typescript@<version>` and the examples' packages.
fn ts_project(version: &str) -> Option<PathBuf> {
    let d = cache().join(format!("ts-{version}"));
    if !d.join("node_modules/typescript/package.json").exists()
        || !d.join("node_modules/ioredis").exists()
    {
        std::fs::create_dir_all(&d).ok()?;
        std::fs::write(
            d.join("package.json"),
            "{\"name\": \"fixture\", \"private\": true}\n",
        )
        .ok()?;
        run(Command::new("bun")
            .args([
                "add",
                "-d",
                &format!("typescript@{version}"),
                "@types/node@24.10.1",
                "ioredis@6.0.0",
                "redis@6.3.0",
            ])
            .current_dir(&d))?;
    }
    std::fs::write(
        d.join("tsconfig.json"),
        r#"{"compilerOptions": {"strict": true, "target": "es2022", "module": "nodenext", "moduleResolution": "nodenext", "types": ["node"], "skipLibCheck": true, "noEmit": true}}"#,
    )
    .ok()?;
    std::fs::canonicalize(d).ok()
}

/// A case: a file, the metavariables a match binds (`«…»` marks the bound code in `at`), the
/// condition of each, and the verdict expected.
struct Case {
    name: &'static str,
    text: &'static str,
    vars: Vec<(&'static str, &'static str, &'static str)>,
    expect: &'static str,
}

fn case(
    name: &'static str,
    text: &'static str,
    vars: &[(&'static str, &'static str, &'static str)],
    expect: &'static str,
) -> Case {
    Case {
        name,
        text,
        vars: vars.to_vec(),
        expect,
    }
}

fn to_match(file: &str, c: &Case) -> Match {
    let mut bindings = BTreeMap::new();
    let mut conditions = BTreeMap::new();
    let mut line = 0;
    for (var, at, cond) in &c.vars {
        let plain = at.replace(['«', '»'], "");
        let base = c
            .text
            .find(&plain)
            .unwrap_or_else(|| panic!("{}: {plain} not in the example", c.name));
        let s = base + at.find('«').unwrap();
        let e = base + at.find('»').unwrap() - '«'.len_utf8();
        line = c.text[..s].matches('\n').count() as u32 + 1;
        bindings.insert(
            (*var).to_owned(),
            Binding {
                start: s,
                end: e,
                text: c.text[s..e].to_owned(),
            },
        );
        conditions.insert(
            (*var).to_owned(),
            serde_json::from_str::<Condition>(cond).unwrap(),
        );
    }
    Match {
        file: file.to_owned(),
        rule: format!("test/{}", c.name),
        line,
        bindings,
        conditions,
    }
}

fn verdict_name(v: &Verdict) -> &'static str {
    match v {
        Verdict::Holds => "holds",
        Verdict::Fails => "fails",
        Verdict::Unchecked { .. } => "unchecked",
    }
}

/// Starts the daemon (in a thread of this process) and waits until its server for `lang` is ready.
fn start(config: EngineConfig, lang: Lang) -> (Client, Duration) {
    let t = Instant::now();
    let c = config.clone();
    let client = ensure(&config, Some("s1"), Duration::from_secs(120), move || {
        std::thread::spawn(move || forge_engine::serve(c).unwrap());
        Ok(())
    })
    .unwrap();
    loop {
        let s = client.ping().unwrap();
        if let Some(why) = s.failed.get(&lang) {
            panic!("{lang} did not start: {why}");
        }
        if s.servers.contains_key(&lang) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    (client.with_timeout(Duration::from_secs(120)), t.elapsed())
}

fn socket(name: &str) -> PathBuf {
    // a long path: the daemon falls back to a short one in $TMPDIR
    let d = cache().join("sockets").join("x".repeat(60));
    std::fs::create_dir_all(&d).unwrap();
    d.join(format!("{name}-{}.sock", std::process::id()))
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Asks every case (one file each, under `dir`), checks the verdicts, then measures warm
/// answers: an edit of the first file (a `changed` notification) and its question again.
fn ask_all(client: &Client, lang: Lang, root: &Path, dir: &str, ext: &str, cases: &[Case]) {
    let mut matches = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let file = format!("{dir}/case_{i}_{}.{ext}", c.name.replace('-', "_"));
        std::fs::create_dir_all(root.join(dir)).unwrap();
        std::fs::write(root.join(&file), c.text).unwrap();
        matches.push(to_match(&file, c));
    }
    let paths: Vec<String> = matches.iter().map(|m| m.file.clone()).collect();
    client.changed(&paths).unwrap();
    let t = Instant::now();
    let verdicts = client.types(lang, &matches).unwrap();
    let first = t.elapsed();
    let mut wrong = Vec::new();
    for (c, v) in cases.iter().zip(&verdicts) {
        if verdict_name(v) != c.expect {
            wrong.push(format!("{}: expected {}, got {v:?}", c.name, c.expect));
        }
    }
    // warm: the file of one match edited (a comment appended), then asked again
    let mut warm = Vec::new();
    for n in 0..10 {
        let m = &matches[0];
        let p = root.join(&m.file);
        let text = format!(
            "{}{} edit {n}\n",
            cases[0].text,
            if ext == "py" { "#" } else { "//" }
        );
        std::fs::write(&p, text).unwrap();
        let t = Instant::now();
        client.changed(std::slice::from_ref(&m.file)).unwrap();
        let v = client.types(lang, std::slice::from_ref(m)).unwrap();
        warm.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(verdict_name(&v[0]), cases[0].expect);
    }
    std::fs::write(root.join(&matches[0].file), cases[0].text).unwrap();
    let t = Instant::now();
    let _ = client.types(lang, &matches).unwrap();
    let all_warm = t.elapsed();
    eprintln!(
        "{lang} ({}): {} matches: first {:.0} ms, all again {:.0} ms; one match after an edit: median {:.1} ms, max {:.1} ms",
        client.ping().unwrap().servers[&lang],
        matches.len(),
        first.as_secs_f64() * 1000.0,
        all_warm.as_secs_f64() * 1000.0,
        median(warm.clone()),
        warm.iter().cloned().fold(0.0, f64::max),
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(median(warm) < 300.0, "warm answers are over budget");
}

fn python_cases() -> Vec<Case> {
    let hmac_m0 = "import hashlib\nimport hmac\n\n\ndef valid(secret: bytes, body: bytes, sig: str) -> bool:\n    mac = hmac.new(secret, body, hashlib.sha256)\n    return mac.hexdigest() == sig\n";
    let hmac_m1 = "import hmac\n\n\ndef invalid(secret: bytes, body: bytes, sig: bytes) -> bool:\n    return sig != hmac.digest(secret, body, \"sha256\")\n";
    let hmac_nm1 = "import hashlib\n\n\ndef same(a: bytes, b: str) -> bool:\n    return hashlib.sha256(a).hexdigest() == b\n";
    let hmac_t = r#"{"type": {"module": "hmac", "name": "HMAC"}}"#;
    let hmac_d = r#"{"declared": {"module": "hmac", "name": "digest"}}"#;
    let tar_t = r#"{"type": {"module": "tarfile", "name": "TarFile"}}"#;
    let unpack_d = r#"{"declared": {"module": "shutil", "name": "unpack_archive"}}"#;
    let sa = r#"{"type": {"package": "sqlalchemy", "name": ["Session", "scoped_session", "AsyncSession", "async_scoped_session"]}}"#;
    vec![
        case(
            "hmac-type",
            hmac_m0,
            &[("H", "return «mac».hexdigest", hmac_t)],
            "holds",
        ),
        case(
            "hmac-declared",
            hmac_m1,
            &[("D", "«hmac.digest»(secret", hmac_d)],
            "holds",
        ),
        case(
            "hmac-other-digest",
            hmac_nm1,
            &[("H", "«hashlib.sha256(a)».hexdigest", hmac_t)],
            "fails",
        ),
        case(
            "tar-open",
            "import tarfile\n\n\ndef unpack(path: str) -> None:\n    with tarfile.open(path) as tar:\n        tar.extractall(\"out\")\n",
            &[("T", "«tar».extractall", tar_t)],
            "holds",
        ),
        case(
            "tar-param",
            "import tarfile\n\n\ndef first(tar: tarfile.TarFile) -> None:\n    tar.extract(tar.getmembers()[0], path=\"out\")\n",
            &[("T", "«tar».extract(", tar_t)],
            "holds",
        ),
        case(
            "unpack-alias",
            "import shutil as sh\n\nsh.unpack_archive(\"upload.tar.gz\", \"out\")\n",
            &[("U", "«sh.unpack_archive»(", unpack_d)],
            "holds",
        ),
        case(
            "zip",
            "import zipfile\n\n\ndef unpack(path: str) -> None:\n    with zipfile.ZipFile(path) as z:\n        z.extractall(\"out\")\n",
            &[("T", "«z».extractall", tar_t)],
            "fails",
        ),
        case(
            "project-archive",
            "class Archive:\n    def extractall(self, to: str) -> None:\n        print(to)\n\n\ndef unpack_archive(path: str) -> None:\n    print(path)\n\n\nArchive().extractall(\"out\")\nunpack_archive(\"a.tar\")\n",
            &[("T", "«Archive()».extractall", tar_t)],
            "fails",
        ),
        case(
            "project-unpack",
            "def unpack_archive(path: str) -> None:\n    print(path)\n\n\nunpack_archive(\"a.tar\")\n",
            &[("U", "«unpack_archive»(\"a.tar", unpack_d)],
            "fails",
        ),
        case(
            "sa-session",
            "from sqlalchemy.orm import Session\n\n\ndef users(session: Session):\n    return session.query(int).all()\n",
            &[("S", "«session».query", sa)],
            "holds",
        ),
        case(
            "sa-alias",
            "from sqlalchemy.orm import Session as Db\n\n\ndef first(db: Db):\n    return db.query(int).first()\n",
            &[("S", "«db».query", sa)],
            "holds",
        ),
        case(
            "sa-sessionmaker",
            "from sqlalchemy import create_engine\nfrom sqlalchemy.orm import sessionmaker\n\nSessionLocal = sessionmaker(create_engine(\"sqlite://\"))\n\n\ndef count():\n    with SessionLocal() as s:\n        return s.query(int).count()\n",
            &[("S", "«s».query", sa)],
            "holds",
        ),
        case(
            "sa-scoped",
            "from sqlalchemy import create_engine\nfrom sqlalchemy.orm import scoped_session, sessionmaker\n\ndb = scoped_session(sessionmaker(create_engine(\"sqlite://\")))\nrows = db.query(int).all()\n",
            &[("S", "«db».query", sa)],
            "holds",
        ),
        case(
            "sa-subclass",
            "from sqlalchemy.orm import Session\n\n\nclass AppSession(Session):\n    pass\n\n\ndef rows(s: AppSession):\n    return s.query(int).all()\n",
            &[("S", "«s».query", sa)],
            "holds",
        ),
        case(
            "sa-call",
            "from sqlalchemy import create_engine\nfrom sqlalchemy.orm import sessionmaker\n\nrows = sessionmaker(create_engine(\"sqlite://\"))().query(int).all()\n",
            &[(
                "S",
                "«sessionmaker(create_engine(\"sqlite://\"))()».query",
                sa,
            )],
            "holds",
        ),
        case(
            "sa-project-session",
            "class Session:\n    def query(self, q: str) -> list[str]:\n        return [q]\n\n\ndef search(session: Session):\n    return session.query(\"text\")\n",
            &[("S", "«session».query", sa)],
            "fails",
        ),
        case(
            "sa-any",
            "from typing import Any\n\n\ndef f(db: Any):\n    return db.query(int)\n",
            &[("S", "«db».query", sa)],
            "fails",
        ),
        case(
            "unknown-match",
            "def f(x):\n    return x.y\n",
            &[(
                "X",
                "return «x».y",
                r#"{"type": {"package": "foo", "unknown": "match"}}"#,
            )],
            "holds",
        ),
        case(
            "not-type",
            "import tarfile\n\n\ndef first(tar: tarfile.TarFile) -> None:\n    tar.extract(tar.getmembers()[0], path=\"out\")\n",
            &[(
                "T",
                "«tar».extract(",
                r#"{"not": {"type": {"module": "tarfile", "name": "TarFile"}}}"#,
            )],
            "fails",
        ),
        case(
            "keyword-name",
            "def f(**k: int) -> None:\n    print(k)\n\n\nf(name=1)\n",
            &[("N", "f(«name»=1)", r#"{"type": {"module": "builtins"}}"#)],
            "unchecked",
        ),
    ]
}

#[test]
fn python_engine() {
    let (Some(ty), Some(root)) = (ty(), python_project()) else {
        eprintln!("skipped: uvx/uv (ty {TY}, SQLAlchemy) are not available");
        return;
    };
    let mut config = EngineConfig::new(&root, socket("py"));
    config.want = format!("ty {TY}");
    config.log = Some(root.join("engine.log"));
    config.python = Some(PythonConfig {
        ty: vec![ty.to_string_lossy().into_owned()],
        ..PythonConfig::default()
    });
    let (client, cold) = start(config.clone(), Lang::Python);
    let status = client.ping().unwrap();
    eprintln!(
        "python: daemon and {} ready in {:.0} ms (socket {})",
        status.servers[&Lang::Python],
        cold.as_secs_f64() * 1000.0,
        forge_engine::socket_addr(&config.socket).display()
    );
    ask_all(&client, Lang::Python, &root, "cases", "py", &python_cases());

    // the edit-time typecheck: only the errors the edit added
    std::fs::create_dir_all(root.join("diag")).unwrap();
    let base = root.join("diag/base_a.py");
    std::fs::write(&base, "x: int = 1\ny: str = 2\n").unwrap();
    std::fs::write(root.join("diag/a.py"), "x: int = \"a\"\ny: str = 2\n").unwrap();
    let t = Instant::now();
    let added = client
        .diagnostics(
            Lang::Python,
            &[DiagFile {
                path: "diag/a.py".into(),
                base: Some(base),
            }],
            false,
        )
        .unwrap();
    eprintln!(
        "python: diagnostics in {:.0} ms: {:?}",
        t.elapsed().as_secs_f64() * 1000.0,
        added.iter().map(|d| d.concise()).collect::<Vec<_>>()
    );
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(
        (added[0].line, added[0].code.as_deref()),
        (1, Some("invalid-assignment"))
    );

    // the last session out stops it
    assert!(client.unregister("s1").unwrap());
    let until = Instant::now() + Duration::from_secs(10);
    while client.ping().is_ok() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(client.ping().is_err(), "the daemon still answers");
}

fn ts_cases() -> Vec<Case> {
    let redis = r#"{"type": {"package": ["ioredis", "@redis/client"], "name": ["Redis", "Cluster", "RedisClient", "RedisCluster"]}}"#;
    let crypto = r#"{"declared": {"package": "node", "module": "node/crypto", "name": ["createHash", "createHmac"]}}"#;
    let dep = r#"{"deprecated": true}"#;
    let connect = "interface ConnectOptions {\n  /** @deprecated use timeoutMs */\n  timeout?: number;\n  timeoutMs?: number;\n}\n\ndeclare function connect(options: ConnectOptions): void;\n\nconnect({ timeout: 5 });\n";
    let decl = "/** @deprecated use total() */\nexport function sum(a: number, b: number): number {\n  return a + b;\n}\n\nexport interface ConnectOptions {\n  /** @deprecated use timeoutMs */\n  timeout: number;\n  timeoutMs: number;\n}\n";
    let other = "interface ConnectOptions {\n  /** @deprecated use timeoutMs */\n  timeout?: number;\n  timeoutMs?: number;\n}\n\ndeclare function connect(options: ConnectOptions): void;\n\nconnect({ timeoutMs: 5 });\nconst parser = { parse: (s: string) => s };\nexport const p = parser.parse(\"x\");\n";
    vec![
        case(
            "ioredis",
            "import Redis from \"ioredis\";\n\nconst redis = new Redis();\n\nexport async function sessions(): Promise<string[]> {\n  return redis.keys(\"session:*\");\n}\n",
            &[("C", "return «redis».keys", redis)],
            "holds",
        ),
        case(
            "ioredis-cluster",
            "import { Cluster } from \"ioredis\";\n\nexport const all = (c: Cluster) => c.keys(\"*\");\n",
            &[("C", "«c».keys", redis)],
            "holds",
        ),
        case(
            "node-redis",
            "import { createClient } from \"redis\";\n\nconst client = createClient();\n\nexport async function sessions(): Promise<string[]> {\n  return client.keys(\"session:*\");\n}\n",
            &[("C", "return «client».keys", redis)],
            "holds",
        ),
        case(
            "map-keys",
            "const cache = new Map<string, number>();\n\nexport const names = [...cache.keys()];\nexport const fields = Object.keys({ a: 1 });\n",
            &[("C", "«cache».keys", redis)],
            "fails",
        ),
        case(
            "object-keys",
            "const cache = new Map<string, number>();\n\nexport const names = [...cache.keys()];\nexport const fields = Object.keys({ a: 1 });\n",
            &[("C", "«Object».keys", redis)],
            "fails",
        ),
        case(
            "create-hash",
            "import { createHash } from \"node:crypto\";\n\nexport const digest = (password: string) => createHash(\"md5\").update(password).digest(\"hex\");\n",
            &[("F", "«createHash»(\"md5\")", crypto)],
            "holds",
        ),
        case(
            "create-hmac",
            "import crypto from \"crypto\";\n\nexport const sign = (key: string, body: string) => crypto.createHmac('SHA1', key).update(body).digest(\"hex\");\n",
            &[("F", "«crypto.createHmac»(", crypto)],
            "holds",
        ),
        case(
            "project-hash",
            "function createHash(alg: string): string {\n  return alg;\n}\n\nexport const label = createHash(\"md5\");\n",
            &[("F", "«createHash»(\"md5\")", crypto)],
            "fails",
        ),
        case(
            "url-parse",
            "import * as url from \"node:url\";\n\nexport const home = url.parse(\"https://example.com/\");\n",
            &[("N", "url.«parse»(", dep)],
            "holds",
        ),
        case(
            "new-buffer",
            "export const bytes = new Buffer(\"payload\");\n",
            &[("N", "new «Buffer»(", dep)],
            "holds",
        ),
        case(
            "own-deprecated",
            "/** @deprecated use total() */\nfunction sum(a: number, b: number): number {\n  return a + b;\n}\n\nexport const s = sum(1, 2);\n",
            &[("N", "= «sum»(1", dep)],
            "holds",
        ),
        case(
            "deprecated-key",
            connect,
            &[("N", "{ «timeout»: 5 }", dep)],
            "holds",
        ),
        case(
            "deprecated-overload",
            "declare function format(value: string): string;\n/** @deprecated pass a string */\ndeclare function format(value: number): string;\n\nexport const f = format(1);\n",
            &[("N", "= «format»(1)", dep)],
            "holds",
        ),
        case(
            "new-url",
            "export const home = new URL(\"https://example.com/\");\n",
            &[("N", "new «URL»(", dep)],
            "fails",
        ),
        case(
            "buffer-from",
            "export const bytes = Buffer.from(\"payload\");\n",
            &[("N", "Buffer.«from»(", dep)],
            "fails",
        ),
        case(
            "deprecated-declaration",
            decl,
            &[("N", "function «sum»(", dep)],
            "fails",
        ),
        case(
            "deprecated-member",
            decl,
            &[("N", "  «timeout»: number", dep)],
            "fails",
        ),
        case(
            "other-key",
            other,
            &[("N", "{ «timeoutMs»: 5 }", dep)],
            "fails",
        ),
        case(
            "other-parse",
            other,
            &[("N", "parser.«parse»(", dep)],
            "fails",
        ),
        case(
            "good-overload",
            "declare function format(value: string): string;\n/** @deprecated pass a string */\ndeclare function format(value: number): string;\n\nexport const f = format(\"1\");\n",
            &[("N", "= «format»(\"1\")", dep)],
            "fails",
        ),
        case(
            "parameter-name",
            "export function f(a: number): number {\n  return a;\n}\n",
            &[(
                "A",
                "f(«a»: number)",
                r#"{"type": {"package": "typescript"}}"#,
            )],
            "unchecked",
        ),
    ]
}

fn js_runtime() -> Option<PathBuf> {
    ["node", "bun"]
        .iter()
        .find_map(|n| run(Command::new("which").arg(n)).map(PathBuf::from))
}

fn typescript_engine(version: &str) {
    let (Some(root), Some(rt)) = (ts_project(version), js_runtime()) else {
        eprintln!("skipped: bun (typescript@{version}) or a JavaScript runtime is not available");
        return;
    };
    let mut config = EngineConfig::new(&root, socket(&format!("ts{version}")));
    config.want = "typescript (the project's)".into();
    config.log = Some(root.join("engine.log"));
    config.typescript = Some(TypeScriptConfig {
        js_runtime: Some(rt),
        preload: true,
    });
    let (client, cold) = start(config, Lang::Typescript);
    eprintln!(
        "typescript {version}: daemon and {} ready in {:.0} ms",
        client.ping().unwrap().servers[&Lang::Typescript],
        cold.as_secs_f64() * 1000.0
    );
    ask_all(&client, Lang::Typescript, &root, "src", "ts", &ts_cases());

    let base = root.join("base_a.ts.txt");
    std::fs::write(
        &base,
        "export const x: number = 1;\nexport const y: string = 2;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/a.ts"),
        "export const x: number = \"a\";\nexport const y: string = 2;\n",
    )
    .unwrap();
    client.changed(&["src/a.ts".to_owned()]).unwrap();
    let t = Instant::now();
    let added = client
        .diagnostics(
            Lang::Typescript,
            &[DiagFile {
                path: "src/a.ts".into(),
                base: Some(base),
            }],
            false,
        )
        .unwrap();
    eprintln!(
        "typescript {version}: diagnostics in {:.0} ms: {:?}",
        t.elapsed().as_secs_f64() * 1000.0,
        added.iter().map(|d| d.concise()).collect::<Vec<_>>()
    );
    std::fs::remove_file(root.join("src/a.ts")).unwrap();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((added[0].line, added[0].code.as_deref()), (1, Some("2322")));
    client.stop().unwrap();
    assert!(client.ping().is_err());
}

#[test]
fn typescript_7_engine() {
    typescript_engine("7.0.2");
}

#[test]
fn typescript_6_engine() {
    typescript_engine("6.0.3");
}

#[test]
fn typescript_5_engine() {
    typescript_engine("5.9.3");
}
