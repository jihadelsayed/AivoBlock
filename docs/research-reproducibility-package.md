# AivoBlock — reproducible public evidence package

The private testnet CI can produce a **single public research-data ZIP** with
the exact experiment inputs, observations, phase summaries, per-request
finalized-block evidence, and an integrity manifest. It is intended to support
a **prototype research preprint**; it is not a patent filing, a peer-reviewed
result, or independent confirmation of broad blockchain/AI performance claims.

## Contents and provenance

The artifact **research-evidence-bundle** contains
\`aivoblock-research-evidence.zip\` with:

- \`manifest.json\`: SHA-256 digest and byte count for every packaged file,
  source-head commit, **actual CI checkout** commit, CI run ID, and limitations.
- \`REPRODUCE.md\`: command-line verification and analysis instructions.
- \`evidence/research-out/\`: the trained digits QMLP-V1 export, main research
  report, and the three independent dataset/model seed reports.
- \`evidence/testnet/generated/\`: verifier and research summaries, three
  benchmark invocations, workload manifest, separate phase summaries for
  seeded/fast-poll/trained runs, CPU service observations, the exact service
  model/input/result artifact bytes, all 270 raw chain observations, launch
  reports, and the **90 finalized QMLP block records** referenced by the
  trained model's 90 raw observations.

Every included path is explicitly allowlisted; Docker logs, private testnet
keys, local node storage and credentials are excluded. For the trained-model
scenario, the builder independently checks each referenced finalized block's
task ID and block ID against the observation record. It rejects missing,
duplicate, inconsistent or modified summary and service artifact provenance.

A GitHub Actions *pull request* normally tests a temporary merge checkout;
both that tested checkout SHA and the PR source-head SHA are preserved. The
archive is stable for the same input bytes and identifiers. The SHA hashes
detect accidental/unauthorized changes **relative to the manifest**; the
manifest itself must be obtained from a trusted GitHub Actions run.

## Obtain and verify

Install [GitHub CLI](https://cli.github.com/) and Python 3.11+. From a clone
of this repository, download a **successful** GitHub CI run that executed the
Docker testnet job and produced \`research-evidence-bundle\`:

\`\`\`bash
gh run download RUN_ID -R jihadelsayed/AivoBlock \
  -n research-evidence-bundle -D publication
python3 scripts/package_research_evidence.py verify \
  publication/aivoblock-research-evidence.zip
unzip publication/aivoblock-research-evidence.zip -d publication/unpacked
\`\`\`

The ZIP includes \`REPRODUCE.md\` with the exact source and checkout hashes.
Use a released or explicitly chosen successful main-branch run for the final
citation rather than describing a PR's temporary merge SHA as a permanent
release. Artifacts expire under repository retention settings, so the final
paper should archive this ZIP with a permanent DOI through an appropriate
research-data repository (later publication milestone).

## Recalculate the descriptive chain observations

\`\`\`bash
python3 scripts/summarize_chain_phases.py \
  --directory publication/unpacked/evidence/testnet/generated/trained-qmlp-chain \
  --out publication/trained-qmlp-summary.json
python3 scripts/summarize_chain_phases.py \
  --directory publication/unpacked/evidence/testnet/generated/pilot-measurements \
  --out publication/seeded-1s-summary.json
python3 scripts/summarize_chain_phases.py \
  --directory publication/unpacked/evidence/testnet/generated/fast-poll-measurements \
  --out publication/seeded-100ms-summary.json
\`\`\`

These calculations require **no Docker** and do not invent timing results.
To **rerun** the experiment (not merely reanalyze prior measurements),
check out the recorded source SHA, ensure Docker + Compose v2 and
Python 3.11+ are installed, then follow the [private testnet guide](testnet.md)
and CI workflow. Running the full workflow is substantially more expensive
and can produce different times on different machines or under contention.

## Scientific limitations

- Private three-node permissioned testnet, not an open/public blockchain.
- QMLP-V1 is a small deterministic integer network; validators recompute it
  in full, not via succinct or general-purpose proof of AI computation.
- Finality measurements are *client observed*, include CLI startup, Python
  execution, transport/consensus, and status polling; they are **not**
  consensus-only, network-only, or AI-only wall-clock measurements.
- CPU service observations use an **in-process service-core** boundary.
  Blockchain observations use a larger request-to-observed-finality
  boundary. No speedup or causal overhead ratio is justified.
- 30 tasks share the same testnet per launch. Three launches are not
  90 independent network deployments or cross-machine replication.
- Performance figures are descriptive for the tested GitHub-hosted runner,
  not a claim about throughput, energy, or production scalability.
- GitHub CI passing is not independent replication or peer review.
- JSONL observations may reference ephemeral absolute paths on the original
  runner; corresponding model, input, and result artifacts are included
  separately using the launch-indexed folders in the bundle.

The next milestone is **independent reproduction and audit** using a fixed,
retrievable bundle, followed by a research preprint grounded in its actual
verified results.
