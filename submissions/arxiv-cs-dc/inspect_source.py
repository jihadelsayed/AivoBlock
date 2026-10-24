#!/usr/bin/env python3
"""Inspect the isolated arXiv source ZIP and its newly compiled PDF."""
from pathlib import Path
from hashlib import sha256
from zipfile import ZipFile
from pypdf import PdfReader
import json

base = Path("research/paper/build")
zip_path = base / "aivoblock-arxiv-source.zip"
info = json.loads((base / "arxiv-source-report.json").read_text())
assert sha256(zip_path.read_bytes()).hexdigest() == info["archive_sha256"], "Archive checksum mismatch"
with ZipFile(zip_path) as archive:
    assert archive.namelist() == ["main.tex"], "Unexpected ZIP files"
    tex_bytes = archive.read("main.tex")
    assert sha256(tex_bytes).hexdigest() == info["source_sha256"], "Source checksum mismatch"
    workdir = Path("research/paper/arxiv-isolated")
    workdir.mkdir(parents=True, exist_ok=True)
    (workdir / "main.tex").write_bytes(tex_bytes)

pdf = Path("research/paper/arxiv-isolated/main.pdf")
if pdf.is_file():
    reader = PdfReader(str(pdf))
    assert 6 <= len(reader.pages) <= 40, "Unexpected PDF page count"
    content = "\n".join(page.extract_text() or "" for page in reader.pages)
    for marker in ("AivoBlock", "730.760", "References", "Generative AI assistance"):
        assert marker.lower() in content.lower(), f"Missing in compiled PDF: {marker}"
    # The generated PDF should be derived solely from the exported main.tex file.
    print(f"PASS: arXiv ZIP integrity and {len(reader.pages)} source-compiled pages (not published)")
else:
    print("PASS: extracted and verified standalone main.tex; PDF compilation still required")
