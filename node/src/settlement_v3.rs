//! Settlement of consensus-finalized V3 AI-work blocks with scheduled native
//! Sayed Token issuance. Haskell (`ledger_ai_work_block_v3`) is the ledger
//! authority; the independent Rust mirror in `issuance` must produce the
//! identical successor ledger or the block is rejected (fail closed).

use crate::ipc::haskell::HaskellValidatorClient;
use crate::ipc::ledger_responses::{decode_ai_work_block_v3_reply, ExpectedAiWorkSettlement};
use crate::issuance::{mirror_settle_issued_rewards, IssuancePolicyV1, IssuedSettlementV3};
use crate::reward_block::checked_receipt_ids;
use crate::storage::ledger_state::{encode_ledger_state, LedgerState};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Result of one accepted settlement: the successor ledger, canonical
/// receipt IDs in block order, and the evidence persisted with the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledBlockV3 {
    pub post: LedgerState,
    pub receipt_ids: Vec<[u8; 32]>,
    pub evidence: Vec<u8>,
}

/// Build the exact framed JSON request for the Haskell core.
pub fn haskell_request_v3(
    policy: &IssuancePolicyV1,
    prior: &LedgerState,
    input: &IssuedSettlementV3<'_>,
    receipt_ids: &[[u8; 32]],
) -> Result<String, String> {
    let previous = encode_ledger_state(prior).map_err(|e| format!("{e:?}"))?;
    let works = input
        .works
        .iter()
        .zip(receipt_ids)
        .map(|(work, id)| {
            let r = &work.receipt;
            format!(
                r#"{{"puzzle_id":"{}","worker_id":"{}","result_commitment":"{}","puzzle_units":{},"deadline_height":{},"start":{},"count":{},"verified":true,"receipt_id":"{}"}}"#,
                hex(&r.puzzle_id),
                hex(&r.worker_id),
                hex(&r.result_commitment),
                work.puzzle_units,
                work.deadline_height,
                r.range.start,
                r.range.count,
                hex(id)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        r#"{{"protocol_version":1,"type":"ledger_ai_work_block_v3","chain_id":1,"height":{},"state_hex":"{}","fee_budget":{},"issuance":{},"policy":{{"initial_reward":{},"halving_interval":{},"max_supply":{}}},"batch_id":"{}","works":[{}]}}"#,
        input.height,
        hex(&previous),
        input.fee_budget,
        input.issuance,
        policy.initial_reward,
        policy.halving_interval,
        policy.max_supply,
        hex(&input.batch_id),
        works
    ))
}

/// Execute the transition in Haskell and strictly decode its reply.
pub fn haskell_settle_v3(
    core: &HaskellValidatorClient,
    policy: &IssuancePolicyV1,
    prior: &LedgerState,
    input: &IssuedSettlementV3<'_>,
) -> Result<SettledBlockV3, String> {
    let receipt_ids = checked_receipt_ids(input.works, input.height)?;
    let request = haskell_request_v3(policy, prior, input, &receipt_ids)?;
    let response = core
        .exchange(request.as_bytes())
        .map_err(|e| format!("Haskell V3 settlement IPC: {e:?}"))?;
    let post = decode_ai_work_block_v3_reply(
        &response,
        &ExpectedAiWorkSettlement {
            batch_id: &input.batch_id,
            fee_budget: input.fee_budget,
            issuance: input.issuance,
            max_supply: policy.max_supply,
            receipts: &receipt_ids,
        },
        prior,
    )?;
    Ok(SettledBlockV3 {
        post,
        receipt_ids,
        evidence: response,
    })
}

fn mirror_evidence(input: &IssuedSettlementV3<'_>) -> Vec<u8> {
    serde_json::json!({
        "engine": "rust-mirror",
        "height": input.height,
        "issuance": input.issuance,
        "fee_budget": input.fee_budget,
    })
    .to_string()
    .into_bytes()
}

/// Which deterministic ledger engine a node runs.
#[derive(Debug, Clone)]
pub enum SettlementEngine {
    /// Production: Haskell executes; the Rust mirror must agree exactly.
    Haskell(HaskellValidatorClient),
    /// Tests only: the Rust mirror alone, when no Haskell binary exists.
    RustMirror,
}

impl SettlementEngine {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Haskell(_) => "haskell+rust-mirror",
            Self::RustMirror => "rust-mirror-only",
        }
    }

    pub fn settle(
        &self,
        policy: &IssuancePolicyV1,
        prior: &LedgerState,
        input: &IssuedSettlementV3<'_>,
    ) -> Result<SettledBlockV3, String> {
        let (mirror, mirror_ids) = mirror_settle_issued_rewards(policy, prior, input)?;
        match self {
            Self::RustMirror => Ok(SettledBlockV3 {
                post: mirror,
                receipt_ids: mirror_ids,
                evidence: mirror_evidence(input),
            }),
            Self::Haskell(core) => {
                let settled = haskell_settle_v3(core, policy, prior, input)?;
                if settled.post != mirror || settled.receipt_ids != mirror_ids {
                    return Err("Haskell and Rust mirror settlements disagree".into());
                }
                Ok(settled)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::issuance::tests::{test_policy, vector_ledger, vector_works};

    fn input(works: &[crate::reward_block::RewardWorkV1]) -> IssuedSettlementV3<'_> {
        IssuedSettlementV3 {
            height: 1,
            batch_id: [90; 32],
            fee_budget: 30,
            issuance: 50,
            works,
        }
    }

    #[test]
    fn mirror_engine_settles_vector_and_emits_evidence() {
        let works = vector_works();
        let settled = SettlementEngine::RustMirror
            .settle(&test_policy(), &vector_ledger(), &input(&works))
            .unwrap();
        assert_eq!(settled.post.fee_pool, 570);
        assert_eq!(settled.receipt_ids.len(), 3);
        assert!(!settled.evidence.is_empty());
    }

    #[test]
    fn request_contains_policy_and_exact_receipts() {
        let works = vector_works();
        let ids = checked_receipt_ids(&works, 1).unwrap();
        let request =
            haskell_request_v3(&test_policy(), &vector_ledger(), &input(&works), &ids).unwrap();
        let value: serde_json::Value = serde_json::from_str(&request).unwrap();
        assert_eq!(value["type"], "ledger_ai_work_block_v3");
        assert_eq!(value["policy"]["max_supply"], 2_000_000);
        assert_eq!(value["works"].as_array().unwrap().len(), 3);
        assert_eq!(value["works"][0]["receipt_id"], hex(&ids[0]));
    }

    /// Cross-language: run with AIVOBLOCK_HASKELL_EXECUTABLE set (CI
    /// cross_language job). Haskell must accept the shared vector, agree
    /// with the mirror byte-for-byte, and reject forged issuance/fees.
    #[test]
    fn haskell_v3_settlement_agrees_with_rust_mirror() {
        let Some(executable) =
            std::env::var_os("AIVOBLOCK_HASKELL_EXECUTABLE").filter(|v| !v.is_empty())
        else {
            eprintln!("SKIP: set AIVOBLOCK_HASKELL_EXECUTABLE for V3 settlement IPC");
            return;
        };
        let core = HaskellValidatorClient::new(executable).unwrap();
        let engine = SettlementEngine::Haskell(core.clone());
        let works = vector_works();
        let settled = engine
            .settle(&test_policy(), &vector_ledger(), &input(&works))
            .unwrap();
        assert_eq!(settled.post.accounts[&[1; 32]].balance, 27);
        assert_eq!(settled.post.accounts[&[3; 32]].balance, 26);
        for bad in [
            IssuedSettlementV3 {
                issuance: 51,
                ..input(&works)
            },
            IssuedSettlementV3 {
                fee_budget: 601,
                ..input(&works)
            },
        ] {
            assert!(haskell_settle_v3(&core, &test_policy(), &vector_ledger(), &bad).is_err());
        }
        let replay = IssuedSettlementV3 {
            height: 2,
            ..input(&works)
        };
        assert!(haskell_settle_v3(&core, &test_policy(), &settled.post, &replay).is_err());
    }
}
