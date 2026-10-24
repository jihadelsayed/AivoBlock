#!/usr/bin/env bash
# Only builds a review artifact. No external publication or submission.
set -euo pipefail
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
source_dir="$repo_root/submissions/acmse-2027"
output_dir="$source_dir/build"
template_url="https://acmse.net/2027/wp-content/2027-templates/acmart-master-acmse2027-v0.1.zip"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$output_dir" "$tmp/vendor" "$tmp/work"

curl --fail --location --retry 3 --retry-delay 3 --connect-timeout 20 --max-time 120 --silent --show-error "$template_url" -o "$tmp/official.zip"
python3 - "$tmp/official.zip" "$tmp/vendor" <<'PY'
import pathlib, sys, zipfile
archive, target = map(pathlib.Path, sys.argv[1:])
with zipfile.ZipFile(archive) as file:
    members = file.infolist()
    if not members or len(members) > 3000:
        raise SystemExit('Official conference ZIP is empty or unexpectedly large')
    for member in members:
        name = pathlib.PurePosixPath(member.filename)
        if name.is_absolute() or '..' in name.parts:
            raise SystemExit(f'Unsafe path in conference template: {member.filename}')
    file.extractall(target)
PY
sample="$(find "$tmp/vendor" -name sample-sigconf.tex -type f -print -quit)"
class="$(find "$tmp/vendor" -name acmart.cls -type f -print -quit)"
bst="$(find "$tmp/vendor" -name ACM-Reference-Format.bst -type f -print -quit)"
for file in "$sample" "$class" "$bst"; do
  if [[ -z "$file" || ! -f "$file" ]]; then
    echo 'Official template is missing sample-sigconf.tex, acmart.cls, or ACM-Reference-Format.bst' >&2
    exit 1
  fi
done
cp "$class" "$bst" "$tmp/work/"
cp "$source_dir/poster-abstract.tex" "$source_dir/references.bib" "$tmp/work/"
sha256sum "$tmp/official.zip" | tee "$output_dir/official-template.sha256"
printf '%s\n' "$template_url" > "$output_dir/official-template-url.txt"
(
  cd "$tmp/work"
  latexmk -pdf -bibtex -interaction=nonstopmode -halt-on-error -file-line-error poster-abstract.tex
)
cp "$tmp/work/poster-abstract.pdf" "$tmp/work/poster-abstract.log" "$tmp/work/poster-abstract.bbl" "$output_dir/"
python3 "$source_dir/check_submission.py" --pdf "$output_dir/poster-abstract.pdf"
# The venue explicitly demands Linux Biolinum for the title and Linux Libertine otherwise.
pdffonts "$output_dir/poster-abstract.pdf" | grep -q 'LinBiolinum'
pdffonts "$output_dir/poster-abstract.pdf" | grep -q 'LinLibertine'
printf 'PASS: official-template ACMSE review PDF built and preflighted\n'
