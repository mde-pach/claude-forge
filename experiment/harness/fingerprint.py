"""Runs INSIDE a trial container (system python3, as root) and prints a JSON fingerprint
of the environment the agent will see: every file under /work (minus .git internals
other than HEAD/config/hooks) and /home/dev, with sha256, mode and symlink targets.
Big dependency trees (.venv, node_modules) are hashed as a whole (sorted path+size+mtime-free
content hash would be slow), via a cheap listing hash of names and sizes."""
import hashlib
import json
import os
import stat
import sys

ROOTS = ["/work", "/home/dev"]
TREE_DIRS = {".venv", "node_modules"}  # summarised: names + sizes only (installed deps, identical by construction)


def file_sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def tree_digest(top):
    h = hashlib.sha256()
    for d, dirs, files in os.walk(top):
        dirs.sort()
        for n in sorted(files):
            p = os.path.join(d, n)
            try:
                st = os.lstat(p)
            except OSError:
                continue
            h.update(f"{os.path.relpath(p, top)}\0{st.st_size}\0".encode())
    return h.hexdigest()


def main():
    out = {}
    for root in ROOTS:
        for d, dirs, files in os.walk(root):
            rel_d = os.path.relpath(d, "/")
            if rel_d == "work/.git":
                # keep only what defines behaviour: HEAD, config, hooks
                dirs[:] = [x for x in dirs if x == "hooks"]
                files = [f for f in files if f in ("HEAD", "config")]
            keep = []
            for x in sorted(dirs):
                if x in TREE_DIRS:
                    out["/" + os.path.join(rel_d, x) + "/**"] = "tree:" + tree_digest(os.path.join(d, x))
                else:
                    keep.append(x)
            dirs[:] = keep
            for n in sorted(files):
                p = os.path.join(d, n)
                try:
                    st = os.lstat(p)
                except OSError:
                    continue
                if stat.S_ISLNK(st.st_mode):
                    out[p] = "link:" + os.readlink(p)
                elif stat.S_ISREG(st.st_mode):
                    out[p] = f"{oct(st.st_mode & 0o777)}:{file_sha(p)}"
    json.dump(out, sys.stdout, sort_keys=True)


if __name__ == "__main__":
    main()
