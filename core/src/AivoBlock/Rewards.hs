-- | Deterministic reward allocation for verified AI puzzle contributions.
--
-- This module allocates an existing reward budget in native Sayed Token
-- base units. It does NOT mint tokens, choose a block subsidy, or decide
-- whether a worker's AI proof is valid. Those are separate consensus rules.
module AivoBlock.Rewards
  ( Contribution(..)
  , Reward(..)
  , distributeRewards
  ) where

import AivoBlock.PuzzleProof (WorkerId)
import Data.List (sortBy)
import Data.Ord (comparing)
import Data.Word (Word64)

-- | Weight assigned by a trusted, consensus-defined proof verifier.
-- Do not derive weight solely from a miner's unverified claim.
data Contribution = Contribution
  { contributionWorker :: !WorkerId
  , contributionWeight :: !Word64
  } deriving (Eq, Show)

data Reward = Reward
  { rewardWorker :: !WorkerId
  , rewardAmount :: !Word64
  } deriving (Eq, Show)

-- | Split a fixed budget in proportion to validated contributions.
--
-- Uses integer arithmetic and the largest-remainder method, breaking
-- ties by ascending worker ID. Output order is ascending worker ID.
-- The entire budget is allocated with no rounding loss or overflow.
--
-- Calling this function does not authorize issuance or payment:
-- the ledger must independently enforce funding and supply rules.
distributeRewards :: Word64 -> [Contribution] -> Either String [Reward]
distributeRewards budget contributions
  | null contributions =
      if budget == 0 then Right [] else Left "No reward recipients"
  | any ((== 0) . contributionWeight) contributions =
      Left "Contribution weights must be positive"
  | hasDuplicates workers = Left "Duplicate worker contribution"
  | otherwise = Right (map makeReward sorted)
  where
    sorted = sortBy (comparing contributionWorker) contributions
    workers = map contributionWorker sorted
    totalWeight = sum (map (toInteger . contributionWeight) sorted)
    budgetInteger = toInteger budget

    -- Intermediate calculations use unbounded Integer arithmetic.
    quotas =
      [ (contributionWorker entry, numerator `div` totalWeight,
         numerator `mod` totalWeight)
      | entry <- sorted
      , let numerator = budgetInteger * toInteger (contributionWeight entry)
      ]
    allocated = sum [whole | (_, whole, _) <- quotas]
    remaining = fromInteger (budgetInteger - allocated) :: Int

    -- Higher fractional remainders win; ties go to smaller IDs.
    ranked = sortBy compareRemainder quotas
    compareRemainder (workerA, _, remA) (workerB, _, remB) =
      compare remB remA <> compare workerA workerB
    bonusWorkers = map (\(worker, _, _) -> worker) (take remaining ranked)

    makeReward entry =
      let worker = contributionWorker entry
          (_, whole, _) = head [q | q@(w, _, _) <- quotas, w == worker]
          bonus = if worker `elem` bonusWorkers then 1 else 0
      in Reward worker (fromInteger (whole + bonus))

hasDuplicates :: Eq a => [a] -> Bool
hasDuplicates [] = False
hasDuplicates (x:xs) = x `elem` xs || hasDuplicates xs
