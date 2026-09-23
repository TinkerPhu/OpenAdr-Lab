#!/usr/bin/env python3
"""Per-VEN broker credentials, derived from one root secret.

Twenty VENs shared one `openadr-fleet` credential, which means any VEN could
publish as any other: spoofed telemetry, spoofed trace, and a fleet view with
no way to tell. The fix is a credential per VEN plus an ACL that pins each one
to its own topic — but twenty secrets kept by hand on two hosts drift, and a
twenty-first VEN means editing both again.

So they are *derived*: `HMAC-SHA256(root secret, ven name)`. One secret to
keep, any number of VENs, and the same VEN always gets the same password on
both hosts without them ever exchanging anything.

**The root secret never leaves the operator's hands.** It is not given to the
VENs — a VEN holding it could derive every sibling's password, which is the
attack the ACL exists to stop. This script runs where the root lives and emits
only each VEN its own password.

Two shapes come out of this, because two consumers need different things:

`--env-file` writes one aggregate file of `MQTT_FLEET_PW_VEN_<n>=…`, which is
what the broker reads: its start-up command loops over that environment and
creates one account per entry.

`--per-ven-dir` writes one file *per VEN*, each holding the generic
`FLEET_MQTT_PASSWORD=…` that the VEN itself reads. This is not a convenience.
Compose's `env_file:` supplies the container's environment and nothing else —
it is NOT a source for `${…}` interpolation in the same service's
`environment:` block. Pointing a VEN at the aggregate file and writing
`FLEET_MQTT_PASSWORD: "${MQTT_FLEET_PW_VEN_1}"` therefore resolves to the empty
string while the file sits right there looking like it works. A file per VEN
carrying the name the app actually reads removes the interpolation step, and
with it the trap.

Usage:
    # once, to create a root secret
    python3 scripts/gen_fleet_mqtt_secrets.py --new-root

    # then, with MQTT_FLEET_ROOT_SECRET set (or --root):
    # the broker's accounts:
    python3 scripts/gen_fleet_mqtt_secrets.py --env-file VTN/.env.fleet --vens 1-20
    # each VEN's own password:
    python3 scripts/gen_fleet_mqtt_secrets.py --per-ven-dir VEN --vens 1-3
    python3 scripts/gen_fleet_mqtt_secrets.py --per-ven-dir VEN/scale_out/node2 --vens 4-20

The generated files are git-ignored by design; regenerate rather than copy.
"""

import argparse
import base64
import hashlib
import hmac
import os
import secrets
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent


def derive(root: str, ven_name: str) -> str:
    """This VEN's broker password.

    Base64url without padding: mosquitto passwords are opaque strings, and the
    alphabet avoids anything a shell, a compose file or a `.env` parser would
    treat as syntax.
    """
    digest = hmac.new(root.encode(), ven_name.encode(), hashlib.sha256).digest()
    return base64.urlsafe_b64encode(digest).decode().rstrip("=")


def ven_names(spec: str) -> "list[str]":
    """`4-20` or `1,3,7` -> ven names, in order."""
    names = []
    for part in spec.split(","):
        part = part.strip()
        if "-" in part:
            lo, hi = (int(x) for x in part.split("-", 1))
            names.extend(f"ven-{i}" for i in range(lo, hi + 1))
        elif part:
            names.append(f"ven-{int(part)}")
    return names


def env_var(ven_name: str) -> str:
    """`ven-7` -> `MQTT_FLEET_PW_VEN_7`, the shape compose interpolates."""
    return "MQTT_FLEET_PW_" + ven_name.upper().replace("-", "_")


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__,
                                formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--root", help="root secret (default: $MQTT_FLEET_ROOT_SECRET)")
    p.add_argument("--vens", default="1-20", help="range or list, e.g. 4-20 (default: 1-20)")
    p.add_argument("--env-file",
                   help="write the aggregate MQTT_FLEET_PW_VEN_<n> file the broker reads")
    p.add_argument("--per-ven-dir",
                   help="write one .env.fleet.<ven> per VEN, each with FLEET_MQTT_PASSWORD")
    p.add_argument("--new-root", action="store_true",
                   help="print a fresh root secret and exit, writing nothing")
    p.add_argument("--show", metavar="VEN",
                   help="print one VEN's password, e.g. for a manual mosquitto_sub")
    args = p.parse_args()

    if args.new_root:
        print(secrets.token_urlsafe(32))
        return 0

    root = args.root or os.environ.get("MQTT_FLEET_ROOT_SECRET")
    if not root:
        print(
            "No root secret. Pass --root or set MQTT_FLEET_ROOT_SECRET.\n"
            "Create one with: python3 scripts/gen_fleet_mqtt_secrets.py --new-root",
            file=sys.stderr,
        )
        return 2

    if args.show:
        print(derive(root, args.show))
        return 0

    names = ven_names(args.vens)

    if args.per_ven_dir:
        target_dir = Path(args.per_ven_dir)
        if not target_dir.is_absolute():
            target_dir = REPO_ROOT / target_dir
        for n in names:
            f = target_dir / f".env.fleet.{n}"
            header = (
                "# Generated by scripts/gen_fleet_mqtt_secrets.py - do not edit, regenerate.\n"
                f"# {n}'s own broker password, under the name the VEN itself reads.\n"
                "# Not MQTT_FLEET_PW_VEN_<n>: compose's env_file feeds the container\n"
                "# and is not a source for ${...} interpolation, so a per-VEN variable\n"
                "# resolved that way silently becomes the empty string.\n"
            )
            f.write_text(header + f"FLEET_MQTT_PASSWORD={derive(root, n)}\n",
                         encoding="utf-8")
            f.chmod(0o600)
        print(f"wrote {len(names)} per-VEN file(s) to {target_dir}")
        if not args.env_file:
            return 0

    lines = [
        "# Generated by scripts/gen_fleet_mqtt_secrets.py — do not edit, regenerate.",
        "# Each VEN's own broker password, derived from the root secret. The root",
        "# itself is deliberately absent: a VEN that held it could derive every",
        "# other VEN's password, which is the whole point of having one each.",
        "",
    ]
    lines += [f"{env_var(n)}={derive(root, n)}" for n in names]
    body = "\n".join(lines) + "\n"

    if args.env_file:
        target = Path(args.env_file)
        if not target.is_absolute():
            target = REPO_ROOT / target
        target.write_text(body, encoding="utf-8")
        target.chmod(0o600)
        print(f"wrote {len(names)} credential(s) to {target}")
    else:
        print(body, end="")
    return 0


if __name__ == "__main__":
    sys.exit(main())
