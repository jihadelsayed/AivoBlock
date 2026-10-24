#!/usr/bin/env bash
# Rebuilds only the contents of the arXiv upload ZIP; never publishes anything.
set -euo pipefail
repo="$(git rev-parse --show-toplevel)"
cd "$repo"
python3 submissions/arxiv-cs-dc/inspect_source.py
for attempt in 1 2; do
  docker run --rm --user "$(id -u):$(id -g)" \
    --volume "$repo":/data \
    --workdir /data/research/paper/arxiv-isolated \
    --entrypoint xelatex \
    pandoc/latex:3.11.0.0-debian \
    -interaction=nonstopmode -halt-on-error main.tex \
    > /dev/null
done
python3 submissions/arxiv-cs-dc/inspect_source.py
cp research/paper/arxiv-isolated/main.pdf research/paper/build/aivoblock-arxiv-source-preview.pdf
rm -rf research/paper/arxiv-isolated
