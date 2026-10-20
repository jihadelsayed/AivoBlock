module Main (main) where

import AivoBlock.BatchExecution (applySignedBatchV1)
import AivoBlock.Block (hash256Bytes)
import AivoBlock.Ledger (Account(..), Ledger(..))
import AivoBlock.Transaction (Address, Transaction(..), mkAddress, mkAmount, addressBytes)
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Char (digitToInt)
import Test.Hspec

main :: IO ()
main = hspec $ describe "Ordered signed block execution" $ do
  it "applies sequential nonces, fees and balances atomically" $ do
    case applySignedBatchV1 initial [first, second] of
      Left problem -> expectationFailure problem
      Right (state, ids, txRoot, stateRoot) -> do
        fmap accountBalance (Map.lookup alice (ledgerAccounts state))
          `shouldBe` Just 82
        fmap accountNonce (Map.lookup alice (ledgerAccounts state))
          `shouldBe` Just 2
        fmap accountBalance (Map.lookup bob (ledgerAccounts state))
          `shouldBe` Just 15
        ledgerFeePool state `shouldBe` 3
        length ids `shouldBe` 2
        BS.length (hash256Bytes txRoot) `shouldBe` 32
        BS.length (hash256Bytes stateRoot) `shouldBe` 32

  it "rejects a duplicate signed transaction in a block" $
    applySignedBatchV1 initial [first, first] `shouldSatisfy` isLeft

  it "rejects reordered nonce-dependent transactions" $
    applySignedBatchV1 initial [second, first] `shouldSatisfy` isLeft

  it "rejects a valid signed later overspend without partial application" $ do
    let overspend = second {txAmount = mkAmount 500}
    applySignedBatchV1 initial [first, overspend] `shouldSatisfy` isLeft

  it "rejects tampering even on the second transaction" $ do
    applySignedBatchV1 initial [first, second {txAmount=mkAmount 6}]
      `shouldSatisfy` isLeft

  it "rejects empty and oversized batches" $ do
    applySignedBatchV1 initial [] `shouldSatisfy` isLeft
    applySignedBatchV1 initial (replicate 65 first) `shouldSatisfy` isLeft

isLeft :: Either String a -> Bool
isLeft (Left _) = True
isLeft _ = False

fromHex :: String -> BS.ByteString
fromHex (a:b:rest) = fromIntegral (digitToInt a * 16 + digitToInt b)
  `BS.cons` fromHex rest
fromHex [] = BS.empty
fromHex _ = error "Odd hex literal"

address :: BS.ByteString -> Address
address value = case mkAddress value of
  Right result -> result
  Left problem -> error problem

alice :: Address
alice = address (fromHex
  "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c")

bob :: Address
bob = address (BS.replicate 32 2)

initial :: Ledger
initial = Ledger (Map.singleton alice (Account 100 0)) 0
  Set.empty Set.empty

first :: Transaction
first = Transaction 1 1 alice bob (mkAmount 10) (mkAmount 2) 0 Nothing
  (addressBytes alice)
  (fromHex "f1cc31b91b3485d813bbe245ee742795eb94dcd7cfdba90d30d1a4da9ec18ca9822a6ef0a36c54a58a956de172eee42816ec51b2a0dfbcff7422d62deb952405")

second :: Transaction
second = Transaction 1 1 alice bob (mkAmount 5) (mkAmount 1) 1 Nothing
  (addressBytes alice)
  (fromHex "11b05f1a23e906426e4d87f62ace47fb315f3112e15813483a55d8762ef8d0fdd6a3e64ae1c42a8f2c188030760a58947924614e14c785fa038166466693fb0e")
