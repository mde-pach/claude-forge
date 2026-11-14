//! Compose files' third-party services, and whether a project's tests use Testcontainers: what
//! `docker/compose/tests-use-testcontainers` and the test checks' Docker check ask (§10).
//!
//! A third-party service is one the project runs from someone else's image (`image:` and no
//! `build:`) that its code depends on: a database, a broker, a cache. A reverse proxy, an admin
//! UI or an observability stack also runs from an image, but no test needs it:
//! [`is_non_dependency_image`] is the one list of those, and `forge.yaml`
//! `docker.compose.not-dependencies` names a project's own.

use std::path::Path;

use serde_json::Value;

use crate::deps::{norm_py, npm_deps, parse_toml, pipfile_deps, pyproject_deps, requirements_deps};

/// Images that run beside an application without being one of its dependencies, by repository
/// name (the last path segment, or the whole path for images whose last segment says nothing,
/// such as `jaegertracing/all-in-one`): reverse proxies, admin UIs, observability.
pub const NON_DEPENDENCY_IMAGES: &[&str] = &[
    // reverse proxies and load balancers
    "nginx",
    "nginx-unprivileged",
    "openresty",
    "traefik",
    "caddy",
    "haproxy",
    "envoy",
    // admin UIs
    "adminer",
    "pgadmin",
    "pgadmin4",
    "kafka-ui",
    "phpmyadmin",
    "mongo-express",
    "redis-commander",
    "redisinsight",
    // observability
    "grafana",
    "prometheus",
    "jaegertracing/all-in-one",
    "jaeger",
    "otel-collector",
    "opentelemetry-collector",
    "opentelemetry-collector-contrib",
    "loki",
    "promtail",
    "tempo",
    "zipkin",
];

/// An image's repository: no registry host, tag or digest (`ghcr.io/acme/db:1@sha256:…` →
/// `acme/db`), lower case.
pub fn image_repository(image: &str) -> String {
    let no_digest = image.split('@').next().unwrap_or(image);
    let mut parts: Vec<&str> = no_digest.split('/').collect();
    if parts.len() > 1 {
        let first = parts[0];
        if first.contains('.') || first.contains(':') || first == "localhost" {
            parts.remove(0);
        }
    }
    let mut repo = parts.join("/");
    // the tag is after the last ':' of the last segment
    if let Some(i) = repo.rfind(':')
        && !repo[i..].contains('/')
    {
        repo.truncate(i);
    }
    let repo = repo.strip_prefix("library/").unwrap_or(&repo).to_string();
    repo.to_lowercase()
}

/// Whether an image is one of [`NON_DEPENDENCY_IMAGES`]: its repository's last segment, or its
/// whole repository, is listed.
pub fn is_non_dependency_image(image: &str) -> bool {
    let repo = image_repository(image);
    let last = repo.rsplit('/').next().unwrap_or(&repo);
    NON_DEPENDENCY_IMAGES
        .iter()
        .any(|n| *n == last || *n == repo)
}

/// A compose service that runs a third party's image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposeService {
    pub name: String,
    pub image: String,
}

/// A service's mapping with its `<<` merge keys applied (the service's own keys win).
fn merged(service: &Value) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    let Some(m) = service.as_object() else {
        return out;
    };
    let bases: Vec<&Value> = match m.get("<<") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(v) => vec![v],
        None => Vec::new(),
    };
    for b in bases {
        for (k, v) in merged(b) {
            out.insert(k, v);
        }
    }
    for (k, v) in m {
        if k != "<<" {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// The third-party services of a compose file's text: `image:` and no `build:`, not a
/// [non-dependency image](is_non_dependency_image), not named in `excluded`. A file that does
/// not parse has none.
pub fn third_party_services(text: &str, excluded: &[String]) -> Vec<ComposeService> {
    let Ok(doc) = parse_yaml(text) else {
        return Vec::new();
    };
    let Some(services) = doc.get("services").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, s) in services {
        let s = merged(s);
        let Some(image) = s.get("image").and_then(Value::as_str) else {
            continue;
        };
        if s.contains_key("build") || is_non_dependency_image(image) || excluded.contains(name) {
            continue;
        }
        out.push(ComposeService {
            name: name.clone(),
            image: image.to_string(),
        });
    }
    out
}

/// YAML with anchors and aliases resolved (compose files use them), as a JSON value.
fn parse_yaml(text: &str) -> Result<Value, String> {
    use saphyr::{LoadableYamlNode, YamlOwned};
    fn json(y: YamlOwned) -> Value {
        use saphyr::ScalarOwned;
        match y {
            YamlOwned::Value(s) => match s {
                ScalarOwned::Null => Value::Null,
                ScalarOwned::Boolean(b) => Value::Bool(b),
                ScalarOwned::Integer(i) => Value::from(i),
                ScalarOwned::FloatingPoint(f) => {
                    serde_json::Number::from_f64(f.into_inner()).map_or(Value::Null, Value::Number)
                }
                ScalarOwned::String(s) => Value::String(s),
            },
            YamlOwned::Sequence(s) => Value::Array(s.into_iter().map(json).collect()),
            YamlOwned::Mapping(m) => Value::Object(
                m.into_iter()
                    .map(|(k, v)| {
                        let key = match json(k) {
                            Value::String(s) => s,
                            other => other.to_string(),
                        };
                        (key, json(v))
                    })
                    .collect(),
            ),
            YamlOwned::Tagged(_, inner) => json(*inner),
            YamlOwned::Representation(s, _, _) => Value::String(s),
            _ => Value::Null,
        }
    }
    let docs = YamlOwned::load_from_str(text).map_err(|e| e.to_string())?;
    Ok(docs.into_iter().next().map_or(Value::Null, json))
}

/// A language whose tests can use Testcontainers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestLanguage {
    Python,
    TypeScript,
}

impl TestLanguage {
    pub fn parse(s: &str) -> Option<TestLanguage> {
        match s {
            "python" => Some(TestLanguage::Python),
            "typescript" => Some(TestLanguage::TypeScript),
            _ => None,
        }
    }

    /// The Testcontainers package as the language's registry names it.
    pub fn package(self) -> &'static str {
        match self {
            TestLanguage::Python => "testcontainers (PyPI)",
            TestLanguage::TypeScript => "testcontainers or @testcontainers/* (npm)",
        }
    }
}

/// Whether a declared npm package is Testcontainers: `testcontainers` or a module of the
/// `@testcontainers` scope.
pub fn is_testcontainers_npm(name: &str) -> bool {
    name == "testcontainers" || name.starts_with("@testcontainers/")
}

/// Whether a manifest's text (by file name: `pyproject.toml`, `requirements*.txt`, `Pipfile`,
/// `package.json`) declares Testcontainers for `lang`.
pub fn declares_testcontainers(file_name: &str, text: &str, lang: TestLanguage) -> bool {
    match lang {
        TestLanguage::Python => {
            let deps = if file_name == "pyproject.toml" || file_name == "Pipfile" {
                let Ok(t) = parse_toml(text) else {
                    return false;
                };
                if file_name == "Pipfile" {
                    pipfile_deps(&t)
                } else {
                    pyproject_deps(&t)
                }
            } else if file_name.ends_with(".txt") {
                requirements_deps(text)
            } else {
                return false;
            };
            deps.contains_key(&norm_py("testcontainers"))
        }
        TestLanguage::TypeScript => {
            file_name == "package.json"
                && serde_json::from_str::<Value>(text)
                    .is_ok_and(|p| npm_deps(&p).keys().any(|k| is_testcontainers_npm(k)))
        }
    }
}

/// Whether `dir` or one of its parents up to `root` declares Testcontainers for `lang`.
pub fn testcontainers_declared_in(root: &Path, dir: &Path, lang: TestLanguage) -> bool {
    let names: &[&str] = match lang {
        TestLanguage::Python => &["pyproject.toml", "Pipfile", "requirements.txt"],
        TestLanguage::TypeScript => &["package.json"],
    };
    let mut d = Some(dir);
    while let Some(cur) = d {
        for n in names {
            if let Ok(text) = std::fs::read_to_string(cur.join(n))
                && declares_testcontainers(n, &text, lang)
            {
                return true;
            }
        }
        if lang == TestLanguage::Python {
            let reqs = |d: &Path, all: bool| -> bool {
                std::fs::read_dir(d)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| {
                        let n = e.file_name().to_string_lossy().into_owned();
                        (all || n.starts_with("requirements"))
                            && n.ends_with(".txt")
                            && std::fs::read_to_string(e.path())
                                .is_ok_and(|t| declares_testcontainers(&n, &t, lang))
                    })
            };
            if reqs(cur, false) || reqs(&cur.join("requirements"), true) {
                return true;
            }
        }
        if cur == root {
            break;
        }
        d = cur.parent().filter(|p| p.starts_with(root));
    }
    false
}

/// Whether a source file's text imports Testcontainers (`import testcontainers`, `from
/// testcontainers.postgres import …`; `from "testcontainers"`, `"@testcontainers/postgresql"`,
/// `require("testcontainers")`, `import("…")`).
pub fn imports_testcontainers(text: &str, lang: TestLanguage) -> bool {
    use std::sync::LazyLock;
    static PY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"(?m)^\s*(from\s+testcontainers(\.|\s)|import\s+testcontainers\b)")
            .expect("regex")
    });
    static TS: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(
            r#"(\bfrom\s*|\brequire\s*\(\s*|\bimport\s*\(\s*|^\s*import\s+)["'](testcontainers|@testcontainers/[^"']+)["']"#,
        )
        .expect("regex")
    });
    match lang {
        TestLanguage::Python => PY.is_match(text),
        TestLanguage::TypeScript => text.lines().any(|l| TS.is_match(l)),
    }
}

/// Whether a file is a JS/TS test runner's setup file (vitest's `setupFiles`, jest's
/// `setupFilesAfterEach`, bun's `preload` by their usual names): where a suite starts what every
/// test shares.
pub fn is_ts_test_setup(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = base.split_once('.') else {
        return false;
    };
    let ext = ext.rsplit('.').next().unwrap_or(ext);
    if !matches!(ext, "ts" | "mts" | "cts" | "tsx" | "js" | "mjs" | "cjs") {
        return false;
    }
    let in_test_dir = path
        .split('/')
        .any(|d| matches!(d, "test" | "tests" | "__tests__" | "spec"));
    matches!(stem, "vitest" | "jest") && base.contains(".setup.")
        || matches!(
            stem,
            "setupTests" | "setup-tests" | "globalSetup" | "global-setup"
        )
        || stem == "setup" && in_test_dir
}

/// The Compose files Docker Compose reads by default, at a directory's top level.
pub const COMPOSE_FILES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// An image reference's name: its repository's last segment (`docker.io/bitnami/postgresql:17`
/// → `postgresql`).
pub fn image_name(image: &str) -> String {
    let repo = image_repository(image);
    repo.rsplit('/').next().unwrap_or(&repo).to_string()
}

/// The third-party services of `dir`'s default Compose files ([`COMPOSE_FILES`]), as
/// [`third_party_services`] reads each: what `forge apply`'s Testcontainers offer (§3.5) adds
/// modules for. `excluded`: service names the project says are not dependencies.
pub fn dependency_services(dir: &Path, excluded: &[String]) -> Vec<ComposeService> {
    COMPOSE_FILES
        .iter()
        .filter_map(|f| std::fs::read_to_string(dir.join(f)).ok())
        .flat_map(|text| third_party_services(&text, excluded))
        .collect()
}

/// What a dependency service is, by its image's name, as Testcontainers names its modules:
/// `postgres`, `mysql`, `mariadb`, `mongodb`, `redis`, `kafka`, `rabbitmq`, `elasticsearch`,
/// `opensearch`, `localstack`, `minio`, `mssql`, `clickhouse`, `neo4j`, `nats`, `cassandra`,
/// `memcached`; `None` for any other (a generic container).
pub fn service_kind(image: &str) -> Option<&'static str> {
    let n = image_name(image);
    let kind = match n.as_str() {
        "postgres" | "postgresql" | "postgis" | "pgvector" | "timescaledb" => "postgres",
        "mysql" | "mysql-server" => "mysql",
        "mariadb" => "mariadb",
        "mongo" | "mongodb" | "mongodb-community-server" => "mongodb",
        "redis" | "redis-stack" | "redis-stack-server" | "valkey" => "redis",
        "kafka" | "cp-kafka" | "cp-server" | "redpanda" => "kafka",
        "rabbitmq" => "rabbitmq",
        "elasticsearch" => "elasticsearch",
        "opensearch" => "opensearch",
        "localstack" => "localstack",
        "minio" => "minio",
        "server" if image.contains("mssql") => "mssql",
        "mssql-server-linux" | "azure-sql-edge" => "mssql",
        "clickhouse-server" | "clickhouse" => "clickhouse",
        "neo4j" => "neo4j",
        "nats" => "nats",
        "cassandra" | "scylla" => "cassandra",
        "memcached" => "memcached",
        _ => return None,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories() {
        assert_eq!(image_repository("postgres:17"), "postgres");
        assert_eq!(
            image_repository("docker.io/library/redis:7@sha256:ab"),
            "redis"
        );
        assert_eq!(image_repository("ghcr.io/acme/db:1"), "acme/db");
        assert_eq!(image_repository("localhost:5000/nginx"), "nginx");
        assert!(is_non_dependency_image("nginx:1.27-alpine"));
        assert!(is_non_dependency_image("jaegertracing/all-in-one:1.60"));
        assert!(is_non_dependency_image("dpage/pgadmin4"));
        assert!(is_non_dependency_image(
            "otel/opentelemetry-collector-contrib:0.110.0"
        ));
        assert!(!is_non_dependency_image("postgres:17"));
        assert!(!is_non_dependency_image("bitnami/kafka:3.8"));
        assert!(!is_non_dependency_image("acme/all-in-one"));
    }

    #[test]
    fn third_party_services_of_a_compose_file() {
        let text = "x-db: &db\n  image: postgres:17\nservices:\n  api:\n    build: .\n    image: acme/api\n  db:\n    <<: *db\n  cache:\n    image: redis:7\n  proxy:\n    image: nginx:1.27\n  queue:\n    image: rabbitmq:4\n";
        let names: Vec<String> = third_party_services(text, &["queue".into()])
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["db", "cache"]);
        assert!(third_party_services("services: [", &[]).is_empty());
    }

    #[test]
    fn declarations_and_imports() {
        use TestLanguage::*;
        assert!(declares_testcontainers(
            "pyproject.toml",
            "[dependency-groups]\ndev = [\"testcontainers[postgres]>=4.8\"]\n",
            Python
        ));
        assert!(!declares_testcontainers(
            "pyproject.toml",
            "[project]\nname = \"x\"\ndependencies = [\"psycopg\"]\n",
            Python
        ));
        assert!(declares_testcontainers(
            "package.json",
            r#"{"devDependencies":{"@testcontainers/postgresql":"^10.13.0"}}"#,
            TypeScript
        ));
        assert!(imports_testcontainers(
            "import pytest\nfrom testcontainers.postgres import PostgresContainer\n",
            Python
        ));
        assert!(!imports_testcontainers(
            "# testcontainers later\nimport os\n",
            Python
        ));
        assert!(imports_testcontainers(
            "import { PostgreSqlContainer } from \"@testcontainers/postgresql\";\n",
            TypeScript
        ));
        assert!(imports_testcontainers(
            "const { GenericContainer } = require('testcontainers');\n",
            TypeScript
        ));
        assert!(!imports_testcontainers(
            "import x from \"testcontainers-ish\";\n",
            TypeScript
        ));
        assert!(is_ts_test_setup("vitest.setup.ts"));
        assert!(is_ts_test_setup("test/setup.ts"));
        assert!(is_ts_test_setup("src/setupTests.ts"));
        assert!(!is_ts_test_setup("src/setup.ts"));
    }

    #[test]
    fn dependency_services_leave_out_built_and_surrounding_ones() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("compose.yaml"),
            "services:\n  app:\n    build: .\n    image: shop:dev\n  db:\n    image: postgres:17\n  cache:\n    image: docker.io/library/redis:7-alpine\n  proxy:\n    image: nginx:1.27\n  ui:\n    image: dpage/pgadmin4\n  mail:\n    image: axllent/mailpit\n  search:\n    image: docker.elastic.co/elasticsearch/elasticsearch:8.15.0\n",
        )
        .unwrap();
        let names: Vec<String> = dependency_services(d.path(), &["search".into()])
            .into_iter()
            .map(|s| s.name)
            .collect();
        // a mail catcher (mailpit) is a service the code sends to: a dependency
        assert_eq!(names, ["db", "cache", "mail"]);
        assert_eq!(
            service_kind("docker.io/library/redis:7-alpine"),
            Some("redis")
        );
        assert_eq!(
            service_kind("mcr.microsoft.com/mssql/server:2022-latest"),
            Some("mssql")
        );
        assert_eq!(service_kind("ghcr.io/acme/thing:1"), None);
        assert_eq!(image_name("bitnami/postgresql@sha256:abc"), "postgresql");
    }
}
