module Main (main) where

import AivoBlock.Block (Hash256, mkHash256)
import AivoBlock.Issuance
import AivoBlock.Ledger
import AivoBlock.Settlement (RewardEntitlement(..))
import AivoBlock.Transaction (Address, mkAddress)
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Word (Word8, Word64)
import Test.Hspec
import Test.QuickCheck (Positive(..), property)

main :: IO ()
main = hspec $ do
  describe "Sayed Token issuance schedule" $ do
    it "halves every interval and never issues at genesis" $ do
      let policy = testPolicy
      map (\h -> scheduledIssuance policy h 0) [1, 100, 101, 200, 201, 301]
        `shouldBe` map Right [50, 50, 25, 25, 12, 6]
      scheduledIssuance policy 0 0 `shouldBe` Left "Genesis does not issue rewards"
      scheduledIssuance policy (64 * 100 + 1) 0 `shouldBe` Right 0
      scheduledIssuance policy maxBound 0 `shouldBe` Right 0

    it "truncates at the maximum supply and rejects an over-cap ledger" $ do
      scheduledIssuance testPolicy 1 (2000000 - 10) `shouldBe` Right 10
      scheduledIssuance testPolicy 1 2000000 `shouldBe` Right 0
      scheduledIssuance testPolicy 1 2000001
        `shouldBe` Left "Ledger supply exceeds issuance cap"

    it "rejects invalid policies" $ do
      mkIssuancePolicy 50 0 100 `shouldBe` Left "Halving interval must be positive"
      mkIssuancePolicy 50 1 0 `shouldBe` Left "Maximum supply must be positive"
      mkIssuancePolicy 101 1 100 `shouldBe` Left "Initial reward exceeds maximum supply"

  describe "Issued AI-work reward settlement" $ do
    it "matches the shared Rust/Python cross-language vector" $
      case settleIssuedRewards testPolicy 1 (hash 90) 30 50 vectorClaims vectorLedger of
        Left err -> expectationFailure err
        Right ledger -> do
          ledgerFeePool ledger `shouldBe` 570
          balanceOf (address 1) ledger `shouldBe` 27
          balanceOf (address 2) ledger `shouldBe` 27
          balanceOf (address 3) ledger `shouldBe` 26
          balanceOf alice ledger `shouldBe` 200
          nonceOf alice ledger `shouldBe` 4
          ledgerSupply ledger `shouldBe` 850
          ledgerSettledRewardBatches ledger `shouldBe` Set.singleton (hash 90)
          ledgerPaidWorkReceipts ledger
            `shouldBe` Set.fromList [hash 101, hash 102, hash 103]

    it "rejects issuance that differs from the schedule" $ do
      settleIssuedRewards testPolicy 1 (hash 90) 30 51 vectorClaims vectorLedger
        `shouldBe` Left "Issuance differs from policy schedule"
      settleIssuedRewards testPolicy 101 (hash 90) 30 50 vectorClaims vectorLedger
        `shouldBe` Left "Issuance differs from policy schedule"

    it "never pays a fee budget that existing fees cannot cover" $
      settleIssuedRewards testPolicy 1 (hash 90) 601 50 vectorClaims vectorLedger
        `shouldBe` Left "Insufficient fee pool"

    it "pays issuance alone when the fee budget is zero" $
      case settleIssuedRewards testPolicy 1 (hash 90) 0 50 vectorClaims vectorLedger of
        Left err -> expectationFailure err
        Right ledger -> do
          ledgerFeePool ledger `shouldBe` 600
          ledgerSupply ledger `shouldBe` 850

    it "rejects an empty reward once issuance has run out" $ do
      let exhausted = vectorLedger { ledgerFeePool = 2000000 - 200 }
      settleIssuedRewards testPolicy 1 (hash 90) 0 0 vectorClaims exhausted
        `shouldBe` Left "Reward budget must be positive"

    it "keeps replay protection from fee-funded settlement" $
      case settleIssuedRewards testPolicy 1 (hash 90) 30 50 vectorClaims vectorLedger of
        Left err -> expectationFailure err
        Right ledger -> do
          settleIssuedRewards testPolicy 2 (hash 90) 30 50 vectorClaims ledger
            `shouldBe` Left "Reward batch already settled"
          settleIssuedRewards testPolicy 2 (hash 91) 30 50 vectorClaims ledger
            `shouldBe` Left "Work receipt already paid"

    it "grows supply by exactly the issuance and never past the cap" $
      property $ \(Positive rawHeight) (Positive rawPool) (Positive rawUnits) ->
        let height = fromIntegral (1 + (rawHeight :: Int) `mod` 1000) :: Word64
            pool = fromIntegral ((rawPool :: Int) `mod` 1999700) :: Word64
            units = fromIntegral (1 + (rawUnits :: Int) `mod` 50) :: Word64
            initial = vectorLedger { ledgerFeePool = pool }
            supply = ledgerSupply initial
            claims = [ RewardEntitlement (hash 101) (hash 1) (address 1) units
                     , RewardEntitlement (hash 102) (hash 2) (address 2) 1 ]
        in case scheduledIssuance testPolicy height supply of
             Left _ -> False
             Right issued ->
               case settleIssuedRewards testPolicy height (hash 90) 0 issued claims initial of
                 Left _ -> issued == 0
                 Right ledger ->
                   ledgerSupply ledger == supply + toInteger issued
                     && ledgerSupply ledger <= 2000000

testPolicy :: IssuancePolicy
testPolicy = case mkIssuancePolicy 50 100 2000000 of
  Right policy -> policy
  Left err -> error err

vectorLedger :: Ledger
vectorLedger = Ledger (Map.singleton alice (Account 200 4)) 600 Set.empty Set.empty

vectorClaims :: [RewardEntitlement]
vectorClaims =
  [ RewardEntitlement (hash 101) (hash 1) (address 1) 1
  , RewardEntitlement (hash 102) (hash 2) (address 2) 1
  , RewardEntitlement (hash 103) (hash 3) (address 3) 1
  ]

hash :: Word8 -> Hash256
hash byte = case mkHash256 (BS.replicate 32 byte) of
  Right result -> result
  Left err -> error err

alice :: Address
alice = address 11

address :: Word8 -> Address
address byte = case mkAddress (BS.replicate 32 byte) of
  Right result -> result
  Left err -> error err
