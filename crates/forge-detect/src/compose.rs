//! The services a project runs next to its code, from its Compose files: what its tests depend
//! on (a database, a cache, a broker), as opposed to what only runs around it (a reverse proxy,
//! an admin UI, observability). One list for every reader: `forge apply`'s Testcontainers offer
//! (§3.5) and the rules about tests that need these services.

use std::path::Path;

/// The Compose files Docker Compose reads by default, at a directory's top level.
pub const COMPOSE_FILES: &[&str] = &[
    "compose.yaml",
    "compose.yml",
    "docker-compose.yaml",
    "docker-compose.yml",
];

/// Images of services that are not dependencies of the code: reverse proxies, admin UIs,
/// observability. Matched against the image's name without registry, namespace and tag
/// ([`image_name`]). A project excludes more in its own configuration, passed to
/// [`dependency_services`].
pub const NOT_DEPENDENCIES: &[&str] = &[
    // reverse proxies
    "nginx",
    "traefik",
    "caddy",
    "haproxy",
    "envoy",
    // admin UIs
    "adminer",
    "pgadmin4",
    "pgadmin",
    "phpmyadmin",
    "mongo-express",
    "redis-commander",
    "redisinsight",
    "kafka-ui",
    "mailhog",
    "mailpit",
    // observability
    "grafana",
    "prometheus",
    "jaeger",
    "all-in-one",
    "otel-collector",
    "opentelemetry-collector",
    "opentelemetry-collector-contrib",
    "loki",
    "promtail",
    "tempo",
    "zipkin",
];

/// A service of a Compose file that runs a published image (`image:` and no `build:`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub file: String,
    pub name: String,
    pub image: String,
}

/// An image reference's name: no registry, namespace, tag or digest
/// (`docker.io/bitnami/postgresql:17` → `postgresql`).
pub fn image_name(image: &str) -> String {
    let no_digest = image.split('@').next().unwrap_or(image);
    let last = no_digest.rsplit('/').next().unwrap_or(no_digest);
    last.split(':').next().unwrap_or(last).to_lowercase()
}

/// The services of `dir`'s Compose files that run a published image.
pub fn image_services(dir: &Path) -> Vec<Service> {
    use saphyr::{LoadableYamlNode, Yaml};
    let mut out = Vec::new();
    for file in COMPOSE_FILES {
        let Ok(text) = std::fs::read_to_string(dir.join(file)) else {
            continue;
        };
        let Ok(docs) = Yaml::load_from_str(&text) else {
            continue;
        };
        let Some(services) = docs
            .first()
            .and_then(|d| d.as_mapping_get("services"))
            .and_then(Yaml::as_mapping)
        else {
            continue;
        };
        for (name, svc) in services {
            let (Some(name), Some(image)) = (
                name.as_str(),
                svc.as_mapping_get("image").and_then(Yaml::as_str),
            ) else {
                continue;
            };
            if svc.as_mapping_get("build").is_some() {
                continue;
            }
            out.push(Service {
                file: (*file).to_string(),
                name: name.to_string(),
                image: image.to_string(),
            });
        }
    }
    out
}

/// The services of `dir`'s Compose files the code depends on: a published image that is not in
/// [`NOT_DEPENDENCIES`] nor in `also_not` (image names, or service names, the project excludes).
pub fn dependency_services(dir: &Path, also_not: &[String]) -> Vec<Service> {
    image_services(dir)
        .into_iter()
        .filter(|s| {
            let n = image_name(&s.image);
            !NOT_DEPENDENCIES.contains(&n.as_str())
                && !also_not
                    .iter()
                    .any(|x| x.eq_ignore_ascii_case(&n) || x == &s.name)
        })
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
        assert_eq!(names, ["db", "cache"]);
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
