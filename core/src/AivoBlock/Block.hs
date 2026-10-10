-- | Consensus-facing block types for the AivoBlock prototype.
--
-- This module defines data structures only. It deliberately does not
-- calculate block hashes, Merkle roots, or validate AI puzzle proofs:
-- those operations require a canonical encoding and protocol rules.
module AivoBlock.Block
  ( Hash256
  , hash256Length
  , mkHash256
  , hash256Bytes
  , zeroHash
  , BlockHeader(..)
  , BlockBody(..)
  , Block(..)
  , isGenesisCandidate
  ) where

import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import Data.Word (Word32, Word64)

-- | An opaque 32-byte value used for hashes and commitments.
--
-- All construction goes through 'mkHash256' to enforce the size.
-- The type does not prove that a value was cryptographically hashed.
newtype Hash256 = Hash256 ByteString
  deriving (Eq, Ord, Show)

hash256Length :: Int
hash256Length = 32

-- | Construct a 256-bit hash or commitment from exactly 32 bytes.
mkHash256 :: ByteString -> Either String Hash256
mkHash256 bytes
  | BS.length bytes == hash256Length = Right (Hash256 bytes)
  | otherwise =
      Left ("Expected 32 bytes for Hash256, got "
            ++ show (BS.length bytes))

-- | Access the raw bytes for serialization or cryptographic checking.
hash256Bytes :: Hash256 -> ByteString
hash256Bytes (Hash256 bytes) = bytes

-- | Reserved all-zero value. Its use for genesis is protocol-specific.
zeroHash :: Hash256
zeroHash = Hash256 (BS.replicate hash256Length 0)

-- | The consensus-relevant block header.
--
-- Transaction, AI-proof, and state roots are *commitments*. Validators
-- must recompute or verify them under an explicitly versioned protocol.
-- No hash of a header should be used for consensus until the canonical
-- byte encoding has been specified in protocol/spec/canonical-encoding.md.
data BlockHeader = BlockHeader
  { headerVersion          :: !Word32
  , headerChainId          :: !Word32
  , headerHeight           :: !Word64
  , headerParentHash       :: !Hash256
  , headerTransactionsRoot :: !Hash256
  , headerPuzzleProofsRoot :: !Hash256
  , headerStateRoot        :: !Hash256
  , headerTimestampMs      :: !Word64
  } deriving (Eq, Show)

-- | The identifiers of transactions and AI puzzle proofs in a block.
--
-- These references must be resolved and their contents validated
-- before a block can be accepted. Inclusion does not imply validity.
data BlockBody = BlockBody
  { bodyTransactionIds :: ![Hash256]
  , bodyPuzzleProofIds :: ![Hash256]
  } deriving (Eq, Show)

-- | A block consists of its header and associated body.
data Block = Block
  { blockHeader :: !BlockHeader
  , blockBody   :: !BlockBody
  } deriving (Eq, Show)

-- | A structural check, NOT full genesis block validation.
--
-- A real genesis block must also match the chain's fixed genesis
-- commitment, protocol version, initial ledger, and other rules.
isGenesisCandidate :: BlockHeader -> Bool
isGenesisCandidate header =
  headerHeight header == 0 && headerParentHash header == zeroHash
