# AivoBlock arXiv — Step 13 Citation and Scientific-Claim Audit

**23 October 2026. Review-only; NOT SUBMITTED, NOT ENDORSED, NOT PEER REVIEWED.**

This is a limited audit of the existing manuscript and its published evidence, not certification of novelty, academic acceptance, or author rights.

## Verified publication records

| Ref | Original source | Audit result |
|---|---|---|
| [2] | [USENIX OSDI 1999](https://www.usenix.org/conference/osdi-99/practical-byzantine-fault-tolerance) | Castro and Liskov's Byzantine fault-tolerance research exists; it is not a proof of AivoBlock's correctness |
| [3] | [Entropy 21(8), 723 (2019)](https://doi.org/10.3390/e21080723) | Coin.AI title, authors, journal and DOI match |
| [4] | [arXiv:2001.09244](https://arxiv.org/abs/2001.09244) | Author given names are Andrei Lihu, Jincheng Du, Igor Barjaktarevic, Patrick Gerzanics and Mark Harvilla; earlier **training** work |
| [6] | [Springer proceedings](https://link.springer.com/book/10.1007/978-3-031-15979-4) and [Edinburgh record](https://www.research.ed.ac.uk/en/publications/ofelimos-combinatorial-optimization-viaproof-of-useful-work/) | **Resolved:** cite published chapter title “Ofelimos: Combinatorial Optimization via Proof-of-Useful-Work,” LNCS 13508, pp. 339–369 (2022), DOI 10.1007/978-3-031-15979-4_12. The longer subtitle appears in some manuscripts/indexes. |
| [7] | [arXiv:2404.09005](https://arxiv.org/abs/2404.09005) | Title, author group, training-based incentive security match |
| [8] | [arXiv:2504.09971v4](https://arxiv.org/abs/2504.09971v4) | **Resolved for v4:** 13 November 2025, authors **Ilan Komargodski and Omri Weinstein**. Earlier indexed versions include Itamar Schen; citations should specify v4 to avoid mixing versions. |
| [9] | [Elsevier publisher record](https://doi.org/10.1016/j.bcra.2025.100387) | Systematic review and article number 100387 supported |
| [10] | [ACM Computing Surveys](https://doi.org/10.1145/3828717) | Merlina, Garrett, Vitenberg; vol. 58 issue 14 article 358, online July 2026 |
| [11] | [SEDRA original working paper](https://www.sedracoin.com/research/proof-of-useful-inference/) | Explicitly **exploratory, not peer reviewed**; August 2026 |
| [12] | [arXiv:2610.26713](https://arxiv.org/abs/2610.26713) | Wong, Ciampi, Sarkar; submitted 19 October 2026; **preprint** |

Reference [1] matches the original Bitcoin whitepaper title and author at bitcoin.org. Reference [5] matches the AivoBlock v0.1.0 GitHub release's stated attribution; individual contributor-credit permissions still require confirmation. Reference [6] published chapter title and [8] arXiv v4 author list are now resolved. The author must still review all bibliographic formatting and URLs. Source checking does not mean a venue has accepted the work.

## Scientific evidence and boundaries

- **Fixed experimental archive:** `v0.1.0-research-prototype`; source `ade6ff42737d972658e9c309f9a2d8cffbfb988c`; run `39740779280`.
- **270 chain tasks across three conditions**; **90** trained-model observations show **730.760 ms median** and **779.342 ms nearest-rank p95** client-*observed* request-to-finality with **100 ms polling**.
- The separate **1.880 ms CPU-service median** has a different measurement boundary; no valid end-to-end speedup/overhead ratio follows from its comparison to chain finality.
- The **0.742 ms direct inference** and **3.412 ms full-block verification** results are different microbenchmark scopes, not system-wide finality estimates.
- Three repeated testnets per condition on one CI environment are not 90 independent network deployments. Different hardware and an independently rerun live experiment are **not** established.
- Validators **recompute all AI inference**, rather than verifying a cheap cryptographic proof. The protocol's committee is **permissioned**; no permissionless mining-security or economic-efficiency claim is established.

## Final author/venue checks still required

- [ ] Personally review the manuscript and every source, and revise any AI-assisted wording; independently verify scholarly claims.
- [x] Resolve [6] publisher title and [8] current arXiv v4 byline; verify [1] original Bitcoin whitepaper metadata and [5] release attribution against their hosted records.
- [ ] Personally confirm that AivoBlock contributors are credited appropriately and approve all reference formatting.
- [ ] Confirm proposed author, UAB affiliation, ORCID, contribution credit, funding, conflicts, rights and AI-assistance disclosure.
- [ ] Decide the arXiv paper distribution license; code licensing is separate.
- [ ] Obtain arXiv cs.DC endorsement and check ACMSE's double-anonymous/preprint rules before public posting.
- [ ] Verify arXiv's compiled PDF and approve the exact submission version.

The CS 499 homework has different instructions requiring the **student to write the paper personally**; this public research draft is not automatically suitable as that assignment.

Run `python3 submissions/arxiv-cs-dc/audit_preprint.py` to check **internal references, numbers and approval flags only**. This script cannot certify citations' external validity or acceptance.
