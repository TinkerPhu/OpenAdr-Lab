#!/usr/bin/env python3
"""Are the digest-pinned base images in this repo behind their tags?

Pinning a base image by digest is what keeps a build cacheable across a
`docker system prune` (see tests/Dockerfile). The cost is that security updates
stop arriving implicitly: the tag moves, the pin does not. This check is the
other half of that trade — it asks the registry what each pinned tag points at
today and reports the ones that have moved.

It finds its own work rather than being told: every `FROM <name>:<tag>@sha256:…`
line in any Dockerfile under the repo is checked, so pinning a second image is
covered without touching this script.

Only digest-pinned lines are reported. An unpinned `FROM name:tag` is a
different decision, not a stale pin, and saying anything about it here would
make the output noise.

Exit codes: 0 = every pin current, 1 = at least one has moved, 2 = the check
itself could not run. The third is deliberately distinct: "the registry was
unreachable" must never read as "your pins are current".

Usage:
    python scripts/check_base_image_digests.py [--json]
"""

import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TIMEOUT_S = 20

# A manifest *list* / OCI *index* is what a multi-arch tag resolves to, and the
# index digest is what a Dockerfile should pin: the docker hosts here are arm64
# while local WSL is x86_64, so a per-platform digest would break one of them.
# Both media types are requested because Docker Hub serves either depending on
# how the image was pushed.
ACCEPT = ", ".join(
    [
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
    ]
)

# FROM python:3.12-slim@sha256:dddf...  (optionally `AS builder`)
FROM_PINNED = re.compile(
    r"^\s*FROM\s+(?P<name>[^\s:@]+):(?P<tag>[^\s@]+)@(?P<digest>sha256:[0-9a-f]{64})",
    re.IGNORECASE | re.MULTILINE,
)


def pinned_bases() -> "list[dict]":
    """Every digest-pinned FROM in the repo's Dockerfiles, with its source file."""
    # `worktrees/` is skipped deliberately: a git worktree holds a full copy of
    # the tree, so scanning it from the main checkout would report the same pin
    # once per worktree and make the output depend on who has what checked out.
    skip = {"node_modules", "worktrees", ".git", "target"}
    found = []
    for path in sorted(REPO.rglob("Dockerfile*")):
        if not path.is_file():
            continue
        rel = path.relative_to(REPO)
        # Match on the path *relative* to the repo root: `rglob` yields absolute
        # paths, and testing those would let the repo's own location exclude
        # everything (a checkout living under a directory called `worktrees`
        # silently finds nothing).
        if skip.intersection(rel.parts):
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for m in FROM_PINNED.finditer(text):
            found.append(
                {
                    "file": str(rel).replace("\\", "/"),
                    "image": m.group("name"),
                    "tag": m.group("tag"),
                    "pinned": m.group("digest"),
                }
            )
    return found


def _repository(image: str) -> str:
    """Docker Hub's API path for an image name.

    A bare `python` is `library/python` on Hub; `org/name` is already complete.
    An image from another registry is not handled — raise rather than guess, so
    a future non-Hub pin fails loudly instead of being silently reported current.
    """
    if "/" not in image:
        return f"library/{image}"
    if image.count("/") == 1 and "." not in image.split("/")[0]:
        return image
    raise ValueError(f"not a Docker Hub image, cannot check: {image}")


def current_digest(image: str, tag: str) -> str:
    """What `image:tag` resolves to in the registry right now."""
    repository = _repository(image)
    token_url = (
        "https://auth.docker.io/token"
        f"?service=registry.docker.io&scope=repository:{repository}:pull"
    )
    with urllib.request.urlopen(token_url, timeout=TIMEOUT_S) as r:
        token = json.load(r)["token"]

    # HEAD, not GET: the digest is a response header, so there is no reason to
    # pull the manifest body.
    req = urllib.request.Request(
        f"https://registry-1.docker.io/v2/{repository}/manifests/{tag}",
        method="HEAD",
        headers={"Authorization": f"Bearer {token}", "Accept": ACCEPT},
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT_S) as r:
        digest = r.headers.get("Docker-Content-Digest")
    if not digest:
        raise ValueError(f"registry returned no digest for {image}:{tag}")
    return digest


def main() -> int:
    as_json = "--json" in sys.argv
    bases = pinned_bases()
    if not bases:
        # Nothing pinned is a valid state, and not this check's business.
        print("No digest-pinned base images found - nothing to check.")
        return 0

    results, unreachable = [], False
    for base in bases:
        entry = dict(base)
        try:
            entry["current"] = current_digest(base["image"], base["tag"])
            entry["stale"] = entry["current"] != base["pinned"]
        except (urllib.error.URLError, ValueError, KeyError, OSError) as exc:
            entry["error"] = str(exc)
            unreachable = True
        results.append(entry)

    if as_json:
        print(json.dumps({"bases": results}, indent=2))
    else:
        for e in results:
            ref = f"{e['image']}:{e['tag']}"
            if "error" in e:
                print(f"?  {ref} ({e['file']}): could not check - {e['error']}")
            elif e["stale"]:
                print(f"STALE  {ref} ({e['file']})")
                print(f"       pinned  {e['pinned']}")
                print(f"       current {e['current']}")
            else:
                print(f"ok     {ref} ({e['file']}) is current")
        if any(e.get("stale") for e in results):
            print(
                "\nTo bump one: replace the digest in that Dockerfile's FROM line, then "
                "rebuild the affected image and run its suite - a new base can change "
                "system packages, so this is a code change, not a bookkeeping edit."
            )

    if unreachable:
        return 2
    return 1 if any(e.get("stale") for e in results) else 0


if __name__ == "__main__":
    sys.exit(main())
