-- | Candidate V1 signed transaction identifiers and ordered Merkle roots.
-- Only the exact signed V1 envelope is supported. Does not prove block
-- validity, transaction execution, validator votes or finality.
module AivoBlock.TransactionCommitment
  ( signedTransactionIdV1
  , transactionRootV1
  , ledgerStateRootV1
  ) where

import AivoBlock.Block (Hash256, hash256Bytes, mkHash256)
import AivoBlock.LocalLedgerSnapshot (encodeLedgerSnapshot)
import AivoBlock.Ledger (Ledger)
import AivoBlock.Transaction (Transaction(..))
import AivoBlock.TransferAuth (transferSigningBytesV1, verifySignedTransferV1)
import Crypto.Hash (Digest, SHA256, hash)
import Data.ByteArray (convert)
import qualified Data.ByteString as BS

digest :: BS.ByteString -> Hash256
digest input = case mkHash256 (convert (hash input :: Digest SHA256)) of
  Right result -> result
  Left errorMessage -> error errorMessage

domain :: String -> BS.ByteString
domain name = BS.pack (map (fromIntegral . fromEnum) name) `BS.snoc` 0

-- | SHA256("AIVOBLOCK-TX-ID-V1" || NUL ||
--           canonical signing bytes[118] || Ed25519 signature[64]).
-- Reject invalid signatures; no ID for unauthenticated envelope.
signedTransactionIdV1 :: Transaction -> Either String Hash256
signedTransactionIdV1 tx = do
  verifySignedTransferV1 tx
  bytes <- transferSigningBytesV1 tx
  pure (digest (BS.concat [domain "AIVOBLOCK-TX-ID-V1", bytes, txSignature tx]))

-- | SHA256("AIVOBLOCK-TX-EMPTY-V1" || NUL) for empty body.
-- Each leaf = SHA256("AIVOBLOCK-TX-LEAF-V1" || NUL || txID[32]).
-- Each parent = SHA256("AIVOBLOCK-TX-NODE-V1" || NUL ||
--                      left[32] || right[32]); duplicate last odd node.
-- Ordered IDs, at most 65536; duplicates explicitly forbidden.
transactionRootV1 :: [Hash256] -> Either String Hash256
transactionRootV1 ids
  | length ids > 65536 = Left "Too many transactions in a block"
  | length ids /= length (unique ids) = Left "Duplicate transaction ID"
  | null ids = Right (digest (domain "AIVOBLOCK-TX-EMPTY-V1"))
  | otherwise = Right (finish (map leaf ids))
  where
    leaf txId = digest (BS.append (domain "AIVOBLOCK-TX-LEAF-V1")
                                  (hash256Bytes txId))
    finish [one] = one
    finish nodes = finish (parents nodes)
    parents (left:right:rest) = combine left right : parents rest
    parents [lastNode] = [combine lastNode lastNode]
    parents [] = []
    combine left right = digest (BS.concat
      [ domain "AIVOBLOCK-TX-NODE-V1"
      , hash256Bytes left
      , hash256Bytes right ])
    unique = foldr (\item acc -> if item `elem` acc then acc else item : acc) []

-- | Local ledger commitment with domain separation, not a consensus trie.
ledgerStateRootV1 :: Ledger -> Either String Hash256
ledgerStateRootV1 ledger = do
  bytes <- encodeLedgerSnapshot ledger
  pure (digest (BS.append (domain "AIVOBLOCK-STATE-ROOT-V1") bytes))
