module Main (main) where

import AivoBlock.Ledger
import AivoBlock.Transaction
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Word (Word8, Word64)
import Test.Hspec
import Test.QuickCheck (NonNegative(..), property)

main :: IO ()
main = hspec $ do
  describe "Empty ledger" $ do
    it "starts with zero balances and nonces" $ do
      balanceOf alice emptyLedger `shouldBe` 0
      nonceOf alice emptyLedger `shouldBe` 0
      ledgerFeePool emptyLedger `shouldBe` 0
      ledgerSettledRewardBatches emptyLedger `shouldBe` Set.empty
      ledgerPaidWorkReceipts emptyLedger `shouldBe` Set.empty

  describe "Verified transfers" $ do
    it "moves funds, collects fees and advances the sender nonce" $ do
      let result = applyVerifiedTransfer (transfer alice bob 30 2 0) (funded 100)
      case result of
        Left err -> expectationFailure err
        Right ledger -> do
          balanceOf alice ledger `shouldBe` 68
          balanceOf bob ledger `shouldBe` 30
          nonceOf alice ledger `shouldBe` 1
          ledgerFeePool ledger `shouldBe` 2

    it "rejects insufficient funds" $
      applyVerifiedTransfer (transfer alice bob 100 1 0) (funded 100)
        `shouldBe` Left "Insufficient funds"

    it "rejects an incorrect nonce" $
      applyVerifiedTransfer (transfer alice bob 1 0 1) (funded 100)
        `shouldBe` Left "Invalid account nonce"

    it "rejects a zero-value transfer" $
      applyVerifiedTransfer (transfer alice bob 0 0 0) (funded 100)
        `shouldBe` Left "Transfer amount must be positive"

    it "charges a fee for a self-transfer without changing the principal" $ do
      let result = applyVerifiedTransfer (transfer alice alice 20 3 0) (funded 100)
      case result of
        Left err -> expectationFailure err
        Right ledger -> do
          balanceOf alice ledger `shouldBe` 97
          nonceOf alice ledger `shouldBe` 1
          ledgerFeePool ledger `shouldBe` 3

    it "rejects recipient balance overflow" $
      let ledger = Ledger
            (Map.fromList [(alice, Account 10 0), (bob, Account maxBound 0)]) 0
            Set.empty Set.empty
      in applyVerifiedTransfer (transfer alice bob 1 0 0) ledger
           `shouldBe` Left "Unsigned 64-bit overflow"

    it "rejects amount plus fee overflow" $
      applyVerifiedTransfer (transfer alice bob maxBound 1 0) (funded 100)
        `shouldBe` Left "Unsigned 64-bit overflow"

    it "preserves funds plus accumulated fees for an ordinary transfer" $
      property $ \(NonNegative rawAmount) (NonNegative rawFee) ->
        let amount = fromIntegral ((rawAmount :: Int) `mod` 1001) :: Word64
            fee = fromIntegral ((rawFee :: Int) `mod` 1001) :: Word64
            initial = funded 3000
            result = applyVerifiedTransfer (transfer alice bob amount fee 0) initial
        in if amount == 0
             then result == Left "Transfer amount must be positive"
             else case result of
               Left _ -> False
               Right ledger ->
                 balanceOf alice ledger + balanceOf bob ledger
                   + ledgerFeePool ledger == 3000

alice, bob :: Address
alice = address 1
bob = address 2

address :: Word8 -> Address
address byte =
  case mkAddress (BS.replicate 32 byte) of
    Right value -> value
    Left err -> error err

funded :: Word64 -> Ledger
funded amount = Ledger (Map.singleton alice (Account amount 0)) 0
  Set.empty Set.empty

transfer :: Address -> Address -> Word64 -> Word64 -> Word64 -> Transaction
transfer sender recipient amount fee nonce =
  Transaction
    { txVersion = 1
    , txChainId = 1
    , txSender = sender
    , txRecipient = recipient
    , txAmount = mkAmount amount
    , txFee = mkAmount fee
    , txNonce = nonce
    , txReference = Nothing
    , txPublicKey = BS.replicate 32 7
    , txSignature = BS.replicate 64 9
    }
