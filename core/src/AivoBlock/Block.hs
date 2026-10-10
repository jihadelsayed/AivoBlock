-- | Consensus-facing block types for the AivoBlock prototype.
--
-- This module defines block types and a candidate V1 header encoding.
-- It can hash candidate V1 headers; it does not verify committed roots or AI proofs.
module AivoBlock.Block
  ( Hash256
  , hash256Length
  , mkHash256
  , hash256Bytes
  , zeroHash
  , encodeBlockHeaderV1
  , hashBlockHeaderV1
  , BlockHeader(..)
  , BlockBody(..)
  , Block(..)
  , isGenesisCandidate
  ) where

import Crypto.Hash (Digest, SHA256, hash)
import Data.ByteArray (convert)
import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import Data.Bits (Bits, shiftR)
import Data.Word (Word8)
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
-- Encoding is specified in protocol/spec/canonical-encoding.md, but
-- this candidate is not yet approved for consensus.
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


-- | Candidate canonical V1 block-header preimage (178 bytes).
--
-- ASCII domain tag plus NUL; then unsigned big-endian fields in the exact
-- order specified by protocol/spec/canonical-encoding.md.
-- This function supplies the SHA-256 preimage, but does not calculate
-- its digest or establish block validity.
-- V1 is deliberately restricted to protocol header version 1.
encodeBlockHeaderV1 :: BlockHeader -> Either String ByteString
encodeBlockHeaderV1 h
  | headerVersion h /= 1 = Left "Unsupported V1 header version"
  | headerChainId h == 0 = Left "Chain ID must not be zero"
  | otherwise = Right (BS.concat
      [ BS.pack (map (fromIntegral . fromEnum) "AIVOBLOCK-BLOCK-HEADER-V1") 
      , BS.singleton 0
      , word32BE (headerVersion h)
      , word32BE (headerChainId h)
      , word64BE (headerHeight h)
      , hash256Bytes (headerParentHash h)
      , hash256Bytes (headerTransactionsRoot h)
      , hash256Bytes (headerPuzzleProofsRoot h)
      , hash256Bytes (headerStateRoot h)
      , word64BE (headerTimestampMs h)
      ])
  where
    word32BE :: Word32 -> ByteString
    word32BE n = BS.pack [byte n s | s <- [24,16,8,0]]

    word64BE :: Word64 -> ByteString
    word64BE n = BS.pack [byte n s | s <- [56,48,40,32,24,16,8,0]]

    byte :: (Integral a, Bits a) => a -> Int -> Word8
    byte n s = fromIntegral (n `shiftR` s)

-- | SHA-256 of the exact 178-byte V1 header encoding.
-- This digest identifies the supplied header bytes only; it does not
-- establish valid Merkle roots, a valid ledger, or consensus approval.
hashBlockHeaderV1 :: BlockHeader -> Either String Hash256
hashBlockHeaderV1 header = do
  preimage <- encodeBlockHeaderV1 header
  let digest = hash preimage :: Digest SHA256
  mkHash256 (convert digest)

-- | A structural check, NOT full genesis block validation.
--
-- A real genesis block must also match the chain's fixed genesis
-- commitment, protocol version, initial ledger, and other rules.
isGenesisCandidate :: BlockHeader -> Bool
isGenesisCandidate header =
  headerHeight header == 0 && headerParentHash header == zeroHash
