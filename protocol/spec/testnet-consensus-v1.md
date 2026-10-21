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

- TCP with bounded 4-byte length framing (`network::tcp`, max 4 MiB for
  encrypted application records). **Before authentication**, each challenge is
  capped at 32 bytes, each signed hello at 136 bytes, and the signed X25519
  offer at 96 bytes. Oversized frame headers are rejected before allocating
  or reading the advertised body, limiting memory abuse during handshakes.
- Mutual challenge-response: each side sends a fresh 32-byte challenge, answers
  the peer's challenge with the signed `AIVOBLOCK-PEER-HELLO-V1` message, and
  accepts the peer only if its key is a configured validator, is not its own,
  and (when dialing) equals the key configured for that address. Zero or
  reflected challenges are rejected.
- Every inbound and outbound peer must complete **both** the challenge/hello
  exchange and signed X25519 negotiation within a single **10-second absolute
  deadline**. Each blocking socket read/write uses the smaller of the normal
  three-second I/O timeout and remaining handshake time; partial frame bytes
  cannot renew the total budget. Expired connections close and release inbound
  admission capacity. Authenticated links return to their normal read/write
  timeouts after negotiation.
- One link per pair: the smaller key dials; reconnects every 0.5 s.
  The dialer permits **at most one active outbound connection worker per
  configured validator key**, counting the entire TCP connection, handshake,
  and authenticated-link lifetime. Duplicate dial attempts are skipped until
  that worker exits, including after authentication or connection failures.
  Other validator targets are not blocked by a stalled target.
- The TCP listener admits at most **32 concurrent inbound, unauthenticated
  handshakes**, with a further maximum of **8 pending handshakes per source IP**.
  Extra connections are closed before allocating a worker thread. Both admission
  slots are released on authentication failure or after the signed X25519
  exchange completes. IPv4-mapped IPv6 sources share the IPv4 address's limit;
  unused IP entries are removed. Outbound dialing and established encrypted
  validator links are not counted. This protects capacity for other sources
  but is **not a full DDoS defense**. Validators
  sharing one NAT/public IP share the same eight pending slots.
- To reduce short-lived handshake churn, the listener permits at most **16
  inbound TCP connection attempts per source IP per one-second fixed window**,
  checked before a worker is spawned. IPv4-mapped IPv6 addresses share their
  IPv4 counter. A bounded map tracks at most **256 source IPs**; expired
  one-second entries are reclaimed when the map is full, and new sources are
  rejected while all tracked entries are fresh. This is a best-effort source
  rate limiter, **not** a network-wide flood defense; validators behind one NAT
  share a rate budget, and many-source floods can still deny service.
- The local operator control API is **unauthenticated** and can inject
  synthetic consensus messages for testing. Node startup therefore rejects
  any operator control listener bound to a non-loopback or wildcard address
  (including `0.0.0.0` and `::`). `127.0.0.1` and `::1` are allowed. This
  prevents accidental public exposure through the listener address; it is not
  a substitute for same-host process isolation or operator authentication.
- The local operator control listener admits at most **16 concurrent TCP
  client handlers**. Each accepted client holds an atomic reservation until
  its one-request handler exits; excess sockets are closed without launching
  threads. Admission recovers after disconnects, timeouts and malformed
  requests. Every complete inbound request frame and outbound response frame
  now has its **own 10-second absolute deadline**, including headers and
  payloads. Each read/write uses at most the remaining budget (and the
  three-second per-operation I/O timeout), so drip-fed bytes cannot hold a
  control handler indefinitely. The node may still spend up to 30 seconds
  waiting for the corresponding control event reply. This bounds local
  control-handler lifetime but does not authenticate callers or replace
  trusted loopback-only exposure.
- The node dispatches peer, operator-control and AI-worker events through a
  shared **128-event bounded channel** rather than an unbounded queue. When the
  consumer falls behind, producers must wait for queue capacity, which applies
  TCP backpressure to the authenticated peer reader before it can accept more
  complete frames. This bounds queued event *count*, not CPU cost or the
  individual message size; it does not replace network-wide flood protection.
- Each configured validator identity is permitted at most **128 fully
  verified encrypted frames per one-second fixed window**, shared across its
  authenticated TCP links and reconnects. The 129th verified frame closes
  the offending connection before admitting more work to the node event queue.
  Reconnecting or opening another link does not reset the budget; a new time
  window does. Independent configured validators have independent budgets.
  Only authenticated configured identities enter this map, so its size is
  bounded by the validator set. This is not a network-wide DDoS defense.
- Messages are JSON envelopes (`status`, `task`, `claim`, `proposal`, `vote`,
  `commit`, `get_blocks`) carrying hex of canonical binary encodings.
- For each handshake, both nodes compute a shared SHA-256 session ID over a
  domain tag, the chain ID, both validator public keys and the fresh challenge
  generated by each validator, sorted by validator public key.
- After validator authentication, each side **must** send a fresh
  X25519 public key and an Ed25519 signature over
  `AIVOBLOCK-ENCRYPTED-HANDSHAKE-V1\0 || chain_id || session_id ||
  sender_id || recipient_id || ephemeral_pub`. Both signatures must verify,
  and low-order X25519 results are rejected. There is **no plaintext fallback**.
- The session ID is the existing challenge- and identity-bound SHA-256 digest.
  HKDF-SHA256 with this session ID as salt derives separate 256-bit traffic
  keys for sender → receiver and receiver → sender, using
  `AIVOBLOCK-DIRECTIONAL-AEAD-V1\0 || chain_id || sender_id || recipient_id`
  as direction-specific HKDF info. This is an ephemeral session: reconnects
  exchange fresh keys and cannot reuse ciphertext from older sessions.
- After key exchange, each peer message is an outer bounded TCP frame of
  ChaCha20-Poly1305 ciphertext (16-byte authentication tag). The ciphertext,
  **including its tag**, must fit within the 4 MiB outer-frame limit. The
  sender rejects oversized messages before writing any outer-frame bytes.
  Each complete encrypted frame (header plus ciphertext) must arrive within
  one **15-second absolute deadline**; partial reads cannot renew the budget.
  After a complete authenticated frame, a new 15-second deadline starts for the
  next frame. Each outbound encrypted frame (header plus ciphertext) also has
  one **15-second absolute send deadline**; all socket writes use the smaller of
  the normal 3-second per-write limit and the remaining total time. Partial
  writes cannot renew the deadline. A send failure closes the connection without
  reusing its sequence number; successful sends restore normal socket timeouts.
  A stalled or incomplete frame closes the connection, matching the existing
  15-second peer inactivity policy.
  The inner plaintext is the signed peer frame
  `version:u8=1 || sequence:u64_be || Ed25519-signature:64 || JSON`
  prefixed by its own 4-byte size. The nonce is `00000000 || sequence:u64_be`
  (sequence begins at 1); AEAD associated data includes a domain tag,
  chain ID, session ID, sender/recipient identities, and sequence. Per-direction
  keys and monotonically increasing sequence numbers prevent nonce reuse.
- Inner frame signatures cover the domain tag, chain ID, session ID, sender and
  recipient, sequence, length, and exact JSON payload. A receiver accepts only
  the immediate next sequence; failed decryption, signatures, replay, skipped
  sequence, malformed frame or unsupported version terminate the link. The
  replay counter advances only after the entire decrypted inner frame has
  been validated, including checks for trailing data.
- The encrypted validator transport hides P2P payload bytes against passive
  network observers. An on-path adversary can still see network endpoints,
  drop, delay or reset traffic. The local control API, endpoint compromise,
  and public-node discovery are not covered. Consensus objects remain
  independently verified. Older plaintext peer implementations are
  incompatible.

## Work flow

1. The task authority submits an `AiTaskV3` (see `ai-work-block-v3.md`) whose
   `sequence` is the height that must settle it, with partitions assigned to
   currently connected registered workers. Nodes verify it and flood it.
2. Each node whose worker is assigned a partition runs the Python worker,
   refuses to sign if the result differs from the Rust reference, signs a
   claim and floods it. Every receiving node verifies the claim (assignment,
   signature, Rust recomputation) before storing or forwarding it. The node
   runs at most **8 AI task worker executions concurrently**, even with up to
   256 accepted pending tasks. Excess eligible tasks remain queued for later
   scheduler ticks; success, failure, and stale task completion release a
   concurrency slot. A failed task remains marked attempted to avoid automatic
   retry storms. The cap bounds local concurrency, not total work or CPU usage.
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
