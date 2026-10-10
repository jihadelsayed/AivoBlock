-- | Pure consensus primitives for the AivoBlock research prototype.
--
-- This module does not implement a complete consensus protocol. It
-- defines bounded validator voting and quorum checks, without claiming
-- that AI puzzle completion alone grants Sybil resistance or finality.
module AivoBlock.Consensus
  ( ValidatorId
  , mkValidatorId
  , validatorIdBytes
  , Round(..)
  , VoteChoice(..)
  , Vote(..)
  , ValidatorSet
  , mkValidatorSet
  , totalVotingPower
  , requiredQuorumPower
  , tallyVotes
  , hasQuorum
  ) where

import AivoBlock.Block (Hash256)
import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import Data.List (foldl')
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as Map
import Data.Word (Word64)

newtype ValidatorId = ValidatorId ByteString
  deriving (Eq, Ord, Show)

mkValidatorId :: ByteString -> Either String ValidatorId
mkValidatorId bytes
  | BS.length bytes == 32 = Right (ValidatorId bytes)
  | otherwise = Left "Validator ID must contain exactly 32 bytes"

validatorIdBytes :: ValidatorId -> ByteString
validatorIdBytes (ValidatorId bytes) = bytes

-- | The height and round to which a vote belongs.
data Round = Round
  { roundHeight :: !Word64
  , roundNumber :: !Word64
  } deriving (Eq, Ord, Show)

data VoteChoice
  = VoteFor !Hash256
  | VoteNil
  deriving (Eq, Show)

-- | Votes must be authenticated and checked against the active
-- validator set by the caller before a tally is authoritative.
data Vote = Vote
  { voteValidator :: !ValidatorId
  , voteRound     :: !Round
  , voteChoice    :: !VoteChoice
  } deriving (Eq, Show)

newtype ValidatorSet = ValidatorSet (Map ValidatorId Word64)
  deriving (Eq, Show)

mkValidatorSet :: [(ValidatorId, Word64)] -> Either String ValidatorSet
mkValidatorSet entries
  | null entries = Left "Validator set must not be empty"
  | any ((== 0) . snd) entries = Left "Voting power must be positive"
  | Map.size powers /= length entries = Left "Duplicate validator ID"
  | total <= 0 = Left "Invalid voting power"
  | total > toInteger (maxBound :: Word64) =
      Left "Total voting power exceeds Word64"
  | otherwise = Right (ValidatorSet powers)
  where
    powers = Map.fromList entries
    total = sum (map (toInteger . snd) entries)

totalVotingPower :: ValidatorSet -> Word64
totalVotingPower (ValidatorSet powers) =
  fromInteger (sum (map (toInteger . snd) (Map.toList powers)))

-- | Strictly greater than two-thirds of total power.
requiredQuorumPower :: ValidatorSet -> Word64
requiredQuorumPower validators =
  fromInteger ((2 * toInteger (totalVotingPower validators)) `div` 3 + 1)

-- | Tally unique, eligible votes for one block in one round.
-- Rejects duplicate validator votes, including conflicting votes.
-- Signature verification and fork-choice rules are NOT implemented.
tallyVotes
  :: ValidatorSet -> Round -> Hash256 -> [Vote] -> Either String Word64
tallyVotes (ValidatorSet powers) targetRound targetBlock votes =
  go Map.empty 0 votes
  where
    go :: Map ValidatorId () -> Integer -> [Vote] -> Either String Word64
    go _ total [] = Right (fromInteger total)
    go seen total (vote : rest)
      | voteRound vote /= targetRound = Left "Vote belongs to another round"
      | Map.member (voteValidator vote) seen = Left "Duplicate validator vote"
      | otherwise =
          case Map.lookup (voteValidator vote) powers of
            Nothing -> Left "Unknown validator"
            Just power ->
              let matching = voteChoice vote == VoteFor targetBlock
                  next = if matching then total + toInteger power else total
              in go (Map.insert (voteValidator vote) () seen) next rest

hasQuorum :: ValidatorSet -> Round -> Hash256 -> [Vote] -> Either String Bool
hasQuorum validators roundId blockId votes = do
  weight <- tallyVotes validators roundId blockId votes
  pure (weight >= requiredQuorumPower validators)
