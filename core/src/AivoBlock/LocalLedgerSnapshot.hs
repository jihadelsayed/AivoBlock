-- | Byte-identical local snapshot codec shared with Rust RocksDB.
-- This is an experimental storage format, NOT a consensus state root.
module AivoBlock.LocalLedgerSnapshot
  ( encodeLedgerSnapshot
  , decodeLedgerSnapshot
  ) where

import AivoBlock.Block (hash256Bytes, mkHash256)
import AivoBlock.Ledger (Account(..), Ledger(..))
import AivoBlock.Transaction (addressBytes, mkAddress)
import Control.Monad (foldM, when)
import Data.Bits (shiftL, shiftR)
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Word (Word8, Word32, Word64)

magic :: BS.ByteString
magic = BS.pack (map (fromIntegral . fromEnum)
  "AIVOBLOCK-LOCAL-LEDGER-V1") `BS.snoc` 0

be32 :: Word32 -> BS.ByteString
be32 n = BS.pack [fromIntegral (n `shiftR` offset) | offset <- [24, 16, 8, 0]]

be64 :: Word64 -> BS.ByteString
be64 n = BS.pack [fromIntegral (n `shiftR` offset)
  | offset <- [56, 48, 40, 32, 24, 16, 8, 0]]

count32 :: Int -> Either String BS.ByteString
count32 n
  | toInteger n > toInteger (maxBound :: Word32) = Left "Too many snapshot entries"
  | otherwise = Right (be32 (fromIntegral n))

-- | Canonical ordering is Map/Set ascending, with unsigned BE widths.
encodeLedgerSnapshot :: Ledger -> Either String BS.ByteString
encodeLedgerSnapshot ledger = do
  let accounts = Map.toAscList (ledgerAccounts ledger)
      batches = Set.toAscList (ledgerSettledRewardBatches ledger)
      receipts = Set.toAscList (ledgerPaidWorkReceipts ledger)
  accountCount <- count32 (length accounts)
  batchCount <- count32 (length batches)
  receiptCount <- count32 (length receipts)
  pure (BS.concat
    [ magic
    , be64 (ledgerFeePool ledger)
    , accountCount
    , BS.concat
        [ BS.concat [addressBytes address, be64 balance, be64 nonce]
        | (address, Account balance nonce) <- accounts ]
    , batchCount
    , BS.concat (map hash256Bytes batches)
    , receiptCount
    , BS.concat (map hash256Bytes receipts)
    ])

takeAt :: Int -> BS.ByteString -> Int -> Either String (BS.ByteString, Int)
takeAt n input offset
  | n < 0 || offset < 0 || offset > BS.length input
     || n > BS.length input - offset = Left "Malformed ledger snapshot"
  | otherwise = Right (BS.take n (BS.drop offset input), offset + n)

wordAt :: Int -> BS.ByteString -> Int -> Either String (Word64, Int)
wordAt n input offset = do
  (bytes, next) <- takeAt n input offset
  pure (BS.foldl' (\v b -> (v `shiftL` 8) + fromIntegral b) 0 bytes, next)

readCount :: Int -> BS.ByteString -> Int -> Either String (Int, Int)
readCount itemSize input offset = do
  (value, next) <- wordAt 4 input offset
  let count = fromIntegral value
  when (count > (BS.length input - next) `div` itemSize)
    (Left "Malformed ledger snapshot")
  pure (count, next)

-- | Reject malformed, duplicate, out-of-order and trailing records via
-- an exact canonical re-encoding check. No unbounded count allocation.
decodeLedgerSnapshot :: BS.ByteString -> Either String Ledger
decodeLedgerSnapshot input = do
  when (not (magic `BS.isPrefixOf` input)) (Left "Bad snapshot domain")
  let start = BS.length magic
  (fees, atFees) <- wordAt 8 input start
  (nAccounts, accountStart) <- readCount 48 input atFees
  (accounts, afterAccounts) <- foldM readAccount (Map.empty, accountStart) [1..nAccounts]
  (nBatches, batchStart) <- readCount 32 input afterAccounts
  (batches, afterBatches) <- foldM readHash (Set.empty, batchStart) [1..nBatches]
  (nReceipts, receiptStart) <- readCount 32 input afterBatches
  (receipts, done) <- foldM readHash (Set.empty, receiptStart) [1..nReceipts]
  let ledger = Ledger accounts fees batches receipts
  canonical <- encodeLedgerSnapshot ledger
  when (done /= BS.length input || canonical /= input)
    (Left "Noncanonical ledger snapshot")
  pure ledger
  where
    readAccount (accounts, offset) _ = do
      (rawAddress, atAddress) <- takeAt 32 input offset
      address <- mkAddress rawAddress
      (balance, atBalance) <- wordAt 8 input atAddress
      (nonce, next) <- wordAt 8 input atBalance
      when (Map.member address accounts) (Left "Duplicate account")
      pure (Map.insert address (Account balance nonce) accounts, next)
    readHash (entries, offset) _ = do
      (rawHash, next) <- takeAt 32 input offset
      identifier <- mkHash256 rawHash
      when (Set.member identifier entries) (Left "Duplicate identifier")
      pure (Set.insert identifier entries, next)
