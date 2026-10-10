-- | Consensus-facing commitments for collaborative AI puzzle work.
--
-- This is a structural proof envelope, NOT a proof that useful AI
-- computation occurred. A future, versioned verifier must independently
-- check task-specific correctness, reproducibility, worker identity,
-- and binding of results to the nominated puzzle.
module AivoBlock.PuzzleProof
  ( PuzzleId
  , WorkerId
  , Puzzle(..)
  , PuzzleProof(..)
  , ProofStatus(..)
  , validatePuzzleShape
  , validateProofShape
  ) where

import AivoBlock.Block (Hash256)
import Data.Word (Word32, Word64)

type PuzzleId = Hash256
type WorkerId = Hash256

-- | Task identifiers commit to a protocol-defined puzzle specification.
-- The model, input and verification rules must be immutable for a task.
data Puzzle = Puzzle
  { puzzleId              :: !PuzzleId
  , puzzleVersion         :: !Word32
  , puzzleModelCommitment :: !Hash256
  , puzzleInputCommitment :: !Hash256
  , puzzleRulesCommitment :: !Hash256
  , puzzleUnits           :: !Word64
  , puzzleDeadlineHeight  :: !Word64
  } deriving (Eq, Show)

-- | A worker's claimed result for a contiguous range of task units.
-- The optional witness hash is a commitment, not an on-chain payload.
data PuzzleProof = PuzzleProof
  { proofPuzzleId         :: !PuzzleId
  , proofWorkerId         :: !WorkerId
  , proofStartUnit        :: !Word64
  , proofUnitCount        :: !Word64
  , proofResultCommitment :: !Hash256
  , proofWitnessCommitment :: !(Maybe Hash256)
  } deriving (Eq, Show)

data ProofStatus
  = PendingVerification
  | Verified
  | Rejected
  deriving (Eq, Ord, Show)

-- | Static checks only; acceptance requires verification of the puzzle
-- definition, its origin, its deadline and all consensus rules.
validatePuzzleShape :: Puzzle -> Either String ()
validatePuzzleShape puzzle
  | puzzleVersion puzzle == 0 = Left "Puzzle version must be nonzero"
  | puzzleUnits puzzle == 0 = Left "Puzzle must contain at least one unit"
  | otherwise = Right ()

-- | Checks whether the claimed work range lies within the puzzle.
-- This function does not verify the result, witness or worker signature.
validateProofShape :: Puzzle -> PuzzleProof -> Either String ()
validateProofShape puzzle proof
  | proofPuzzleId proof /= puzzleId puzzle =
      Left "Proof refers to a different puzzle"
  | proofUnitCount proof == 0 =
      Left "Proof must cover at least one unit"
  | proofStartUnit proof >= puzzleUnits puzzle =
      Left "Proof begins beyond the puzzle range"
  | proofUnitCount proof > puzzleUnits puzzle - proofStartUnit proof =
      Left "Proof extends beyond the puzzle range"
  | otherwise = Right ()
