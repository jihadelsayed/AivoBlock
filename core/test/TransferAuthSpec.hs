module Main (main) where

import AivoBlock.Transaction
  ( Transaction(..), mkAddress, mkAmount )
import AivoBlock.TransferAuth
  ( transferSigningBytesV1, verifySignedTransferV1 )
import qualified Data.ByteString as BS
import Data.Char (digitToInt)
import Test.Hspec

main :: IO ()
main = hspec $ do
  describe "Ed25519 transfer authentication V1" $ do
    it "accepts the fixed Rust/Python seed-7 vector and canonical 118 bytes" $ do
      verifySignedTransferV1 valid `shouldBe` Right ()
      fmap BS.length (transferSigningBytesV1 valid) `shouldBe` Right 118

    it "rejects tampered amount, fee, nonce, recipient and signature" $ do
      mapM_ (\tx -> verifySignedTransferV1 tx `shouldSatisfy` isLeft)
        [ valid { txAmount = mkAmount 11 }
        , valid { txFee = mkAmount 3 }
        , valid { txNonce = 1 }
        , valid { txRecipient = address (BS.replicate 32 3) }
        , valid { txSignature = BS.replicate 64 0 }
        ]

    it "rejects owner mismatch, chain replay, version and unsupported reference" $ do
      mapM_ (\tx -> verifySignedTransferV1 tx `shouldSatisfy` isLeft)
        [ valid { txSender = address (BS.replicate 32 4) }
        , valid { txChainId = 2 }
        , valid { txVersion = 2 }
        , valid { txAmount = mkAmount 0 }
        , valid { txPublicKey = BS.replicate 32 0 }
        , valid { txSignature = BS.replicate 63 0 }
        ]

isLeft :: Either String () -> Bool
isLeft (Left _) = True
isLeft _ = False

address bytes = case mkAddress bytes of
  Left err -> error err
  Right value -> value

fromHex :: String -> BS.ByteString
fromHex (a:b:rest) =
  fromIntegral (digitToInt a * 16 + digitToInt b) `BS.cons` fromHex rest
fromHex [] = BS.empty
fromHex _ = error "odd hex literal"

valid :: Transaction
valid = Transaction
  { txVersion = 1
  , txChainId = 1
  , txSender = address pub
  , txRecipient = address (BS.replicate 32 2)
  , txAmount = mkAmount 10
  , txFee = mkAmount 2
  , txNonce = 0
  , txReference = Nothing
  , txPublicKey = pub
  , txSignature = fromHex
      "f1cc31b91b3485d813bbe245ee742795eb94dcd7cfdba90d30d1a4da9ec18ca9822a6ef0a36c54a58a956de172eee42816ec51b2a0dfbcff7422d62deb952405"
  }
  where
    pub = fromHex "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"
