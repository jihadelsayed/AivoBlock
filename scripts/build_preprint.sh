#!/usr/bin/env bash
# Build an author-review PDF/TeX. Never submit to public repositories.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
python3 scripts/check_preprint_package.py

output="research/paper/build"
mkdir -p "$output"
cp research/paper/aivoblock-manuscript.md "$output/aivoblock-preprint-source.md"
cp research/paper/submission-metadata.json "$output/submission-metadata.json"
python3 scripts/check_preprint_package.py --out "$output/submission-readiness.json"

# Official Pandoc + TeX Live image; no local TeX install required.
pandoc_image="pandoc/latex:3.11.0.0-debian"
render() {
  docker run --rm --user "$(id -u):$(id -g)" \
    --volume "$repo":/data --workdir /data "$pandoc_image" \
    research/paper/aivoblock-manuscript.md \
    --from markdown+pipe_tables+fenced_code_blocks \
    --standalone \
    --variable documentclass:article \
    --variable fontsize:10pt \
    --variable geometry:margin=0.80in \
    --variable colorlinks:true \
    --variable linkcolor:blue \
    --variable urlcolor:blue \
    --variable lang:en-US \
    "$@"
}
render --to latex --output "$output/aivoblock-preprint.tex"
render --pdf-engine=xelatex --output "$output/aivoblock-preprint.pdf"

python3 - <<'PY'
from pathlib import Path
from hashlib import sha256
import json,os
build=Path("research/paper/build")
expected=["aivoblock-preprint.pdf","aivoblock-preprint.tex",
          "aivoblock-preprint-source.md","submission-metadata.json",
          "submission-readiness.json"]
for name in expected:
    path=build/name
    if not path.is_file() or path.stat().st_size<100:
        raise SystemExit(f"missing or empty build output: {name}")
manifest="\n".join(f"{sha256((build/name).read_bytes()).hexdigest()}  {name}"
                   for name in sorted(expected))+"\n"
(build/"SHA256SUMS.txt").write_text(manifest)
info={
    "status":"DRAFT; NOT SUBMITTED",
    "source_head_sha":os.environ.get("GITHUB_SHA","unversioned-local-checkout"),
    "run_id":os.environ.get("GITHUB_RUN_ID","local"),
    "files":expected,
    "all_rights_reserved_software":True,
    "author_approvals_complete":False,
}
(build/"build-provenance.json").write_text(json.dumps(info,indent=2)+"\n")
print("PASS: produced PDF and LaTeX draft; no public submission")
PY
