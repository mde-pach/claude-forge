<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `go`

### go

Go modules.

Detected by: a file matching `**/go.mod`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/bench-compare` | Measuring a Go performance change with repeated benchmarks, benchstat and pprof profiles. | on-demand | The task is to make Go code faster or use less memory.<br>The task needs a performance difference between two versions of Go code measured. | wherever its pack is |
| `go/coverage-gaps` | Finding the Go code no test executes, from a coverage profile. | on-demand | The task is to add tests for Go code or find what the tests leave untested. | wherever its pack is |
| `go/mechanical-rewrite` | Renaming and rewriting Go code across packages with gopls and gofmt -r instead of editing each file. | on-demand | The task renames or moves a Go identifier used in many files or packages.<br>The task applies the same mechanical change to many places in Go code. | wherever its pack is |
| `go/package-docs` | Reading a Go dependency's real API, versions and known vulnerabilities with go doc, go list and govulncheck. | on-demand | The task writes new code that uses a library's components, types or functions.<br>The task is to add, choose or upgrade a Go module dependency. | wherever its pack is |
| `go/runtime-debug` | Finding the cause of a Go crash, data race, hang, goroutine leak or memory growth with the race detector, goroutine dumps and pprof. | on-demand | The task is to find why a Go program crashes, hangs, deadlocks or has a data race.<br>The task is to find why a Go program leaks goroutines or its memory keeps growing. | wherever its pack is |

### go/charm

Terminal user interfaces built with Charm's Bubble Tea, Bubbles and Lip Gloss.

Detected by: a line matching `^\s*(require\s+)?(charm\.land/bubbletea\|github\.com/charmbracelet/bubbletea)(/v[0-9]+)?\s` in a file matching `**/go.mod`.
Child of `go`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/charm/bubbletea-headless-test` | Checking a Bubble Tea model's behaviour without a terminal, by sending it messages and running the program headless. | on-demand | The task changes how a Bubble Tea terminal program reacts to keys, messages or window sizes.<br>The task changes what a Bubble Tea terminal program renders. | wherever its pack is |

### go/go-swagger

Go servers and clients generated from a Swagger 2.0 spec with go-swagger.

Detected by: a line matching `github\.com/go-swagger/go-swagger\s` in a file matching `**/go.mod`; or a line matching `^//go:generate\s+.*swagger\s+generate\s` in a file matching `**/*.go`.
Child of `go`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/go-swagger/swagger-generate` | Regenerating go-swagger code from the spec and reading what changed. | on-demand | The task changes a Swagger spec that go-swagger generates Go code from.<br>The task adds or changes an endpoint of a go-swagger server or client. | wherever its pack is |

### go/gqlgen

GraphQL servers generated with gqlgen.

Detected by: a file matching `**/gqlgen.yml`, `**/gqlgen.yaml`; or a line matching `github\.com/99designs/gqlgen\s` in a file matching `**/go.mod`.
Child of `go`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/gqlgen/gqlgen-generate` | Regenerating gqlgen code after a GraphQL schema change and filling in the resolver stubs. | on-demand | The task changes a GraphQL schema of a gqlgen server.<br>The task adds or changes a GraphQL query, mutation, type or field in Go. | wherever its pack is |

### go/grpc

gRPC servers and clients in Go.

Detected by: a line matching `google\.golang\.org/grpc\s` in a file matching `**/go.mod`.
Child of `go`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/grpc/grpc-call` | Calling a Go gRPC service, in a bufconn test or with grpcurl, to see its real status codes and responses. | on-demand | The task changes what a gRPC method or interceptor does or returns.<br>The task needs a gRPC service's behaviour checked by calling it. | wherever its pack is |

### go/uber-fx

Go applications wired with Uber's fx dependency injection.

Detected by: a line matching `go\.uber\.org/fx\s` in a file matching `**/go.mod`.
Child of `go`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `go/uber-fx/fx-graph` | Seeing an fx application's dependency graph and startup events to find a missing provider or a wrong startup order. | on-demand | The task is to find why an fx application fails to start or a dependency is missing.<br>The task adds or changes an fx module, provider or lifecycle hook. | wherever its pack is |
