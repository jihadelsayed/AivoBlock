module Main (main) where

import AivoBlock.Block (Hash256, mkHash256)
import AivoBlock.Ledger (Account(..), Ledger(..), applyVerifiedTransfer)
import AivoBlock.LocalLedgerSnapshot (encodeLedgerSnapshot, decodeLedgerSnapshot)
import AivoBlock.Transaction
  ( Address, Transaction(..), mkAddress, mkAmount )
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Test.Hspec

main :: IO ()
main = hspec $ do
  describe "Rust compatible typed ledger snapshots" $ do
    it "re-encodes and decodes balances, nonces, fees and paid IDs" $ do
      let Right encoded = encodeLedgerSnapshot initial
      decodeLedgerSnapshot encoded `shouldBe` Right initial
      BS.take 26 encoded `shouldBe`
        BS.pack (map (fromIntegral . fromEnum) "AIVOBLOCK-LOCAL-LEDGER-V1" ++ [0])
    it "is independent of account input order" $ do
      let swapped = initial
            { ledgerAccounts = Map.fromList (reverse (Map.toList (ledgerAccounts initial))) }
      encodeLedgerSnapshot swapped `shouldBe` encodeLedgerSnapshot initial
    it "rejects every truncated prefix and trailing data" $ do
      case encodeLedgerSnapshot initial of
        Left problem -> expectationFailure problem
        Right encoded -> do
          mapM_ (\n -> decodeLedgerSnapshot (BS.take n encoded)
            `shouldSatisfy` isLeft) [0 .. BS.length encoded - 1]
          decodeLedgerSnapshot (encoded `BS.snoc` 0) `shouldSatisfy` isLeft
    it "rejects duplicate or unordered accounts" $ do
      case encodeLedgerSnapshot initial of
        Left problem -> expectationFailure problem
        Right encoded -> do
          let entryStart = 26 + 8 + 4
              first = BS.take 48 (BS.drop entryStart encoded)
              second = BS.take 48 (BS.drop (entryStart + 48) encoded)
              prefix = BS.take entryStart encoded
              suffix = BS.drop (entryStart + 96) encoded
          decodeLedgerSnapshot (BS.concat [prefix, second, first, suffix])
            `shouldSatisfy` isLeft
          decodeLedgerSnapshot (BS.concat [prefix, first, first, suffix])
            `shouldSatisfy` isLeft
    it "round-trips state obtained from Haskell applyVerifiedTransfer" $ do
      case applyVerifiedTransfer transfer initial of
        Left problem -> expectationFailure problem
        Right moved -> do
          let Right bytes = encodeLedgerSnapshot moved
          decodeLedgerSnapshot bytes `shouldBe` Right moved

isLeft :: Either String a -> Bool
isLeft (Left _) = True
isLeft _ = False

address :: Word -> Address
address b = case mkAddress (BS.replicate 32 (fromIntegral b)) of
  Right a -> a
  Left problem -> error problem

initial :: Ledger
initial = Ledger
  (Map.fromList [(address 1, Account 100 0), (address 2, Account 0 4)])
  7
  (Set.singleton (hash 9))
  (Set.singleton (hash 8))

hash :: Word -> Hash256
hash b = case mkHash256 (BS.replicate 32 (fromIntegral b)) of
  Right a -> a
  Left problem -> error problem

transfer :: Transaction
transfer = Transaction
  { txVersion = 1
  , txChainId = 1
  , txSender = address 1
  , txRecipient = address 2
  , txAmount = mkAmount 12
  , txFee = mkAmount 3
  , txNonce = 0
  , txReference = Nothing
  , txPublicKey = BS.replicate 32 7
  , txSignature = BS.replicate 64 9
  }
