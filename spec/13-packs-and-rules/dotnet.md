<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `dotnet`

### dotnet

.NET solutions and projects (C#, F#, Visual Basic).

Detected by: a file matching `**/*.csproj`, `**/*.fsproj`, `**/*.vbproj`, `**/*.sln`, `**/*.slnx`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `dotnet/framework-upgrade` | Upgrading a .NET solution's target framework project by project, in dependency order, reading each build's errors and package reports. | on-demand | The task upgrades the target framework or .NET version of a solution or project. | wherever its pack is |
| `dotnet/inspect-assembly` | Reading what a built .NET assembly or NuGet package really contains (its types, members, references and metadata) with dotnet-inspect. | on-demand | The task needs to know what a compiled .NET assembly or NuGet package contains or references.<br>The task is to find why a .NET type or member is missing, duplicated or bound to the wrong assembly at run time. | wherever its pack is |

### dotnet/aspire

.NET Aspire application hosts and their resources.

Detected by: a line matching `Aspire\.AppHost\.Sdk\|"Aspire\.Hosting\.AppHost"` in a file matching `**/*.csproj`.
Child of `dotnet`: active only where it is.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `dotnet/aspire/apphost-run` | Running an Aspire AppHost to see whether its resources start and connect, from its output and from a testing-host check. | on-demand | The task is to find why a service of an Aspire application fails to start or to reach another resource.<br>The task changes how an Aspire AppHost wires its projects, containers or connection strings. | wherever its pack is |
