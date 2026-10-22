#!/usr/bin/env python3
"""Build/verify a public, allowlisted AivoBlock experimental evidence ZIP.

This tool does NOT execute benchmarks or establish independent replication.
Only named research outputs are included; local testnet keys, RocksDB state,
logs and generated private credentials must never enter the release package.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import zipfile

SCHEMA = "aivoblock-evidence-v1"
HEX_SHA = re.compile(r"^[0-9a-f]{40}$")
FIXED_DATE = (2020, 1, 1, 0, 0, 0)
SCENARIOS = ("pilot-measurements", "fast-poll-measurements", "trained-qmlp-chain")
SUMMARIES = (
    "research-summary.json", "variability-summary.json",
    "standalone-comparison.json", "workload-manifest.json",
    "polling-comparison.json", "trained-qmlp-identity.json",
    "chain-evidence-audit.json", "finalized-commitment-presence.json",
    "phase-summary-1s-seeded.json", "phase-summary-100ms-seeded.json",
    "phase-summary-100ms-trained.json", "bench.json",
    "bench-replicate-1.json", "bench-replicate-2.json", "bench-replicate-3.json",
)


def file_list() -> list[str]:
    paths = ["research-out/report.json", "research-out/digits.qmlp.json"]
    paths += [f"research-out/replicates/seed-{seed}/report.json" for seed in (11, 17, 23)]
    paths += [f"testnet/generated/{name}" for name in SUMMARIES]
    paths += ["testnet/generated/service-baseline/service-fixture.json",
              "testnet/generated/service-baseline/summary.json"]
    for idx in range(3):
        paths += [f"testnet/generated/service-baseline/report-{idx}.json",
                  f"testnet/generated/service-baseline/launch-{idx}.jsonl"]
        paths += [
            f"testnet/generated/service-baseline/launch-{idx}-artifacts/{name}.json"
            for name in ("model", "input", "result")
        ]
        for scenario in SCENARIOS:
            base = f"testnet/generated/{scenario}/launch-{idx:03d}"
            paths += [base + ".json", base + "/three-node/report.json"]
            if scenario == "trained-qmlp-chain":
                paths += [
                    f"{base}/three-node/finalized-block-evidence/{n:03d}.json"
                    for n in range(30)
                ]
    if len(paths) != len(set(paths)):
        raise RuntimeError("duplicate allowlist paths")
    return sorted(paths)


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def read_safe(root: Path, relative: str) -> bytes:
    path = root / relative
    if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"missing, linked, or unsafe evidence source: {relative}")
    if path.stat().st_size > 50_000_000:
        raise ValueError(f"unexpectedly large evidence source: {relative}")
    return path.read_bytes()


def validate_sources(blobs: dict[str, bytes]) -> None:
    for scenario, summary in (
        ("pilot-measurements", "phase-summary-1s-seeded.json"),
        ("fast-poll-measurements", "phase-summary-100ms-seeded.json"),
        ("trained-qmlp-chain", "phase-summary-100ms-trained.json"),
    ):
        result = json.loads(blobs[f"testnet/generated/{summary}"])
        if result.get("method", {}).get("total_observations") != 90:
            raise ValueError(f"{summary}: expected 90 validated observations")
        per_launch = result.get("per_launch")
        if not isinstance(per_launch, list) or len(per_launch) != 3:
            raise ValueError(f"{summary}: missing per-launch provenance")
        seen = set()
        for launch in per_launch:
            name = launch.get("source_file")
            if name not in {f"launch-{i:03d}.json" for i in range(3)} or name in seen:
                raise ValueError(f"{summary}: invalid launch source")
            seen.add(name)
            key = f"testnet/generated/{scenario}/{name}"
            if launch.get("sha256") != digest(blobs[key]) or launch.get("samples") != 30:
                raise ValueError(f"{summary}: source-file digest/count mismatch")
            observations = json.loads(blobs[key])
            if not isinstance(observations, list) or len(observations) != 30:
                raise ValueError(f"{name}: missing raw observations")
            if scenario == "trained-qmlp-chain":
                for idx, record in enumerate(observations):
                    rel = record.get("finalized_block_evidence_path")
                    expected = (f"launch-{int(name[7:10]):03d}/three-node/"
                                f"finalized-block-evidence/{idx:03d}.json")
                    if rel != expected:
                        raise ValueError(f"{name}: invalid finalized block reference")
                    block = json.loads(blobs[f"testnet/generated/{scenario}/{rel}"])
                    if (block.get("block_id") != record.get("block_id")
                            or block.get("task_id") != record.get("task_id")):
                        raise ValueError(f"{name}: finalized block identity mismatch")
    for idx in range(3):
        path = f"testnet/generated/service-baseline/launch-{idx}.jsonl"
        lines = [json.loads(line) for line in blobs[path].decode().splitlines() if line.strip()]
        if len(lines) != 30:
            raise ValueError(f"{path}: expected 30 CPU service observations")
        for row in lines:
            for name in ("model", "input", "result"):
                actual_path = (f"testnet/generated/service-baseline/"
                               f"launch-{idx}-artifacts/{name}.json")
                declared = row.get(f"artifact_{name}_sha256")
                if declared != digest(blobs[actual_path]):
                    raise ValueError(f"{path}: {name} artifact digest mismatch")


def readme(revision: str, checkout_sha: str, run_id: str) -> str:
    return f"""# AivoBlock public research evidence bundle

Source branch/head commit: \`{revision}\`
Exact CI checkout tested: \`{checkout_sha}\`
GitHub Actions run: https://github.com/jihadelsayed/AivoBlock/actions/runs/{run_id}

The \`evidence/\` directory preserves source-relative paths. The repository's
\`scripts/package_research_evidence.py verify\` checks each archive entry
against \`manifest.json\`. SHA-256 checks integrity, NOT external attestation.

Reproduce the descriptive summaries with Python 3.11+ (standard library only):

\`\`\`bash
python3 scripts/package_research_evidence.py verify aivoblock-research-evidence.zip
unzip aivoblock-research-evidence.zip -d aivoblock-evidence
python3 scripts/summarize_chain_phases.py \\
  --directory aivoblock-evidence/evidence/testnet/generated/trained-qmlp-chain \\
  --out /tmp/trained-phase-summary.json
\`\`\`

Retrieve the source associated with this experimental bundle:

\`\`\`bash
git clone https://github.com/jihadelsayed/AivoBlock.git
cd AivoBlock
git fetch origin {revision}
git checkout {revision}
\`\`\`

The CI checkout may be GitHub's temporary pull-request merge revision,
distinct from the recorded source head. A successful workflow validates
the source against its then-current base, not necessarily a future release.

Experimental scope: permissioned local Docker testnet, three fresh launches,
30 finalized QMLP requests per launch, CPU service baseline recorded separately.
Client timing includes status polling and is NOT consensus-only or AI-only time.
Do NOT report a service-vs-chain speedup/overhead ratio, causal polling impact,
production scalability, third-party attestation, or network security from this
bundle. Some original JSONL metadata may contain temporary CI filesystem paths;
the corresponding artifact bytes are preserved in this ZIP.

Not included: testnet secret keys, local volumes, private node identities,
RocksDB state, container logs, or live service credentials. This bundle is not
the final journal paper, an independently reproduced experiment, or peer review.
"""


def zip_info(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, date_time=FIXED_DATE)
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    return info


def create(root: Path, out: Path, revision: str, checkout_sha: str, run_id: str) -> dict:
    if not HEX_SHA.fullmatch(revision) or not HEX_SHA.fullmatch(checkout_sha):
        raise ValueError("both Git commit hashes must be full 40-character lowercase SHAs")
    if not isinstance(run_id, str) or not run_id.isdigit():
        raise ValueError("numeric GitHub Actions run ID required")
    files = file_list()
    blobs = {name: read_safe(root, name) for name in files}
    validate_sources(blobs)
    entries = [
        {"path": f"evidence/{name}", "sha256": digest(blobs[name]), "bytes": len(blobs[name])}
        for name in files
    ]
    manifest = {
        "schema": SCHEMA, "source_head_sha": revision,
        "tested_checkout_sha": checkout_sha, "ci_run_id": run_id,
        "hash_algorithm": "sha256", "entries": entries,
        "scope": "observed permissioned-testnet and off-chain CPU research evidence",
        "limitations": [
            "not an independently reproduced experiment",
            "not independent third-party verification",
            "not equivalent service-vs-finality timing boundaries",
            "not a measure of consensus-only, network-only or AI-only execution",
            "not production or permissionless network evidence",
        ],
    }
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w") as archive:
        archive.writestr(zip_info("manifest.json"), json.dumps(manifest, sort_keys=True, indent=2) + "\n")
        archive.writestr(zip_info("REPRODUCE.md"), readme(revision, checkout_sha, run_id))
        for name in files:
            archive.writestr(zip_info(f"evidence/{name}"), blobs[name])
    verify(out)
    return manifest


def verify(package: Path) -> dict:
    with zipfile.ZipFile(package) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)) or any(
            PurePosixPath(name).is_absolute()
            or any(part in (".", "..") for part in PurePosixPath(name).parts)
            or name.startswith("/") for name in names
        ):
            raise ValueError("duplicate or unsafe archive paths")
        if "manifest.json" not in names or "REPRODUCE.md" not in names:
            raise ValueError("missing package manifest or reproduction instructions")
        manifest = json.loads(archive.read("manifest.json"))
        if manifest.get("schema") != SCHEMA or manifest.get("hash_algorithm") != "sha256":
            raise ValueError("unsupported evidence manifest")
        entries = manifest.get("entries")
        if not isinstance(entries, list) or not entries:
            raise ValueError("missing evidence manifest entries")
        expected = {"manifest.json", "REPRODUCE.md"}
        for item in entries:
            name, sha, size = item.get("path"), item.get("sha256"), item.get("bytes")
            if not isinstance(name, str) or not name.startswith("evidence/") or name in expected:
                raise ValueError("duplicate or unsafe evidence path")
            expected.add(name)
            data = archive.read(name)
            if digest(data) != sha or len(data) != size:
                raise ValueError(f"evidence checksum/size mismatch: {name}")
        if set(names) != expected:
            raise ValueError("unlisted or missing archive member")
        return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    make = sub.add_parser("create")
    make.add_argument("--root", type=Path, default=Path("."))
    make.add_argument("--out", required=True, type=Path)
    make.add_argument("--source-revision", required=True)
    make.add_argument("--checkout-revision", required=True)
    make.add_argument("--run-id", required=True)
    check = sub.add_parser("verify")
    check.add_argument("package", type=Path)
    args = parser.parse_args()
    if args.action == "create":
        result = create(args.root, args.out, args.source_revision,
                        args.checkout_revision, args.run_id)
    else:
        result = verify(args.package)
    print(f"PASS: {len(result['entries'])} allowlisted, SHA-256-verified evidence files")


if __name__ == "__main__":
    main()
