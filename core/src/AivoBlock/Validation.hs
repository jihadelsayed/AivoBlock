-- | Structural validation primitives for AivoBlock blocks.
--
-- These checks are necessary but not sufficient for consensus:
-- canonical hashing, Merkle commitments, transaction signatures,
-- state transitions, AI proofs and validator finality are separate.
module AivoBlock.Validation
  ( ValidationError(..)
  , validateBlockShape
  , validateChildHeader
  ) where

import AivoBlock.Block
  ( Block(..), BlockBody(..), BlockHeader(..), Hash256, zeroHash )
import Data.List (nub)
import Data.Word (Word32, Word64)

data ValidationError
  = InvalidBlockVersion
  | InvalidChainId
  | DuplicateTransactionId
  | DuplicatePuzzleProofId
  | InvalidGenesisParent
  | InvalidParentHash
  | InvalidHeight
  | NonIncreasingTimestamp
  | HeightOverflow
  deriving (Eq, Show)

-- | Minimal local structure checks. These do not authenticate the
-- included data nor validate header roots against actual contents.
validateBlockShape :: Block -> Either ValidationError ()
validateBlockShape block = do
  let header = blockHeader block
      body = blockBody block
  require (headerVersion header /= 0) InvalidBlockVersion
  require (headerChainId header /= 0) InvalidChainId
  require (unique (bodyTransactionIds body)) DuplicateTransactionId
  require (unique (bodyPuzzleProofIds body)) DuplicatePuzzleProofId
  if headerHeight header == 0
    then require (headerParentHash header == zeroHash) InvalidGenesisParent
    else require (headerParentHash header /= zeroHash) InvalidParentHash

-- | Check direct header ancestry under a fixed chain and version.
-- The provided parent hash must be independently computed from its
-- canonical encoding; it must NOT be trusted from a peer's assertion.
validateChildHeader
  :: Word32        -- ^ Expected chain ID
  -> Word32        -- ^ Expected protocol version
  -> Hash256       -- ^ Independently computed parent block hash
  -> BlockHeader   -- ^ Previously validated parent header
  -> BlockHeader   -- ^ Candidate child header
  -> Either ValidationError ()
validateChildHeader chainId version parentHash parent child = do
  require (chainId /= 0 && headerChainId parent == chainId
        && headerChainId child == chainId) InvalidChainId
  require (version /= 0 && headerVersion parent == version
        && headerVersion child == version) InvalidBlockVersion
  require (headerHeight parent /= maxBound) HeightOverflow
  require (headerHeight child == headerHeight parent + 1) InvalidHeight
  require (headerParentHash child == parentHash) InvalidParentHash
  require (headerTimestampMs child > headerTimestampMs parent)
    NonIncreasingTimestamp

require :: Bool -> ValidationError -> Either ValidationError ()
require condition err = if condition then Right () else Left err

unique :: Eq a => [a] -> Bool
unique xs = length xs == length (nub xs)
