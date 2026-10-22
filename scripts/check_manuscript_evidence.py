#!/usr/bin/env python3
"""Fail-closed numerical manuscript audit against the published v0.1.0 archive.

This proves table/report consistency, not originality, peer review or correctness.
"""
import argparse
import hashlib
import io
import json
import zipfile
from pathlib import Path

SOURCE = "ade6ff42737d972658e9c309f9a2d8cffbfb988c"
RUN = "39740779280"
ARCHIVE_SHA256 = "a489c37ef5f14c68e08b623eb01477cbc4507ed7b7326cddfcaf9a75745ef414"


def require(ok, message):
    if not ok:
        raise ValueError(message)


def rows_in_section(text, section):
    part = next((s for s in text.split("## ") if s.startswith(section)), None)
    require(part is not None, "missing manuscript section " + section)
    table = {}
    for line in part.splitlines():
        if line.startswith("|") and not line.startswith("| ---"):
            fields = [x.strip().replace("**", "") for x in line.strip("|").split("|")]
            if len(fields) > 1:
                table[fields[0]] = fields[1:]
    return table


def audit(paper, archive):
    require(len(paper.split()) >= 2600, "incomplete manuscript")
    for heading in ("## Abstract", "## 1. Introduction", "## 2. System design",
                    "## 3. Experimental design", "## 4. Results", "## 5. Discussion",
                    "## 6. Threats to validity", "## 7. Future work",
                    "## 8. Conclusion", "## References"):
        require(heading in paper, "missing section " + heading)
    require(SOURCE in paper and RUN in paper and ARCHIVE_SHA256 in paper,
            "paper is not bound to this research release")
    require(hashlib.sha256(archive).hexdigest() == ARCHIVE_SHA256,
            "published ZIP digest mismatch")
    with zipfile.ZipFile(io.BytesIO(archive)) as z:
        manifest = json.loads(z.read("manifest.json"))
        require(manifest["source_head_sha"] == SOURCE and
                manifest["tested_checkout_sha"] == SOURCE and
                str(manifest["ci_run_id"]) == RUN, "manifest source mismatch")
        require(len(z.namelist()) == 147 and len(manifest["entries"]) == 145,
                "unexpected research archive membership")
        def load(name):
            return json.loads(z.read("evidence/" + name))
        base = load("research-out/report.json")
        variation = load("testnet/generated/variability-summary.json")
        bench = load("testnet/generated/standalone-comparison.json")
        cpu = load("testnet/generated/service-baseline/summary.json")
        scenarios = [
            ("Seeded QMLP, 1.0 s", "phase-summary-1s-seeded.json"),
            ("Seeded QMLP, 0.1 s", "phase-summary-100ms-seeded.json"),
            ("Trained QMLP, 0.1 s", "phase-summary-100ms-trained.json"),
        ]
        experiments = [load("testnet/generated/" + path) for _, path in scenarios]

    model = rows_in_section(paper, "4.1 ")
    for title, val in (
        ("PyTorch float-model test accuracy", base["float"]["accuracy"] * 100),
        ("Quantized QMLP-V1 test accuracy", base["qmlp"]["accuracy"] * 100),
        ("Agreement between quantized and float predictions",
         base["qmlp"]["agreement_with_float"] * 100),
    ):
        require(model.get(title) == [f"{val:.1f}%"], "model accuracy differs: " + title)
    require(model.get("Integer ONNX vs QMLP-V1 reference mismatches") ==
            [str(base["onnx_integer"]["mismatched_rows"]) + "/500"], "ONNX data differ")
    expected_seeds = ", ".join(f"{value:.1f}%" for value in
                               variation["accuracy_percent"]["float"]["observations"])
    require(expected_seeds in paper and "(seeds 11, 17 and 23, respectively)" in paper,
            "paper mismatches float-model seed replication order")
    require(variation["accuracy_percent"]["qmlp"]["observations"] == [96.0, 96.0, 96.0]
            and "quantized accuracies **96.0%** in each run" in paper,
            "paper mismatches quantized-model replication")

    verifiers = rows_in_section(paper, "4.2 ")
    candidates = {
        "Direct integer QMLP inference (same workload)": (
            bench["main_comparison"]["standalone_qmlp_inference_us"], 1000, 4),
        "Rust-mirror full-block verification": (
            bench["main_comparison"]["full_block_verify_rust_mirror_us"], 1000, 3),
        "Persistent Haskell cross-checked full-block verification": (
            bench["main_comparison"]["full_block_verify_haskell_persistent_us"], 1000, 3),
        "Fresh Haskell process full-block verification": (
            variation["benchmark"]["qmlp_haskell_one_shot_ms"], 1, 3),
    }
    for title, (record, divisor, sd_places) in candidates.items():
        expected = [f'{record["mean"]/divisor:.3f}',
                    f'{record["sample_stddev"]/divisor:.{sd_places}f}']
        require(verifiers.get(title) == expected,
                "verifier benchmark figure differs: " + title)

    finality = rows_in_section(paper, "4.3 ")
    for (title, _), experiment in zip(scenarios, experiments):
        record = experiment["pooled"]["total_ms"]
        values = ["90 (3 × 30)"] + [
            f"{record[field]:.3f}" for field in
            ("mean_ms", "median_ms", "p95_nearest_rank_ms", "sample_stddev_ms")]
        require(finality.get(title) == values, "finality table differs: " + title)
    trained = experiments[2]["pooled"]
    for metric in ("cli_submission_ms", "post_submission_to_observed_finality_ms"):
        require(f'{trained[metric]["median_ms"]:.3f}' in paper,
                "missing phase boundary value")
    for key in ("mean_ms", "median_ms", "p95_ms"):
        require(f'{cpu[key]:.3f}' in paper, "CPU service number differs")
    for warning in ("full recomputation", "not an independent rerun",
                    "different timing boundary", "permissioned",
                    "not a production", "must not be interpreted as a speedup or overhead ratio"):
        require(warning.casefold() in paper.casefold(), "missing scope limit " + warning)
    return {"verdict":"PASS", "source_sha":SOURCE, "ci_run":RUN,
            "audited_chain_observations":270, "audited_cpu_service_observations":90,
            "scope":"published-release numerical consistency, not scientific peer review"}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--archive", required=True, type=Path)
    p.add_argument("--paper", type=Path,
                   default=Path("research/paper/aivoblock-manuscript.md"))
    args = p.parse_args()
    print(json.dumps(audit(args.paper.read_text(), args.archive.read_bytes()), indent=2))


if __name__ == "__main__":
    main()
