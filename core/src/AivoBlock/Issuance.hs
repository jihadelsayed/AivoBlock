-- | Configurable native Sayed Token issuance for verified AI-work blocks.
--
-- The schedule is a pure function of the genesis-bound policy, the block
-- height and the prior ledger supply. Issued units are minted into the fee
-- pool and then paid out by 'settleFundedRewards' in the same pure step, so a
-- rejected block can never mint. Supply never exceeds the configured cap.
--
-- This module does NOT verify AI work, worker signatures, receipt identities
-- or consensus finality. Callers must only settle independently recomputed,
-- consensus-finalized work.
module AivoBlock.Issuance
  ( IssuancePolicy
  , policyInitialReward
  , policyHalvingInterval
  , policyMaxSupply
  , mkIssuancePolicy
  , ledgerSupply
  , scheduledIssuance
  , settleIssuedRewards
  ) where

import AivoBlock.Block (Hash256)
import AivoBlock.Ledger (Account(..), Ledger(..))
import AivoBlock.Settlement (RewardEntitlement, settleFundedRewards)
import Control.Monad (when)
import Data.Bits (shiftR)
import qualified Data.Map.Strict as Map
import Data.Word (Word64)

-- | Genesis-bound issuance parameters, in Sayed Token base units.
--
-- Height h >= 1 issues @initialReward >> ((h - 1) div halvingInterval)@,
-- truncated so that total supply never exceeds 'policyMaxSupply'.
data IssuancePolicy = IssuancePolicy
  { policyInitialReward :: !Word64
  , policyHalvingInterval :: !Word64
  , policyMaxSupply :: !Word64
  } deriving (Eq, Show)

mkIssuancePolicy :: Word64 -> Word64 -> Word64 -> Either String IssuancePolicy
mkIssuancePolicy initial interval cap
  | interval == 0 = Left "Halving interval must be positive"
  | cap == 0 = Left "Maximum supply must be positive"
  | initial > cap = Left "Initial reward exceeds maximum supply"
  | otherwise = Right (IssuancePolicy initial interval cap)

-- | Total native supply: every account balance plus the undistributed fee
-- pool. Integer arithmetic cannot overflow.
ledgerSupply :: Ledger -> Integer
ledgerSupply ledger =
  sum (map (toInteger . accountBalance) (Map.elems (ledgerAccounts ledger)))
    + toInteger (ledgerFeePool ledger)

-- | Units minted by the block at @height@, given the supply before it.
-- Genesis (height 0) never issues; a ledger already above the cap is invalid.
scheduledIssuance :: IssuancePolicy -> Word64 -> Integer -> Either String Word64
scheduledIssuance policy height supply
  | height == 0 = Left "Genesis does not issue rewards"
  | supply < 0 = Left "Negative supply"
  | supply > cap = Left "Ledger supply exceeds issuance cap"
  | otherwise = Right (fromInteger (min (toInteger base) (cap - supply)))
  where
    cap = toInteger (policyMaxSupply policy)
    halvings = (height - 1) `div` policyHalvingInterval policy
    base
      | halvings >= 64 = 0
      | otherwise = policyInitialReward policy `shiftR` fromIntegral halvings

-- | Settle one finalized AI-work block: check the claimed issuance against
-- the schedule, require the fee budget to be covered by EXISTING fees,
-- mint exactly the scheduled issuance into the fee pool and distribute
-- @feeBudget + issuance@ by verified work units. Supply grows by exactly the
-- issuance and stays within the cap; anything else returns Left.
settleIssuedRewards
  :: IssuancePolicy
  -> Word64        -- ^ block height
  -> Hash256       -- ^ reward batch ID
  -> Word64        -- ^ fee-funded budget authorized by the task
  -> Word64        -- ^ issuance claimed by the block
  -> [RewardEntitlement]
  -> Ledger
  -> Either String Ledger
settleIssuedRewards policy height batchId feeBudget claimed entitlements ledger = do
  let before = ledgerSupply ledger
  expected <- scheduledIssuance policy height before
  when (claimed /= expected) (Left "Issuance differs from policy schedule")
  when (feeBudget > ledgerFeePool ledger) (Left "Insufficient fee pool")
  let total = toInteger feeBudget + toInteger claimed
      pool = toInteger (ledgerFeePool ledger) + toInteger claimed
      maxWord = toInteger (maxBound :: Word64)
  when (total == 0) (Left "Reward budget must be positive")
  when (total > maxWord || pool > maxWord) (Left "Reward budget overflow")
  let minted = ledger { ledgerFeePool = fromInteger pool }
  settled <- settleFundedRewards batchId (fromInteger total) entitlements minted
  let after = ledgerSupply settled
  when (after /= before + toInteger claimed)
    (Left "Supply invariant violated")
  when (after > toInteger (policyMaxSupply policy))
    (Left "Issuance cap exceeded")
  pure settled
