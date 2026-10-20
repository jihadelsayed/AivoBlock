-- | Candidate V1 canonical identity for a recomputed AI-work receipt.
--
-- Domain-separated SHA-256 identifies supplied receipt fields. It does NOT
-- validate worker signatures, proof correctness or consensus acceptance.
module AivoBlock.ReceiptId
  ( canonicalReceiptIdV1
  ) where

import AivoBlock.Block (Hash256, hash256Bytes, mkHash256)
import Crypto.Hash (Digest, SHA256, hash)
import Data.ByteArray (convert)
import qualified Data.ByteString as BS
import Data.Bits (Bits, shiftR)
import Data.Word (Word8, Word32, Word64)

-- | Canonical 151-byte preimage:
-- ASCII "AIVOBLOCK-VERIFIED-WORK-RECEIPT-V1" + NUL, BE chain ID,
-- puzzle ID, worker public key, BE range start, BE unit count,
-- result commitment. All hashes and public keys are 32 raw bytes.
-- V1 does not accept zero chain ID or zero work-unit count.
canonicalReceiptIdV1
  :: Word32 -> Hash256 -> Hash256 -> Word64 -> Word64 -> Hash256
  -> Either String Hash256
canonicalReceiptIdV1 chain puzzle worker start count commitment
  | chain == 0 = Left "Chain ID must be positive"
  | count == 0 = Left "Receipt work range must be nonempty"
  | count > maxBound - start = Left "Receipt work range overflow"
  | otherwise =
      let preimage = BS.concat
            [ BS.pack (map (fromIntegral . fromEnum)
                "AIVOBLOCK-VERIFIED-WORK-RECEIPT-V1")
            , BS.singleton 0
            , be32 chain
            , hash256Bytes puzzle
            , hash256Bytes worker
            , be64 start
            , be64 count
            , hash256Bytes commitment
            ]
          digest = hash preimage :: Digest SHA256
      in mkHash256 (convert digest)

be32 :: Word32 -> BS.ByteString
be32 n = BS.pack [byte n shift | shift <- [24, 16, 8, 0]]

be64 :: Word64 -> BS.ByteString
be64 n = BS.pack [byte n shift | shift <- [56, 48, 40, 32, 24, 16, 8, 0]]

byte :: (Integral a, Bits a) => a -> Int -> Word8
byte n shift = fromIntegral (n `shiftR` shift)
