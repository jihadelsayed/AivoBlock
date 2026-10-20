-- | Deterministic, non-minting screening of externally verified AI work.
--
-- IMPORTANT: ProofStatus Verified is supplied by a trusted caller; this
-- module does NOT authenticate signatures, recompute AI work, validate
-- data availability, establish consensus, or authorize token issuance.
module AivoBlock.RewardEligibility
  ( VerifiedWork(..)
  , eligibleContributions
  ) where

import AivoBlock.Block (zeroHash)
import AivoBlock.PuzzleProof
  ( Puzzle(..), PuzzleProof(..), ProofStatus(..), WorkerId
  , validatePuzzleShape, validateProofShape )
import AivoBlock.Rewards (Contribution(..))
import Data.List (sortBy)
import Data.Ord (comparing)
import Data.Word (Word64)

-- | Input evidence must already have been independently authenticated and
-- verified; the status alone is never proof of correctness.
data VerifiedWork = VerifiedWork
  { workPuzzle :: !Puzzle
  , workProof :: !PuzzleProof
  , workStatus :: !ProofStatus
  } deriving (Eq, Show)

-- | Screen claims at the current chain height, rejecting invalid,
-- overlapping or duplicate unit ranges on the same puzzle.
-- Each accepted unit contributes one unit of weight. Allocation of a
-- funded reward budget is a separate operation in 'distributeRewards'.
eligibleContributions :: Word64 -> [VerifiedWork] -> Either String [Contribution]
eligibleContributions height works = do
  mapM_ (validateOne height) works
  let ordered = sortBy (comparing rangeKey) works
  validateOverlaps ordered
  let workers = sortBy compare [ (proofWorkerId (workProof w), proofUnitCount (workProof w))
                               | w <- works ]
  aggregate workers
  where
    rangeKey w = (puzzleId (workPuzzle w), proofStartUnit (workProof w))

validateOne :: Word64 -> VerifiedWork -> Either String ()
validateOne height work = do
  let puzzle = workPuzzle work
      proof = workProof work
  validatePuzzleShape puzzle
  validateProofShape puzzle proof
  if workStatus work /= Verified
    then Left "Proof not independently verified"
    else Right ()
  if height > puzzleDeadlineHeight puzzle
    then Left "Puzzle deadline exceeded"
    else Right ()
  if puzzleId puzzle == zeroHash || proofWorkerId proof == zeroHash
       || proofResultCommitment proof == zeroHash
    then Left "Zero puzzle, worker, or result commitment"
    else Right ()

validateOverlaps :: [VerifiedWork] -> Either String ()
validateOverlaps [] = Right ()
validateOverlaps [_] = Right ()
validateOverlaps (a:b:rest)
  | puzzleId (workPuzzle a) == puzzleId (workPuzzle b)
      && proofStartUnit (workProof b) <
         proofStartUnit (workProof a) + proofUnitCount (workProof a) =
      Left "Duplicate or overlapping verified work"
  | puzzleId (workPuzzle a) == puzzleId (workPuzzle b)
      && workPuzzle a /= workPuzzle b =
      Left "Conflicting definitions for the same puzzle"
  | otherwise = validateOverlaps (b:rest)

aggregate :: [(WorkerId, Word64)] -> Either String [Contribution]
aggregate [] = Right []
aggregate ((worker, units):rest) = go worker (toInteger units) rest
  where
    go current weight [] = Right [Contribution current (fromInteger weight)]
    go current weight ((next, unitsNext):xs)
      | current == next =
          let combined = weight + toInteger unitsNext
          in if combined > toInteger (maxBound :: Word64)
               then Left "Worker weight overflow"
               else go current combined xs
      | otherwise = do
          tailEntries <- go next (toInteger unitsNext) xs
          Right (Contribution current (fromInteger weight) : tailEntries)
