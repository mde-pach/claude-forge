You are working in the git repository in the current directory. Make the change described below. Work autonomously: nobody will answer questions.

## Block SSRF bypasses through IPv6 transition addresses

The SSRF guard in `fastmcp.server.auth.ssrf` decides whether a resolved IP address is safe to fetch. It already rejects IPv6 addresses that wrap a blocked IPv4 target in a few forms: IPv4-mapped, 6to4, Teredo, and the well-known NAT64 prefix `64:ff9b::/96`.

Several other IPv6 transition encodings are not checked. An attacker can use them to reach loopback, private (RFC 1918), link-local (including cloud metadata at `169.254.169.254`) or CGNAT (`100.64.0.0/10`) targets.

### Required behaviour

`is_ip_allowed(ip_str: str) -> bool` must return `False` for any IPv6 address that embeds an IPv4 address which would itself be disallowed. This applies to every recognised transition form below. Any one blocked embedded candidate is enough to reject the address. The address must still also pass the existing global-unicast and non-multicast checks.

Recognised forms:

- **IPv4-mapped** `::ffff:a.b.c.d`. Already handled; keep it.
- **6to4** `2002::/16`. Already handled; keep it.
- **Teredo**. Already handled; keep it. Check both the server and the client address.
- **NAT64 well-known prefix** `64:ff9b::/96`, with the IPv4 address in the low 32 bits.
- **NAT64 local-use prefix** `64:ff9b:1::/48`. Consider every RFC 6052 embedding position:
  - the /48 layout: bytes 6, 7, 9, 10
  - the /56 layout: bytes 7, 9, 10, 11
  - the /64 layout: bytes 9, 10, 11, 12
  - the /96 layout: low 32 bits
- **IPv4-translated** `::ffff:0:a.b.c.d` (`::ffff:0:0:0/96`), with the IPv4 address in the low 32 bits.
- **IPv4-compatible** `::a.b.c.d`, meaning the upper 96 bits are zero. This excludes `::` and `::1`, which are handled as unspecified and loopback.
- **ISATAP**: any address whose interface identifier starts with `0000:5efe` or `0200:5efe` (bytes 8–11). The IPv4 address is in the low 32 bits.

Examples that must be rejected:

- `64:ff9b:1::a9fe:a9fe`
- `64:ff9b:1:a9fe:a9:fe00::`
- `::ffff:0:7f00:1`
- `::0a00:1`
- `::6440:1`
- `2002:a9fe:a9fe::1`
- `2606:4700::5efe:192.168.1.1`
- `2606:4700::200:5efe:169.254.169.254`

Transition addresses that embed a public IPv4 address must remain allowed when they are otherwise globally routable. Examples:

- `64:ff9b::0808:0808`
- `::ffff:0:0808:0808`
- `::0808:0808`
- `2606:4700::5efe:8.8.8.8`

`validate_url(...)` must raise `SSRFError` with a message containing `"blocked IP"` when the hostname resolves to any of the newly blocked forms.

The function name, signature and module path stay unchanged.
