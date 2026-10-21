#!/usr/bin/env python3
"""AivoBlock private research testnet: build, run and verify end to end.

    python3 scripts/testnet.py demo      # 3 nodes: AI work, consensus, rewards, restart
    python3 scripts/testnet.py fault     # 4 validators: crash + leader crash + recovery
    python3 scripts/testnet.py all       # both, sharing one image build
    python3 scripts/testnet.py demo --skip-build --qmlp-file research-out/digits.qmlp.json
    python3 scripts/testnet.py down --profile three-node

Every Sayed Token balance is predicted independently with the Python reward
reference (ai-engine/src/aivoblock_ai/rewards.py) and compared with every
node. Requires Docker with Compose v2 and Python 3.11+ (stdlib only).
PRIVATE TESTNET ONLY: generated keys are test keys; never use real funds.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "ai-engine" / "src"))
from aivoblock_ai.rewards import IssuancePolicy, distribute_rewards  # noqa: E402

IMAGE = os.environ.get("AIVOBLOCK_IMAGE", "aivoblock-node:testnet")
PROFILES = {
    "three-node": {"validators": 3, "compose": ROOT / "testnet" / "compose.three-node.yaml"},
    "four-validator": {"validators": 4, "compose": ROOT / "testnet" / "compose.four-validator.yaml"},
}
LEDGER_MAGIC = b"AIVOBLOCK-LOCAL-LEDGER-V1\x00"


class Failure(RuntimeError):
    pass


def log(message: str) -> None:
    print(f"[testnet {time.strftime('%H:%M:%S')}] {message}", flush=True)


def check(condition: bool, message: str) -> None:
    if not condition:
        raise Failure(message)
    log(f"PASS {message}")


def run(cmd: list[str], env: dict | None = None, check_rc: bool = True) -> str:
    result = subprocess.run(cmd, env=env, capture_output=True, text=True)
    if check_rc and result.returncode != 0:
        raise Failure(f"command failed ({result.returncode}): {' '.join(cmd)}\n"
                      f"stdout: {result.stdout[-4000:]}\nstderr: {result.stderr[-4000:]}")
    return result.stdout


def last_json(text: str) -> dict:
    text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        for line in reversed(text.splitlines()):
            line = line.strip()
            if line.startswith("{"):
                return json.loads(line)
    raise Failure(f"no JSON in output: {text[-2000:]}")


def summarize(samples: list[int]) -> dict:
    """Order statistics of measured samples (nearest-rank percentiles)."""
    if not samples:
        return {"count": 0}
    ordered = sorted(samples)

    def rank(q: float) -> int:
        return ordered[min(len(ordered) - 1, max(0, round(q * len(ordered)) - 1))]

    return {"count": len(ordered), "min": ordered[0], "median": rank(0.5), "p95": rank(0.95),
            "max": ordered[-1], "mean": sum(ordered) / len(ordered)}


def decode_ledger(raw: bytes) -> dict:
    """Independent decoder for the canonical genesis ledger snapshot."""
    if not raw.startswith(LEDGER_MAGIC):
        raise Failure("genesis ledger has the wrong domain")
    at = len(LEDGER_MAGIC)
    fee_pool = int.from_bytes(raw[at:at + 8], "big")
    at += 8
    count = int.from_bytes(raw[at:at + 4], "big")
    at += 4
    accounts = {}
    for _ in range(count):
        address = raw[at:at + 32].hex()
        balance = int.from_bytes(raw[at + 32:at + 40], "big")
        at += 48
        accounts[address] = balance
    return {"fee_pool": fee_pool, "accounts": accounts}


class Model:
    """Expected chain economics, computed without trusting any node."""

    def __init__(self, network: dict) -> None:
        issuance = network["issuance"]
        self.policy = IssuancePolicy(issuance["initial_reward"], issuance["halving_interval"],
                                     issuance["max_supply"])
        genesis = decode_ledger(bytes.fromhex(network["genesis"]["ledger_hex"]))
        self.fee_pool = genesis["fee_pool"]
        self.balances = dict(genesis["accounts"])
        self.supply = self.fee_pool + sum(self.balances.values())
        self.height = 0
        self.issued = 0
        self.paid = 0

    def apply(self, block: dict) -> None:
        if block["height"] != self.height + 1:
            raise Failure(f"unexpected block height {block['height']}")
        issuance = self.policy.scheduled_issuance(block["height"], self.supply)
        if block["issuance"] != issuance:
            raise Failure(f"block {block['height']} issuance {block['issuance']} != schedule {issuance}")
        units = {}
        for partition in block["partitions"]:
            worker = bytes.fromhex(partition["worker"])
            units[worker] = units.get(worker, 0) + partition["count"]
        total = block["fee_budget"] + issuance
        for worker, amount in distribute_rewards(total, units).items():
            key = worker.hex()
            self.balances[key] = self.balances.get(key, 0) + amount
        self.fee_pool -= block["fee_budget"]
        self.supply += issuance
        self.issued += issuance
        self.paid += total
        self.height = block["height"]


class Testnet:
    def __init__(self, profile: str, out_root: Path) -> None:
        self.profile = profile
        self.validators = PROFILES[profile]["validators"]
        self.compose_file = PROFILES[profile]["compose"]
        self.out = out_root / profile
        self.project = f"aivoblock-{profile}"
        self.env = dict(os.environ)
        self.report: dict = {"profile": profile, "steps": []}
        self.model: Model | None = None
        self.network: dict = {}

    def nodes(self) -> list[str]:
        return [f"node-{i}" for i in range(1, self.validators + 1)]

    def step(self, name: str, **details) -> None:
        self.report["steps"].append({"step": name, "time": time.time(), **details})

    def compose(self, *args: str, check_rc: bool = True) -> str:
        return run(["docker", "compose", "-f", str(self.compose_file), "-p", self.project, *args],
                   env=self.env, check_rc=check_rc)

    def client(self, node: str, *args: str) -> dict:
        return last_json(self.compose("exec", "-T", node, "aivoblock-node", "client", *args))

    def resources(self, label: str) -> dict:
        """One ``docker stats`` snapshot per running container: CPU, memory,
        cumulative network and block I/O since the container started."""
        ids = self.compose("ps", "-q", check_rc=False).split()
        if not ids:
            return {}
        raw = run(["docker", "stats", "--no-stream", "--format", "{{json .}}", *ids], check_rc=False)
        stats = {}
        for line in raw.splitlines():
            line = line.strip()
            if not line.startswith("{"):
                continue
            entry = json.loads(line)
            stats[entry.get("Name", "?")] = {
                key: entry.get(key) for key in ("CPUPerc", "MemUsage", "MemPerc", "NetIO", "BlockIO", "PIDs")
            }
        self.report.setdefault("resources", []).append({"label": label, "time": time.time(), "containers": stats})
        log(f"resources [{label}]: " + "; ".join(
            f"{name} cpu {s['CPUPerc']} mem {s['MemUsage']} net {s['NetIO']}" for name, s in sorted(stats.items())))
        return stats

    def status(self, node: str) -> dict:
        return self.client(node, "status")

    def wait(self, what: str, timeout: float, predicate) -> None:
        started = time.time()
        last_error = None
        while time.time() - started < timeout:
            try:
                if predicate():
                    log(f"ok: {what} ({time.time() - started:.1f}s)")
                    return
            except Failure as error:
                last_error = error
            time.sleep(1)
        raise Failure(f"timed out after {timeout}s waiting for {what}; last error: {last_error}")

    # --- lifecycle -------------------------------------------------------
    def init(self) -> None:
        if self.out.exists():
            shutil.rmtree(self.out)
        self.out.mkdir(parents=True)
        user = [] if os.name == "nt" else ["--user", f"{os.getuid()}:{os.getgid()}"]
        output = run(["docker", "run", "--rm", *user, "-v", f"{self.out}:/out", IMAGE,
                      "testnet-init", "--out", "/out", "--validators", str(self.validators),
                      "--round-timeout-ms", "2000"])
        log(f"generated testnet: {output.strip()}")
        env_line = (self.out / "testnet.env").read_text().strip()
        key, value = env_line.split("=", 1)
        self.env[key] = value
        self.env["AIVOBLOCK_TESTNET_DIR"] = str(self.out)
        self.env["AIVOBLOCK_IMAGE"] = IMAGE
        self.network = json.loads((self.out / "public" / "network.json").read_text())
        self.model = Model(self.network)
        self.step("init", authority=value, validators=self.validators)

    def up(self) -> None:
        self.compose("down", "-v", "--remove-orphans", check_rc=False)
        self.compose("up", "-d")
        self.wait_connected(self.nodes())

    def down(self) -> None:
        if self.compose_file.exists() and "AIVOBLOCK_TESTNET_DIR" in self.env:
            logs = self.compose("logs", "--no-color", check_rc=False)
            (self.out / "compose.log").write_text(logs)
            self.compose("down", "-v", "--remove-orphans", check_rc=False)

    def wait_connected(self, nodes: list[str]) -> None:
        expected = len(nodes) - 1

        def connected() -> bool:
            return all(len(self.status(n)["peers"]) == expected for n in nodes)

        self.wait(f"{len(nodes)} nodes mutually authenticated", 180, connected)
        for node in nodes:
            status = self.status(node)
            peers = sorted(p["name"] for p in status["peers"])
            log(f"{node}: validator {status['validator'][:12]}.. authenticated peers {peers}")
            check(status["settlement_engine"] == "haskell+rust-mirror",
                  f"{node} settles with Haskell cross-checked by the Rust mirror")
            check(status["worker_backend"] == "python", f"{node} computes AI work with the Python worker")
        self.step("connected", nodes=nodes)

    # --- actions -----------------------------------------------------------
    def submit(self, *extra: str) -> dict:
        task = self.client("node-1", "submit-task", "--key-dir", "/keys", *extra)
        check(task["verdict"] == "accepted", f"task {task['task_id'][:12]}.. accepted for height "
              f"{task['sequence']} with {len(task['partitions'])} worker partitions")
        self.step("task", task=task)
        return task

    def wait_height(self, height: int, nodes: list[str], timeout: float = 180) -> None:
        self.wait(f"height {height} finalized on {nodes}", timeout,
                  lambda: all(self.status(n)["height"] >= height for n in nodes))

    def verify(self, nodes: list[str]) -> dict:
        statuses = {n: self.status(n) for n in nodes}
        reference = statuses[nodes[0]]
        for block in reference["blocks"][self.model.height:]:
            self.model.apply(block)
            check(len({p["worker"] for p in block["partitions"]}) == len(block["partitions"]),
                  f"block {block['height']} pays {len(block['partitions'])} distinct workers")
            check(block["signers"] * 3 > self.validators * 2,
                  f"block {block['height']} finalized by {block['signers']}/{self.validators} validator signatures")
        for node, status in statuses.items():
            for field in ("height", "tip", "state_root", "supply", "fee_pool"):
                check(status[field] == reference[field], f"{node} {field} matches {nodes[0]}")
            check([b["block_id"] for b in status["blocks"]] == [b["block_id"] for b in reference["blocks"]],
                  f"{node} holds the identical finalized history")
            check(status["height"] == self.model.height, f"{node} height {status['height']} is expected")
            check(status["supply"] == self.model.supply, f"{node} supply {status['supply']} = genesis + issuance")
            check(status["fee_pool"] == self.model.fee_pool, f"{node} fee pool {status['fee_pool']} is expected")
            for address, expected in self.model.balances.items():
                actual = status["accounts"].get(address, {}).get("balance", 0)
                check(actual == expected, f"{node} balance of {address[:12]}.. is {expected}")
        return reference

    def probe_unauthorized(self) -> None:
        before = self.status("node-1")["counters"]["rejected_handshakes"]
        probe = self.client("node-2", "probe-unauthorized", "--peer", "node-1:30333")
        check(probe["admitted"] is False, "unregistered peer key refused by node-1")
        self.wait("node-1 counts the rejected handshake", 30,
                  lambda: self.status("node-1")["counters"]["rejected_handshakes"] > before)
        self.step("unauthorized_peer", probe=probe)

    def finality(self, nodes: list[str]) -> list[int]:
        samples: list[int] = []
        for node in nodes:
            samples.extend(self.status(node)["finality_ms"])
        return samples

    def leader_of(self, height: int, round_: int = 0) -> str:
        validators = sorted(self.network["validators"], key=lambda v: bytes.fromhex(v["public_key"]))
        return validators[(height + round_) % len(validators)]["name"]

    def save_report(self) -> None:
        (self.out / "report.json").write_text(json.dumps(self.report, indent=2))
        log(f"report written to {self.out / 'report.json'}")


def demo(net: Testnet, qmlp_file: Path | None = None) -> None:
    nodes = net.nodes()
    net.up()
    net.resources("idle after authentication")
    net.probe_unauthorized()
    # 1. The repository's original deterministic workload, one row per worker.
    net.submit("--workload", "fixture", "--fee-budget", "30")
    net.wait_height(1, nodes)
    net.verify(nodes)
    # 2. A larger seeded workload split across all workers.
    net.submit("--workload", "synthetic", "--rows", "24", "--features", "8", "--fee-budget", "40")
    net.wait_height(2, nodes)
    net.verify(nodes)
    # 3. Adversarial suite against node-1 (forged, duplicated, expired,
    #    overlapping, manipulated, tampered and replayed inputs).
    suite = net.client("node-1", "adversarial", "--key-dir", "/keys")
    for case in suite["cases"]:
        check(case["pass"], f"adversarial {case['case']}: {case['verdict']} ({case['reason']})")
    check(suite["height_unchanged"], "adversarial inputs did not change finalized state")
    net.step("adversarial", suite=suite)
    net.resources("after adversarial suite")
    net.submit("--workload", "synthetic", "--rows", "9", "--features", "4",
               "--sequence", str(suite["next_free_sequence"]))
    net.wait_height(4, nodes)
    net.verify(nodes)
    # 4. Restart one node: it must replay its finalized history from disk.
    net.compose("restart", "node-3")
    net.wait_connected(nodes)
    restarted = net.status("node-3")
    check(restarted["startup"]["replayed_blocks"] == 4, "node-3 replayed 4 finalized blocks from RocksDB")
    check(restarted["startup"]["initialized_genesis"] is False, "node-3 reused its persisted chain")
    net.verify(nodes)
    # 5. QMLP-V1 work: the ordinary seeded fixture, or a REAL model trained
    #    off-chain by PyTorch, quantized, exported, and verified by Rust.
    #    Docker nodes use Python workers, Haskell settlement and BFT finality.
    before_qmlp = net.status("node-1")
    # Prepare and validate the artifact before timing the client submission.
    # In particular, Docker copying and local JSON checks are setup, not
    # submission-to-finality latency. The CLI's own model load remains timed.
    if qmlp_file is not None:
        if not qmlp_file.is_file():
            raise Failure(f"research QMLP export not found: {qmlp_file}")
        try:
            export = json.loads(qmlp_file.read_text())
        except (OSError, ValueError) as error:
            raise Failure(f"invalid research export: {error}") from error
        check(export.get("workload") == "qmlp-v1", "research artifact is QMLP-V1")
        check(isinstance(export.get("rows"), list) and bool(export["rows"]),
              "research artifact has nonempty inference input rows")
        check(isinstance(export.get("model"), dict), "research artifact has model parameters")
        net.compose("cp", str(qmlp_file.resolve()), "node-1:/tmp/digits.qmlp.json")

    # Client-visible latency includes signed CLI submission, distributed AI
    # work, BFT finalization, and polling of every validator.
    qmlp_started = time.monotonic()
    if qmlp_file is None:
        task = net.submit("--workload", "qmlp", "--rows", "12")
    else:
        task = net.submit("--qmlp-file", "/tmp/digits.qmlp.json")
        check(task["rows"] == len(export["rows"]), "submitted the exported input rows")
        check(task["features"] == export["model"]["d_in"], "submitted the exported model dimensions")
        net.step("pytorch_export", model_commitment=export["model_commitment"],
                 result_commitment=export["result_commitment"], task_id=task["task_id"])
    net.wait_height(5, nodes)
    qmlp_observed_ms = (time.monotonic() - qmlp_started) * 1000
    final = net.verify(nodes)
    check(final["height"] >= 5, "QMLP task finalized on all three validators")
    net.step("qmlp_end_to_end_finality", height=5, workload="qmlp-v1",
             measured_from="before_cli_submission",
             measured_until="all_validators_observed_height",
             observation_method="sequential_one_second_status_polling",
             elapsed_ms=round(qmlp_observed_ms, 3), rows=task["rows"],
             partitions=len(task["partitions"]), validators=len(nodes))
    net.report["qmlp_end_to_end_finality"] = {
        "height": 5, "elapsed_ms": round(qmlp_observed_ms, 3),
        "rows": task["rows"], "partitions": len(task["partitions"]),
        "validators": len(nodes), "polling_interval_s": 1,
        "includes": ["cli_submission", "python_worker_execution", "bft_finalization",
                     "status_polling_to_all_validators"],
    }
    check(final["blocks"][4]["workload"] == "qmlp-v1", "height 5 settled QMLP-V1 inference work")
    if qmlp_file is not None:
        workers = [p["worker"] for p in task["partitions"]]
        gains = [final["accounts"][w]["balance"] -
                 before_qmlp["accounts"].get(w, {}).get("balance", 0) for w in workers]
        expected_paid = task["fee_budget"] + final["blocks"][4]["issuance"]
        check(sum(gains) == expected_paid and all(gain > 0 for gain in gains),
              f"trained model paid {expected_paid} Sayed Tokens to {len(workers)} workers")
        net.step("pytorch_rewards", height=5, worker_gains=gains, tokens_paid=expected_paid)
    net.resources("end of demo")
    net.report["final"] = {k: final[k] for k in ("height", "tip", "state_root", "supply", "fee_pool")}
    net.report["finality_ms"] = net.finality(nodes)
    net.report["finality_summary_ms"] = summarize(net.report["finality_ms"])
    net.report["counters"] = {n: net.status(n)["counters"] for n in nodes}
    net.report["issued"] = net.model.issued
    net.report["paid"] = net.model.paid


def fault(net: Testnet) -> None:
    nodes = net.nodes()
    net.up()
    net.submit("--workload", "synthetic", "--rows", "16", "--features", "6")
    net.wait_height(1, nodes)
    net.verify(nodes)
    # Crash one validator: 3 of 4 voting power still exceeds two thirds.
    net.compose("stop", "node-4")
    live = nodes[:3]
    net.wait_connected(live)
    # A QMLP-V1 block while node-4 is down: it must later sync and verify it.
    net.submit("--workload", "qmlp", "--rows", "12")
    net.wait_height(2, live)
    net.submit("--workload", "fixture")
    net.wait_height(3, live)
    net.verify(live)
    # Recover: node-4 replays height 1 locally and syncs heights 2-3.
    net.compose("start", "node-4")
    net.wait_height(3, nodes)
    recovered = net.status("node-4")
    check(recovered["startup"]["replayed_blocks"] == 1, "node-4 replayed its 1 persisted block")
    check(recovered["counters"]["synced_blocks"] >= 2, "node-4 synced 2 certified blocks from peers")
    check(recovered["blocks"][1]["workload"] == "qmlp-v1", "node-4 re-verified the synced QMLP-V1 block")
    net.resources("after node-4 crash recovery")
    net.wait_connected(nodes)
    net.verify(nodes)
    # Crash the round-0 leader of the next height: a round change must occur.
    # node-1 holds the task-authority key, so skip a height it would lead.
    height = 4
    while net.leader_of(height) == "node-1":
        net.submit("--workload", "fixture")
        net.wait_height(height, nodes)
        height += 1
    leader = net.leader_of(height)
    log(f"crashing {leader}, the round-0 leader for height {height}")
    net.compose("kill", leader)
    live = [n for n in nodes if n != leader]
    net.wait_connected(live)
    net.submit("--workload", "synthetic", "--rows", "9", "--features", "5")
    net.wait_height(height, live, timeout=240)
    block = net.status(live[0])["blocks"][height - 1]
    check(block["round"] >= 1, f"height {height} finalized in round {block['round']} after the leader crash")
    net.step("leader_crash", leader=leader, height=height, round=block["round"])
    net.verify(live)
    net.compose("start", leader)
    net.wait_height(height, nodes)
    net.wait_connected(nodes)
    final = net.verify(nodes)
    net.resources("end of fault scenario")
    net.report["final"] = {k: final[k] for k in ("height", "tip", "state_root", "supply", "fee_pool")}
    net.report["finality_ms"] = net.finality(nodes)
    net.report["finality_summary_ms"] = summarize(net.report["finality_ms"])
    net.report["counters"] = {n: net.status(n)["counters"] for n in nodes}
    net.report["issued"] = net.model.issued
    net.report["paid"] = net.model.paid


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=["demo", "fault", "all", "build", "down"])
    parser.add_argument("--profile", choices=sorted(PROFILES))
    parser.add_argument("--skip-build", action="store_true", help="reuse an existing image")
    parser.add_argument("--qmlp-file", type=Path, help="PyTorch QMLP export for the three-node demo")
    parser.add_argument("--keep", action="store_true", help="leave containers running")
    parser.add_argument("--out", default=str(ROOT / "testnet" / "generated"))
    args = parser.parse_args()
    if args.qmlp_file is not None and args.command not in ("demo", "all"):
        parser.error("--qmlp-file is supported only by demo or all")
    out = Path(args.out).resolve()
    if args.command == "down":
        for profile in [args.profile] if args.profile else sorted(PROFILES):
            subprocess.run(["docker", "compose", "-f", str(PROFILES[profile]["compose"]),
                            "-p", f"aivoblock-{profile}", "down", "-v", "--remove-orphans"],
                           env={**os.environ, "AIVOBLOCK_TESTNET_DIR": str(out / profile),
                                "AIVOBLOCK_AUTHORITY_PUBLIC_KEY": "0" * 64})
        return 0
    if not args.skip_build:
        log(f"building {IMAGE} (Haskell core, Rust node, Python worker)")
        run(["docker", "build", "-f", str(ROOT / "docker" / "Dockerfile.node"), "-t", IMAGE, str(ROOT)])
    if args.command == "build":
        return 0
    research_demo = lambda net: demo(net, args.qmlp_file)
    plan = {"demo": [("three-node", research_demo)], "fault": [("four-validator", fault)],
            "all": [("three-node", research_demo), ("four-validator", fault)]}[args.command]
    if args.profile:
        plan = [(args.profile, fn) for _, fn in plan]
    failed = False
    for profile, scenario in plan:
        net = Testnet(profile, out)
        started = time.time()
        try:
            net.init()
            scenario(net)
            net.report["result"] = "PASS"
            log(f"{profile}: ALL CHECKS PASSED in {time.time() - started:.0f}s")
        except Failure as error:
            failed = True
            net.report["result"] = f"FAIL: {error}"
            log(f"{profile}: FAILED: {error}")
        finally:
            net.report["duration_s"] = round(time.time() - started, 1)
            if not args.keep:
                net.down()
            net.save_report()
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
