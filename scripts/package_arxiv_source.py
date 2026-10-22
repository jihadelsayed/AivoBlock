#!/usr/bin/env python3
"""Create a self-contained arXiv TeX-source upload candidate, NOT a submission.

The existing author-review ZIP deliberately contains PDF, metadata, and
internal build checks. arXiv instead wants only files needed to build TeX.
We also add a conventional TeX title/author block to the Pandoc export.

A PASS validates packaging and static constraints, not author/rights approval,
arXiv account endorsement, venue acceptance, or scientific originality.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import zipfile
from pathlib import Path

TAG="aivoblock-arxiv-source-candidate-v1"
FIXED_DATE=(2020,1,1,0,0,0)
BODY="main.tex"
EXPECTED_SOURCE="ade6ff42737d972658e9c309f9a2d8cffbfb988c"
EXPECTED_RUN="39740779280"

def require(condition: bool, error: str) -> None:
    if not condition:
        raise ValueError(error)

def pending(meta: dict) -> None:
    require(meta.get("schema") == "aivoblock-preprint-candidate-v1",
            "unexpected metadata schema")
    require(meta.get("publicly_submitted") is False and
            meta.get("arxiv_id") is None and meta.get("preprint_doi") is None,
            "public-submission claim is not authorized")
    require(meta.get("evidence_commit") == EXPECTED_SOURCE and
            meta.get("evidence_run_id") == EXPECTED_RUN, "research provenance changed")
    flags=meta.get("approval")
    require(isinstance(flags,dict) and len(flags)==10 and
            all(value is False for value in flags.values()),
            "author approvals must remain pending")
    creators=meta.get("creators",[])
    require(isinstance(creators,list) and len(creators)==1 and
            creators[0].get("confirmed_by_author") is False,
            "author identity cannot be marked confirmed")

def make_tex(source: str, meta: dict) -> str:
    pending(meta)
    require(source.count(r"\begin{document}") == 1 and
            source.count(r"\end{document}") == 1 and
            r"\documentclass" in source, "not a standalone LaTeX source")
    require(r"\author{}" in source and r"\date{}" in source,
            "Pandoc title metadata changed; review conversion manually")
    title=meta.get("title","")
    creator=meta["creators"][0].get("name","")
    require(isinstance(title,str) and
            re.fullmatch(r"[A-Za-z0-9 :,\-]+",title) is not None,
            "unsafe or unexpected manuscript title")
    require(isinstance(creator,str) and
            re.fullmatch(r"[A-Za-z .'\-]+",creator) is not None,
            "unsafe or unexpected author text")
    # Pandoc renders the Markdown H1 as a section. Convert just that section
    # to title metadata, without changing any measured values/body text.
    heading=re.compile(r"\\section\{AivoBlock:[\s\S]+?\}\\label\{[^}]+\}\s*")
    rewritten,count=heading.subn("",source,count=1)
    require(count == 1, "expected unique Pandoc manuscript H1")
    rewritten=rewritten.replace(r"\author{}",
                                rf"\title{{{title}}}"+"\n"+rf"\author{{{creator}}}",1)
    rewritten=rewritten.replace(r"\date{}",r"\date{22 October 2026}",1)
    rewritten=rewritten.replace(r"\begin{document}",
                                r"\begin{document}"+"\n"+r"\maketitle"+"\n",1)
    # Pandoc's source keeps some research-data URLs as plain text rather
    # than link commands. Wrap only *bare* URLs (never href{target}) so TeX
    # can break them across lines. xurl also breaks long hyphenated paths.
    require(r"\usepackage{lmodern}" in rewritten,
            "Pandoc LaTeX preamble changed; inspect URL package placement")
    rewritten=rewritten.replace(r"\usepackage{lmodern}",
                                r"\usepackage{xurl}"+"\n"+r"\usepackage{lmodern}",1)
    rewritten=re.sub(r"(?<![\\{])https?://[^\s{}\\]+",
                     lambda m: r"\url{" + m.group(0) + "}", rewritten)
    # Long commit hashes are otherwise unbreakable inside \texttt and
    # can extend beyond the text block. \path preserves the exact value,
    # while xurl supplies safe breaks inside a long alphanumeric string.
    rewritten=re.sub(r"\\texttt\{([0-9a-f]{40})\}",
                     lambda m: r"\path{" + m.group(1) + "}", rewritten)
    # Any external include would require dependent files in the ZIP. Fail
    # rather than produce a paper with missing data/figures/references.
    require(re.search(r"\\(?:input|include|includegraphics|bibliography|addbibresource|lstinputlisting)\b",
                      rewritten) is None, "external TeX dependencies must be bundled")
    require("Generative AI assistance" in rewritten and
            EXPECTED_SOURCE in rewritten, "disclosures or evidence references missing")
    return ("% Draft only: authorship, license, and publication need explicit approval.\n"
            "% This file has not been submitted to arXiv.\n"
            +rewritten)

def create(tex_path: Path, meta_path: Path, out: Path) -> dict:
    meta=json.loads(meta_path.read_text(encoding="utf-8"))
    raw=tex_path.read_text(encoding="utf-8")
    prepared=make_tex(raw,meta).encode("utf-8")
    require(len(prepared)>10000 and len(prepared)<1_000_000,
            "unexpected LaTeX size")
    out.parent.mkdir(parents=True,exist_ok=True)
    with zipfile.ZipFile(out,"w") as archive:
        info=zipfile.ZipInfo(BODY,date_time=FIXED_DATE)
        info.compress_type=zipfile.ZIP_DEFLATED
        info.create_system=3
        info.external_attr=0o100644<<16
        archive.writestr(info,prepared)
    verified=verify(out)
    require(verified["sha256"]==hashlib.sha256(prepared).hexdigest(),
            "round-trip archive content differs")
    return verified

def verify(path: Path) -> dict:
    with zipfile.ZipFile(path) as archive:
        require(archive.namelist()==[BODY],
                "arXiv upload archive must contain exactly main.tex, no PDF or metadata")
        content=archive.read(BODY)
        require(content.startswith(b"% Draft only:") and
                r"\maketitle" in content.decode("utf-8") and
                r"\begin{document}" in content.decode("utf-8") and
                r"\end{document}" in content.decode("utf-8"),
                "invalid standalone TeX candidate")
        require(b"Generative AI assistance" in content and
                EXPECTED_SOURCE.encode() in content,
                "publication disclosure or experimental provenance missing")
        return {"verdict":"PASS", "schema":TAG,
                "file":BODY,"bytes":len(content),
                "sha256":hashlib.sha256(content).hexdigest(),
                "publication_status":"DRAFT; NOT SUBMITTED"}

def main() -> None:
    parser=argparse.ArgumentParser(description=__doc__)
    commands=parser.add_subparsers(dest="command",required=True)
    make=commands.add_parser("create")
    make.add_argument("--tex",type=Path,required=True)
    make.add_argument("--metadata",type=Path,required=True)
    make.add_argument("--out",type=Path,required=True)
    check=commands.add_parser("verify")
    check.add_argument("archive",type=Path)
    args=parser.parse_args()
    result=(create(args.tex,args.metadata,args.out)
            if args.command=="create" else verify(args.archive))
    print(json.dumps(result,indent=2,sort_keys=True))

if __name__=="__main__":
    main()
