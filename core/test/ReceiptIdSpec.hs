module Main (main) where

import AivoBlock.Block (Hash256, mkHash256, hash256Bytes)
import AivoBlock.ReceiptId (canonicalReceiptIdV1)
import qualified Data.ByteString as BS
import Data.Word (Word8)
import Test.Hspec

main :: IO ()
main = hspec $ do
  describe "Cross-language canonical receipt ID" $ do
    it "matches Rust's fixed 151-byte V1 test vector" $
      fmap toHex (canonicalReceiptIdV1 1 (hash 1) (hash 2) 0 2 (hash 3))
        `shouldBe` Right
          "1085d690f142951fb5fc32a9979d2b8d0afb2861fcc7cfae095d1f2cf9c80db1"
    it "binds chain, worker and result commitment" $ do
      let base = canonicalReceiptIdV1 1 (hash 1) (hash 2) 0 2 (hash 3)
      canonicalReceiptIdV1 2 (hash 1) (hash 2) 0 2 (hash 3)
        `shouldNotBe` base
      canonicalReceiptIdV1 1 (hash 1) (hash 4) 0 2 (hash 3)
        `shouldNotBe` base
      canonicalReceiptIdV1 1 (hash 1) (hash 2) 0 2 (hash 4)
        `shouldNotBe` base
    it "rejects zero chain and invalid ranges" $ do
      canonicalReceiptIdV1 0 (hash 1) (hash 2) 0 2 (hash 3)
        `shouldBe` Left "Chain ID must be positive"
      canonicalReceiptIdV1 1 (hash 1) (hash 2) 0 0 (hash 3)
        `shouldBe` Left "Receipt work range must be nonempty"
      canonicalReceiptIdV1 1 (hash 1) (hash 2) maxBound 2 (hash 3)
        `shouldBe` Left "Receipt work range overflow"

hash :: Word8 -> Hash256
hash n = case mkHash256 (BS.replicate 32 n) of
  Right x -> x
  Left err -> error err

toHex :: Hash256 -> String
toHex v = concatMap encodeByte (BS.unpack (hash256Bytes v))
  where
    encodeByte b = [digits !! fromIntegral (b `div` 16),
                    digits !! fromIntegral (b `mod` 16)]
    digits = "0123456789abcdef"
