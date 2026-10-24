#!/usr/bin/env python3
"""Fail-closed source-only checks for the draft external reviewer handoff.

Does not solicit or perform independent review, and never grants approval.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "research/review"
SOURCE = "ade6ff42737d972658e9c309f9a2d8cffbfb988c"
RUN = "39740779280"
SHA = "a489c37ef5f14c68e08b623eb01477cbc4507ed7b7326cddfcaf9a75745ef414"
ALLOWED_CONTACT_STATES = {"not_requested", "requested", "response_received"}

def validate(base=BASE):
    packet = (base / "README.md").read_text(encoding="utf-8")
    form = (base / "REVIEW_FORM.md").read_text(encoding="utf-8")
    obj = json.loads((base / "STATUS.json").read_text(encoding="utf-8"))
    assert obj.get("schema") == "aivoblock-independent-review-tracker-v1"
    assert obj.get("original_evidence_commit") == SOURCE
    assert obj.get("original_evidence_run_id") == RUN
    assert obj.get("original_archive_sha256") == SHA
    for value in (SOURCE, RUN, SHA):
        assert value in packet, f"Reviewer packet lacks immutable evidence marker {value}"
    for marker in ("python3 scripts/testnet.py all",
                   "scripts/audit_public_evidence.py",
                   "not an independent execution",
                   "730.760", "1.880", "full recomputation"):
        assert marker.lower() in packet.lower(), f"Missing reproducibility limit: {marker}"
    for marker in ("Reproduction detail", "Critical findings", "Reviewer consent"):
        assert marker in form, f"Review template missing {marker}"
    assert obj.get("external_reviewer_contact_status") in ALLOWED_CONTACT_STATES
    assert obj.get("external_review_report_received") in (True, False)
    assert obj.get("independent_live_experiment_replication_verified") in (True, False)
    if obj["external_review_report_received"]:
        assert obj["external_reviewer_contact_status"] == "response_received"
        assert isinstance(obj.get("review_report_url"), str) and obj["review_report_url"].startswith("https://"), "Missing verifiable review evidence"
    else:
        assert obj.get("review_report_url") is None, "Review report link without received review"
    if obj["independent_live_experiment_replication_verified"]:
        assert isinstance(obj.get("new_replication_report_url"), str) and obj["new_replication_report_url"].startswith("https://"), "Missing new-replication artifact"
    else:
        assert obj.get("new_replication_report_url") is None
    for pending in ("arxiv_endorsement_verified","acmse_preprint_anonymity_guidance_received","publication_author_approved"):
        assert obj.get(pending) is False, f"Draft gate {pending} must not be silently approved"
    return True

if __name__ == "__main__":
    validate()
    print("PASS: external technical review packet is internally consistent")
    print("NOTE: no independent reviewer, live replication, or endorsement has been verified by this check")
