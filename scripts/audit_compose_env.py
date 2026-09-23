#!/usr/bin/env python3
"""No compose variable may resolve to an empty string in silence.

Docker Compose interpolates `${VAR}` from the shell and from the project's
`.env` file — and from nowhere else. In particular it does NOT read the
service's own `env_file:`, which supplies the *container's* environment after
interpolation has already happened.

That distinction cost this project a live deployment. Twenty VENs were given
`env_file: .env.fleet` holding `MQTT_FLEET_PW_VEN_1=...` and, four lines below
in the same service block, `FLEET_MQTT_PASSWORD: "${MQTT_FLEET_PW_VEN_1}"`.
It reads as obviously correct. It resolves to `""`. Every VEN connected to the
broker with an empty password, and the only symptom was a line in the broker
log that looked like the ACL rollout working as intended.

Compose does warn, once, into the middle of a build log. So the rule here is
the structural version of that warning: inside our own compose files, a
`${VAR}` must carry a `:-default`. A value that genuinely has no sensible
default belongs in an `env_file` under the name the program reads, not in an
interpolation that quietly becomes empty.

`$${VAR}` is not ours to check — the doubled dollar escapes interpolation and
hands the variable to a shell inside the container, where the container's own
environment (env_file included) is exactly the right source.

Vendored compose files (openleadr-rs/) are upstream's, not ours.
"""

import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
VENDORED = ("openleadr-rs",)

# A single `$` followed by `{NAME}` with no `:-` or `-` default. The negative
# lookbehind is what keeps `$${VAR}` — deliberately escaped — out of it.
BARE = re.compile(r"(?<!\$)\$\{([A-Za-z_][A-Za-z0-9_]*)\}")


def compose_files() -> "list[Path]":
    return sorted(
        p
        for p in REPO_ROOT.rglob("docker-compose*.yml")
        if not any(part in VENDORED for part in p.relative_to(REPO_ROOT).parts)
        and "node_modules" not in p.parts
    )


def findings(path: Path) -> "list[str]":
    out = []
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        for var in BARE.findall(line):
            out.append(f"{path.relative_to(REPO_ROOT)}:{n}: ${{{var}}} has no default")
    return out


def main() -> int:
    problems = [f for p in compose_files() for f in findings(p)]
    if problems:
        print("Compose variables that silently become empty when unset:\n")
        for p in problems:
            print(f"  {p}")
        print(
            "\nGive each a `:-default`, or move the value into the service's\n"
            "env_file under the name the program itself reads — env_file is not\n"
            "a source for ${...} interpolation."
        )
        return 1
    print(f"OK — {len(compose_files())} compose file(s), no defaultless interpolation")
    return 0


if __name__ == "__main__":
    sys.exit(main())
