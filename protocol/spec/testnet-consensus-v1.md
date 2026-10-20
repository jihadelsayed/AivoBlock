# Private-testnet consensus and networking V1

Status: implemented in `node/src/testnet/` for the **private research testnet**.
Permissioned, fixed validator set; no stake, slashing, rotation or public use.

## Configuration trust

`network.json` lists validators (Ed25519 key, voting power, P2P address),
registered AI workers, the issuance policy, the round timeout and the signed
genesis checkpoint. The authority signs

```text
"AIVOBLOCK-TESTNET-CONFIG-V1\0" || chain_id:u32 || validator_set_commitment
|| n:u32 || (key[32] || power:u64 || len:u8 || address)*   (sorted by key)
|| workers:u32 || worker_key[32]*                          (sorted)
|| issuance_policy_bytes || round_timeout_ms:u64 || genesis_id
```

A node accepts the file only if the signature and the genesis checkpoint verify
under `AIVOBLOCK_AUTHORITY_PUBLIC_KEY`, which is pinned out of band and must
equal the key in the file. Names are cosmetic and unsigned.

## Transport

- TCP with the bounded 4-byte length framing (`network::tcp`, max 4 MiB).
- Mutual challenge-response: each side sends a fresh 32-byte challenge, answers
  the peer's challenge with the signed `AIVOBLOCK-PEER-HELLO-V1` message, and
  accepts the peer only if its key is a configured validator, is not its own,
  and (when dialing) equals the key configured for that address. Zero or
  reflected challenges are rejected.
- One link per pair: the smaller key dials; reconnects every 0.5 s.
- Messages are JSON envelopes (`status`, `task`, `claim`, `proposal`, `vote`,
  `commit`, `get_blocks`) carrying hex of canonical binary encodings. A
  malformed frame closes the link. **The channel is not encrypted and messages
  are not MACed after the handshake**; safety relies on every consensus object
  being independently authenticated (authority-signed tasks, worker-signed
  claims, validator-signed votes, self-verifying blocks + certificates).

## Work flow

1. The task authority submits an `AiTaskV3` (see `ai-work-block-v3.md`) whose
   `sequence` is the height that must settle it, with partitions assigned to
   currently connected registered workers. Nodes verify it and flood it.
2. Each node whose worker is assigned a partition runs the Python worker,
   refuses to sign if the result differs from the Rust reference, signs a
   claim and floods it. Every receiving node verifies the claim (assignment,
   signature, Rust recomputation) before storing or forwarding it.
3. Rejections: forged/foreign authority, expired (`deadline < next height`),
   stale sequence, already-settled task, overlapping/gapped partitions,
   unregistered workers, manipulated results, wrong worker, forged signature,
   conflicting claims. Duplicates are ignored and counted.

## Consensus per height `h`

- Proposals are deterministic: the block for (parent, task, verified claims)
  is unique (deterministic timestamp and issuance, ordered claims,
  deterministic Ed25519), so honest leaders of any round propose the same
  block.
- Leader of `(h, r)` = validators sorted by key `[(h + r) mod n]`.
- On a proposal, a validator re-executes it completely (decode, task,
  every claim recomputed, issuance schedule, Haskell settlement cross-checked
  by the Rust mirror, state root). It then precommits
  `PRECOMMIT-V1(chain, set commitment, h, r, block)` **only if** it has not
  precommitted a different block at `h`; the lock (block ID and bytes) is
  fsynced to RocksDB before the vote is sent and survives restarts.
- More than 2/3 of voting power for the same `(h, r, block)` forms a
  `QuorumCertificateV1`. Only then is the block persisted with its certificate,
  height index, successor ledger and replay markers in one synced RocksDB
  write — this is the only point at which Sayed Token balances change.
- The finalizer broadcasts `commit(block, certificate)`; peers verify the
  certificate **and** re-execute the block before persisting it.
- Round timeout: `round_timeout_ms * (1 + min(r, 3))` while work is pending;
  the next leader re-proposes (its locked block if it has one).
- Sync: peers advertise `(height, tip)` every second; a node behind sends
  `get_blocks(from)` and receives up to 64 certified commits per request.
  Commits for future heights are buffered and applied in order.

## Safety and liveness

- Two conflicting blocks cannot both be certified at one height unless more
  than 1/3 of voting power signs two blocks at the same height (equivocates),
  because each certificate needs > 2/3 and honest validators lock one block per
  height. With 4 equal validators this tolerates 1 Byzantine or crashed node;
  with 3 it tolerates none (quorum is 3 of 3).
- Liveness assumes partial synchrony and a single valid task per height. A
  Byzantine leader proposing a *different valid* block (e.g. a second
  authority-signed task for the same height) can split locks and stall the
  height; there is no unlock / proof-of-lock round. Equivocation evidence and
  proposer signatures are not implemented.

## Restart recovery

On start a node verifies the stored genesis against the signed configuration,
then replays every finalized block from the height index: certificate check,
full block re-execution (including Haskell settlement) and state-root check.
The final ledger must equal the persisted snapshot, otherwise the node refuses
to start. It then syncs any missing certified blocks from peers.
