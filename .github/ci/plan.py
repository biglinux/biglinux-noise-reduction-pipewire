"""Select application checks once, using the complete Git diff, not API pages.

Unknown files and unavailable history select all checks. A failed API lookup
never suppresses tests. The token is read-only and used only for PR discovery.
"""

from __future__ import annotations

import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
from urllib.parse import urlencode
from urllib.request import Request, urlopen

CHECKS = (
    "rust", "native", "dependencies", "i18n", "shell", "flatpak",
    "plasma", "spelling", "secrets",
)


def all_checks(enabled: bool = True) -> dict[str, bool]:
    return dict.fromkeys(CHECKS, enabled)


def classify(paths: list[str] | None) -> dict[str, bool]:
    if paths is None:
        return all_checks()
    result = all_checks(False)
    for path in paths:
        result["spelling"] = result["secrets"] = True
        name = PurePosixPath(path).name
        if path == ".github/workflows/ci.yml" or path.startswith(
            (".github/ci/", ".github/actions/")
        ):
            return all_checks()
        if name.startswith("LICENSE") or path in ("deny.toml", "approved-crates.txt"):
            result["dependencies"] = True
        elif path.endswith(".md") or path.startswith("docs/"):
            continue
        elif path.startswith("po/"):
            result["i18n"] = True
        elif name in ("Cargo.toml", "Cargo.lock"):
            for check in ("rust", "native", "dependencies", "flatpak", "i18n"):
                result[check] = True
        elif path.endswith(".rs") or path.startswith(".cargo/") or name == "rust-toolchain.toml":
            result["rust"] = result["native"] = result["i18n"] = True
        elif path.startswith("vendor/"):
            result["rust"] = result["native"] = True
        elif path.startswith("packaging/flatpak/"):
            result["flatpak"] = True
            result["shell"] |= path.endswith(".sh")
        elif path.startswith("usr/"):
            result["native"] = True
            if path.endswith((".qml", ".js")):
                result["plasma"] = result["i18n"] = True
        elif path == "tests/plasmoid_status.test.cjs":
            result["plasma"] = True
        elif path.endswith(".sh"):
            result["shell"] = True
            result["native"] |= path in ("scripts/test-ui.sh", "scripts/test-pipewire.sh")
            result["rust"] |= path == "scripts/test-miri.sh"
            result["i18n"] |= path == "scripts/refresh-pot.sh"
        elif path == ".github/workflows/security.yml" or path == ".github/dependabot.yml":
            result["dependencies"] = True
        elif path.startswith(".github/workflows/") or path == "_typos.toml":
            continue
        else:
            # Never silently miss a new build input or a new test language.
            return all_checks()
    return result


def covered_by_pr(event: dict, repository: str, pulls: list[dict]) -> bool:
    ref = event.get("ref", "")
    if not ref.startswith("refs/heads/") or ref == "refs/heads/main":
        return False
    branch = ref.removeprefix("refs/heads/")
    after = event.get("after")
    if not after:
        return False
    return any(
        pr.get("state") == "open"
        and pr.get("base", {}).get("ref") == "main"
        and pr.get("head", {}).get("ref") == branch
        and pr.get("head", {}).get("sha") == after
        and pr.get("head", {}).get("repo", {}).get("full_name") == repository
        for pr in pulls
    )


def discover_duplicate(event: dict) -> bool:
    repository = os.environ["GITHUB_REPOSITORY"]
    ref = event.get("ref", "")
    token = os.environ.get("GH_TOKEN")
    if not token or ref == "refs/heads/main" or not ref.startswith("refs/heads/"):
        return False
    query = urlencode({
        "state": "open", "base": "main", "per_page": 100,
        "head": repository.split("/", 1)[0] + ":" + ref.removeprefix("refs/heads/"),
    })
    api = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")
    request = Request(f"{api}/repos/{repository}/pulls?{query}", headers={
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "biglinux-ci-planner",
    })
    try:
        with urlopen(request, timeout=15) as response:
            pulls = json.load(response)
        return covered_by_pr(event, repository, pulls)
    except (OSError, ValueError, TypeError, AttributeError):
        print("PR discovery unavailable; keeping the push checks.", file=sys.stderr)
        return False


def changed_paths(event_name: str, event: dict) -> list[str] | None:
    if event_name == "pull_request":
        base = event.get("pull_request", {}).get("base", {}).get("sha", "")
    elif event_name == "push":
        base = event.get("before", "")
    else:
        return None
    if not re.fullmatch(r"[0-9a-fA-F]{40,64}", base) or set(base) == {"0"}:
        return None
    try:
        diff = subprocess.run(
            ["git", "diff", "--name-only", "-z", "--no-renames", base, "HEAD", "--"],
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30,
        ).stdout
        return [os.fsdecode(name) for name in diff.split(b"\0") if name]
    except (OSError, subprocess.SubprocessError):
        print("Diff unavailable; selecting all checks.", file=sys.stderr)
        return None


def main() -> None:
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    event_name = os.environ["GITHUB_EVENT_NAME"]
    duplicate = event_name == "push" and discover_duplicate(event)
    selected = all_checks(False) if duplicate else classify(changed_paths(event_name, event))
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, enabled in selected.items():
            print(f"{name}={str(enabled).lower()}", file=output)
    reason = "Push covered by an open PR at the same revision." if duplicate else "Checks selected from the complete change set."
    summary = reason + "\n\n" + "\n".join(
        f"- {name}: {'selected' if enabled else 'not applicable'}" for name, enabled in selected.items()
    ) + "\n"
    print(summary)
    if path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(path, "a", encoding="utf-8") as output:
            output.write(summary)


if __name__ == "__main__":
    main()
