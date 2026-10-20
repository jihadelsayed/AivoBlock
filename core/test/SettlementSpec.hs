module Main (main) where

import AivoBlock.Block (Hash256, mkHash256, zeroHash)
import AivoBlock.Ledger
import AivoBlock.Settlement
import AivoBlock.Transaction
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Word (Word8, Word64)
import Test.Hspec
import Test.QuickCheck (Positive(..), property)

main :: IO ()
main = hspec $ do
  describe "Fee-funded AI reward settlement (trusted inputs)" $ do
    it "spends existing fees, credits workers deterministically, and keeps nonces" $ do
      let claims = [claim 1 1 bob 2, claim 2 2 carol 1]
      case settleFundedRewards (hash 90) 6 claims (ledgerWithPool 10) of
        Left err -> expectationFailure err
        Right ledger -> do
          ledgerFeePool ledger `shouldBe` 4
          balanceOf bob ledger `shouldBe` 4
          balanceOf carol ledger `shouldBe` 2
          balanceOf alice ledger `shouldBe` 200
          nonceOf alice ledger `shouldBe` 4
          nonceOf bob ledger `shouldBe` 0
          ledgerSettledRewardBatches ledger
            `shouldBe` Set.singleton (hash 90)
          ledgerPaidWorkReceipts ledger
            `shouldBe` Set.fromList [hash 1, hash 2]

    it "is independent of receipt order and uses worker ID for remainder ties" $ do
      let claims = [claim 1 2 carol 1, claim 2 1 bob 1]
          a = settleFundedRewards (hash 90) 1 claims (ledgerWithPool 5)
          b = settleFundedRewards (hash 90) 1 (reverse claims) (ledgerWithPool 5)
      a `shouldBe` b
      case a of
        Left err -> expectationFailure err
        Right ledger -> do
          balanceOf bob ledger `shouldBe` 1
          balanceOf carol ledger `shouldBe` 0

    it "rejects insufficient fee funding without changing the source ledger" $
      settleFundedRewards (hash 90) 11 [claim 1 1 bob 1] (ledgerWithPool 10)
        `shouldBe` Left "Insufficient fee pool"

    it "rejects the same reward batch replay" $ do
      let first = settleFundedRewards (hash 90) 2 [claim 1 1 bob 1]
            (ledgerWithPool 10)
      case first of
        Left err -> expectationFailure err
        Right ledger ->
          settleFundedRewards (hash 90) 2 [claim 2 2 carol 1] ledger
            `shouldBe` Left "Reward batch already settled"

    it "rejects already-paid receipts even under a new batch ID" $ do
      let first = settleFundedRewards (hash 90) 2 [claim 1 1 bob 1]
            (ledgerWithPool 10)
      case first of
        Left err -> expectationFailure err
        Right ledger ->
          settleFundedRewards (hash 91) 2 [claim 1 1 bob 1] ledger
            `shouldBe` Left "Work receipt already paid"

    it "rejects duplicate receipts within one batch" $
      settleFundedRewards (hash 90) 2
        [claim 1 1 bob 1, claim 1 1 bob 1] (ledgerWithPool 10)
          `shouldBe` Left "Duplicate work receipt in reward batch"

    it "rejects inconsistent recipient addresses for the same worker" $
      settleFundedRewards (hash 90) 2
        [claim 1 1 bob 1, claim 2 1 carol 1] (ledgerWithPool 10)
          `shouldBe` Left "Conflicting payout addresses for worker"

    it "rejects overflow when many receipts for one worker are combined" $
      settleFundedRewards (hash 90) 2
        [claim 1 1 bob maxBound, claim 2 1 bob 1] (ledgerWithPool 10)
          `shouldBe` Left "Worker weight overflow"

    it "rejects recipient balance overflow atomically" $
      let ledger = (ledgerWithPool 10)
            { ledgerAccounts = Map.fromList
                [(alice, Account 200 4), (bob, Account maxBound 7)] }
      in settleFundedRewards (hash 90) 1 [claim 1 1 bob 1] ledger
           `shouldBe` Left "Unsigned 64-bit overflow"

    it "rejects a zero batch ID" $
      settleFundedRewards zeroHash 1 [claim 1 1 bob 1] (ledgerWithPool 10)
        `shouldBe` Left "Zero reward batch ID"

    it "rejects a zero receipt ID or worker ID" $ do
      settleFundedRewards (hash 90) 1
        [claim 0 1 bob 1] (ledgerWithPool 10)
          `shouldBe` Left "Zero work receipt ID"
      settleFundedRewards (hash 90) 1
        [claim 1 0 bob 1] (ledgerWithPool 10)
          `shouldBe` Left "Zero worker ID"

    it "rejects zero units, zero budget, and an empty batch" $ do
      settleFundedRewards (hash 90) 1
        [claim 1 1 bob 0] (ledgerWithPool 10)
          `shouldBe` Left "Work units must be positive"
      settleFundedRewards (hash 90) 0
        [claim 1 1 bob 1] (ledgerWithPool 10)
          `shouldBe` Left "Reward budget must be positive"
      settleFundedRewards (hash 90) 1 [] (ledgerWithPool 10)
        `shouldBe` Left "No reward entitlements"

    it "retains settled IDs and fees through later verified transfers" $ do
      let first = settleFundedRewards (hash 90) 4
            [claim 1 1 bob 1] (ledgerWithPool 10)
      case first of
        Left err -> expectationFailure err
        Right paid ->
          case applyVerifiedTransfer (transfer alice carol 8 2 4) paid of
            Left err -> expectationFailure err
            Right transferred -> do
              ledgerFeePool transferred `shouldBe` 8
              balanceOf carol transferred `shouldBe` 8
              ledgerSettledRewardBatches transferred
                `shouldBe` ledgerSettledRewardBatches paid
              ledgerPaidWorkReceipts transferred
                `shouldBe` ledgerPaidWorkReceipts paid

    it "never changes balances plus fees on successful settlements" $
      property $ \(Positive rawBudget) (Positive rawUnits) ->
        let budget = fromIntegral (1 + (rawBudget :: Int) `mod` 1000) :: Word64
            units = fromIntegral (1 + (rawUnits :: Int) `mod` 1000) :: Word64
            initial = ledgerWithPool 1000
        in case settleFundedRewards (hash 90) budget
                  [claim 1 1 bob units, claim 2 2 carol 1] initial of
          Left _ -> False
          Right ledger -> totalSupply ledger == totalSupply initial

hash :: Word8 -> Hash256
hash byte = case mkHash256 (BS.replicate 32 byte) of
  Right result -> result
  Left err -> error err

alice, bob, carol :: Address
alice = address 11
bob = address 12
carol = address 13

address :: Word8 -> Address
address byte = case mkAddress (BS.replicate 32 byte) of
  Right result -> result
  Left err -> error err

ledgerWithPool :: Word64 -> Ledger
ledgerWithPool amount = Ledger
  (Map.singleton alice (Account 200 4)) amount Set.empty Set.empty

claim :: Word8 -> Word8 -> Address -> Word64 -> RewardEntitlement
claim receiptId worker recipient units =
  RewardEntitlement (hash receiptId) (hash worker) recipient units

totalSupply :: Ledger -> Integer
totalSupply ledger =
  sum (map (toInteger . accountBalance) (Map.elems (ledgerAccounts ledger)))
    + toInteger (ledgerFeePool ledger)

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
