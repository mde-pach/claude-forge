"""Allow-list CONNECT proxy for trial containers (DECISIONS.md D4).

Listens on the gateway of the internal Docker network. CONNECT to an allowed host
is tunnelled through the environment's upstream HTTPS proxy ($HTTPS_PROXY);
everything else gets 403. Every request is logged (JSON lines) so blocked hosts
are visible per trial (the client address identifies the container).
"""
import json
import os
import select
import socket
import sys
import threading
import time
from urllib.parse import urlparse

ALLOWED = {
    "api.anthropic.com",
    "statsig.anthropic.com",
    "statsig.com",
    "featuregates.org",
    "sentry.io",
    "o1158394.ingest.us.sentry.io",
    "http-intake.logs.us5.datadoghq.com",
    "claude.ai",
    "console.anthropic.com",
    "platform.claude.com",
    "mcp-proxy.anthropic.com",
}

LOG_LOCK = threading.Lock()


def log(path, rec):
    rec["ts"] = time.time()
    with LOG_LOCK, open(path, "a") as f:
        f.write(json.dumps(rec) + "\n")


def pipe(a, b):
    socks = [a, b]
    try:
        while True:
            r, _, x = select.select(socks, [], socks, 600)
            if x or not r:
                return
            for s in r:
                data = s.recv(65536)
                if not data:
                    return
                (b if s is a else a).sendall(data)
    except OSError:
        return
    finally:
        for s in socks:
            try:
                s.close()
            except OSError:
                pass


def handle(client, addr, upstream, logpath):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = client.recv(4096)
            if not chunk:
                client.close()
                return
            buf += chunk
            if len(buf) > 65536:
                client.close()
                return
        head, rest = buf.split(b"\r\n\r\n", 1)
        line = head.split(b"\r\n")[0].decode("latin1")
        method, target, _ = (line.split(" ") + ["", ""])[:3]
        host = target.rsplit(":", 1)[0] if method == "CONNECT" else (urlparse(target).hostname or "")
        ok = method == "CONNECT" and host in ALLOWED
        log(logpath, {"client": addr[0], "method": method, "host": host, "allowed": ok})
        if not ok:
            client.sendall(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            client.close()
            return
        up = socket.create_connection(upstream, timeout=30)
        up.sendall(f"CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n".encode())
        resp = b""
        while b"\r\n\r\n" not in resp:
            chunk = up.recv(4096)
            if not chunk:
                break
            resp += chunk
        status = resp.split(b"\r\n")[0]
        if b" 200" not in status:
            log(logpath, {"client": addr[0], "host": host, "upstream_status": status.decode("latin1")})
            client.sendall(resp or b"HTTP/1.1 502 Bad Gateway\r\n\r\n")
            client.close()
            up.close()
            return
        up.settimeout(None)
        client.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n")
        extra = resp.split(b"\r\n\r\n", 1)[1]
        if extra:
            client.sendall(extra)
        if rest:
            up.sendall(rest)
        pipe(client, up)
    except Exception as e:  # noqa: BLE001 - proxy must never die on one client
        log(logpath, {"client": addr[0], "error": repr(e)})
        try:
            client.close()
        except OSError:
            pass


def main():
    bind_host, bind_port, logpath = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    up = urlparse(os.environ["HTTPS_PROXY"])
    upstream = (up.hostname, up.port)
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind((bind_host, bind_port))
    srv.listen(512)
    while True:
        c, a = srv.accept()
        threading.Thread(target=handle, args=(c, a, upstream, logpath), daemon=True).start()


if __name__ == "__main__":
    main()
