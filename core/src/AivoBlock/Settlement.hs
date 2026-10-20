-- | Pure settlement of a previously authorized AI-work reward budget.
--
-- This module does NOT verify work, signatures, receipt identities, account
-- ownership, fee-budget policy or consensus. The caller must supply a
-- consensus-approved budget and canonical, independently verified receipt IDs.
-- In particular, two identifiers for the same real-world work cannot be
-- detected here. No tokens are minted: rewards spend existing ledger fees.
module AivoBlock.Settlement
  ( RewardEntitlement(..)
  , settleFundedRewards
  ) where

import AivoBlock.Block (Hash256, zeroHash)
import AivoBlock.Ledger (Account(..), Ledger(..))
import AivoBlock.PuzzleProof (WorkerId)
import AivoBlock.Rewards
  ( Contribution(..), Reward(..), distributeRewards )
import AivoBlock.Transaction (Address)
import Control.Monad (foldM, when)
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Word (Word64)

-- | A uniquely identified, authenticated contribution for one worker.
-- A caller must verify the association between work, worker and recipient.
data RewardEntitlement = RewardEntitlement
  { entitlementReceiptId :: !Hash256
  , entitlementWorkerId :: !WorkerId
  , entitlementRecipient :: !Address
  , entitlementUnits :: !Word64
  } deriving (Eq, Show)

-- | Allocate a preauthorized positive fee-pool budget by verified work units
-- using 'distributeRewards', then atomically commit payout and replay state.
-- No account nonce changes, no subsidy, no creation of new supply.
--
-- This operation is pure: a failed batch returns Left and changes no state.
-- For persistent replay protection, the caller must durably commit the entire
-- returned Ledger in the same transaction as the block state transition.
settleFundedRewards
  :: Hash256 -> Word64 -> [RewardEntitlement] -> Ledger -> Either String Ledger
settleFundedRewards batchId budget entitlements ledger = do
  when (batchId == zeroHash) (Left "Zero reward batch ID")
  when (Set.member batchId (ledgerSettledRewardBatches ledger))
    (Left "Reward batch already settled")
  when (budget == 0) (Left "Reward budget must be positive")
  when (null entitlements) (Left "No reward entitlements")
  when (budget > ledgerFeePool ledger) (Left "Insufficient fee pool")

  mapM_ validateEntitlement entitlements
  let receiptIds = map entitlementReceiptId entitlements
      receiptSet = Set.fromList receiptIds
  when (Set.size receiptSet /= length receiptIds)
    (Left "Duplicate work receipt in reward batch")
  when (not (Set.null (Set.intersection receiptSet (ledgerPaidWorkReceipts ledger))))
    (Left "Work receipt already paid")

  -- Map ordering is stable regardless of input order. Integer intermediates
  -- avoid overflow; reject any worker total outside Word64 range.
  recipientsAndWeights <- foldM addEntitlement Map.empty entitlements
  let contributions =
        [ Contribution worker (fromInteger weight)
        | (worker, (_, weight)) <- Map.toAscList recipientsAndWeights ]
  rewards <- distributeRewards budget contributions
  accounts <- foldM (creditReward recipientsAndWeights)
    (ledgerAccounts ledger) rewards

  pure ledger
    { ledgerAccounts = accounts
    , ledgerFeePool = ledgerFeePool ledger - budget
    , ledgerSettledRewardBatches =
        Set.insert batchId (ledgerSettledRewardBatches ledger)
    , ledgerPaidWorkReceipts =
        Set.union receiptSet (ledgerPaidWorkReceipts ledger)
    }

validateEntitlement :: RewardEntitlement -> Either String ()
validateEntitlement entry = do
  when (entitlementReceiptId entry == zeroHash)
    (Left "Zero work receipt ID")
  when (entitlementWorkerId entry == zeroHash)
    (Left "Zero worker ID")
  when (entitlementUnits entry == 0)
    (Left "Work units must be positive")

addEntitlement
  :: Map WorkerId (Address, Integer)
  -> RewardEntitlement
  -> Either String (Map WorkerId (Address, Integer))
addEntitlement entries entry =
  let worker = entitlementWorkerId entry
      recipient = entitlementRecipient entry
      weight = toInteger (entitlementUnits entry)
  in case Map.lookup worker entries of
    Nothing -> Right (Map.insert worker (recipient, weight) entries)
    Just (previous, oldWeight) -> do
      when (previous /= recipient)
        (Left "Conflicting payout addresses for worker")
      let combined = oldWeight + weight
      when (combined > toInteger (maxBound :: Word64))
        (Left "Worker weight overflow")
      pure (Map.insert worker (recipient, combined) entries)

creditReward
  :: Map WorkerId (Address, Integer)
  -> Map Address Account
  -> Reward
  -> Either String (Map Address Account)
creditReward mappings accounts reward = do
  let amount = rewardAmount reward
      worker = rewardWorker reward
  (recipient, _) <- maybe
    (Left "Missing reward recipient")
    Right (Map.lookup worker mappings)
  if amount == 0
    then Right accounts
    else do
      let account = Map.findWithDefault (Account 0 0) recipient accounts
      when (amount > maxBound - accountBalance account)
        (Left "Unsigned 64-bit overflow")
      let credited = account
            { accountBalance = accountBalance account + amount }
      pure (Map.insert recipient credited accounts)
