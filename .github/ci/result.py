"""A selected-but-skipped job is missing evidence, not a successful check."""

from __future__ import annotations

import json
import os
from pathlib import Path

SELECTORS = {
    "fmt": "rust",
    "native": "native",
    "deny": "dependencies",
    "audit": "dependencies",
    "machete": "rust",
    "typos": "spelling",
    "complexity": "rust",
    "i18n": "i18n",
    "flatpak-manifest": "flatpak",
    "shell": "shell",
    "plasmoid-contracts": "plasma",
    "secrets": "secrets",
}
RELEASE_JOBS = ("msrv", "release-build")


def blocked_jobs(results: dict, release_required: bool) -> dict[str, str]:
    plan = results.get("plan", {})
    if plan.get("result") != "success":
        return {"plan": plan.get("result", "missing")}
    outputs = plan.get("outputs", {})
    invalid = {key for key in SELECTORS.values() if outputs.get(key) not in ("true", "false")}
    if invalid:
        return {"plan": "Missing or invalid selection outputs: " + ", ".join(sorted(invalid))}
    expected = {name for name, key in SELECTORS.items() if outputs[key] == "true"}
    if release_required and outputs["rust"] == "true":
        expected.update(RELEASE_JOBS)
    blocked = {}
    for name in (*SELECTORS, *RELEASE_JOBS):
        status = results.get(name, {}).get("result", "missing")
        if (name in expected and status != "success") or status not in ("success", "skipped"):
            blocked[name] = status
    return blocked


def main() -> None:
    results = json.loads(os.environ["RESULTS"])
    release_required = os.environ.get("RELEASE_REQUIRED") == "true"
    blocked = blocked_jobs(results, release_required)
    lines = ["## CI acceptance", "", "| Check | Result |", "| --- | --- |"]
    lines.extend(f"| {name} | {job.get('result', 'missing')} |" for name, job in results.items())
    lines.extend(["", "Skipped checks are not test passes.",
                  "Release profiles required: " + str(release_required).lower(),
                  "This result does not certify hardware audio or assistive-technology acceptance."])
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with Path(summary).open("a", encoding="utf-8") as stream:
            stream.write("\n".join(lines) + "\n")
    if blocked:
        raise SystemExit(f"CI acceptance blocked: {blocked}")
    print("Every selected check executed successfully; other checks were intentionally out of scope.")


if __name__ == "__main__":
    main()
