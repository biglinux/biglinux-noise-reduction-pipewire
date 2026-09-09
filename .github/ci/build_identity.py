"""Record Cargo's actual path dependencies and verify their Git provenance.

No compilation, tests, allocator probing or source mutation is performed.
Only public source inputs are recorded, not environment or authentication.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess


def capture(*arguments: str) -> str:
    return subprocess.run(arguments, check=True, text=True,
                          stdout=subprocess.PIPE, timeout=120).stdout


def main() -> None:
    root = Path.cwd().resolve()
    metadata = json.loads(capture("cargo", "metadata", "--locked", "--format-version", "1"))
    packages = [package for package in metadata["packages"] if package["name"] == "big-os-kit"]
    if len(packages) != 1:
        raise SystemExit("Expected exactly one resolved big-os-kit package")
    package = packages[0]
    expected = root / "vendor/big-rust-components/crates/foundation/big-os-kit/Cargo.toml"
    actual = Path(package["manifest_path"]).resolve()
    if actual != expected.resolve() or package["source"] is not None:
        raise SystemExit(f"Unexpected subprocess dependency source: {actual}")

    git = ("git", "-c", f"safe.directory={root}")
    files = [actual, root / "Cargo.lock", root / "tests/review_subprocess.rs"]
    files += sorted((actual.parent / "src").glob("subprocess*.rs"))
    files += sorted((actual.parent / "src/subprocess").glob("*.rs"))
    inputs = {}
    for path in files:
        relative = path.relative_to(root).as_posix()
        content = path.read_bytes()
        expected_blob = capture(*git, "rev-parse", f"HEAD:{relative}").strip()
        observed_blob = capture(*git, "hash-object", "--", relative).strip()
        if observed_blob != expected_blob:
            raise SystemExit(f"Build input changed after checkout: {relative}")
        entry = {"git_blob": observed_blob, "sha256": hashlib.sha256(content).hexdigest()}
        if path.suffix == ".rs":
            entry["source"] = content.decode("utf-8")
        inputs[relative] = entry
        print(f"BUILD_INPUT {observed_blob} {relative}")
    report = {
        "checkout": capture(*git, "rev-parse", "HEAD").strip(),
        "package_id": package["id"],
        "manifest_path": str(actual),
        "targets": package["targets"],
        "target_directory": metadata["target_directory"],
        "inputs": inputs,
    }
    directory = root / ".ci-results/native"
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "build-inputs.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"RESOLVED_SUBPROCESS {package['id']} {actual}")
    for filename, entry in inputs.items():
        if filename.endswith("subprocess.rs") and "pub struct BigSubprocessOutput" in entry.get("source", ""):
            excerpt = entry["source"].split("pub struct BigSubprocessOutput", 1)[1].split("\n}", 1)[0]
            print("SUBPROCESS_RESULT_CONTRACT\npub struct BigSubprocessOutput" + excerpt + "\n}")


if __name__ == "__main__":
    main()
