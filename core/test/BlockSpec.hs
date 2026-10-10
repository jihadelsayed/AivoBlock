{-# LANGUAGE ScopedTypeVariables #-}

module Main (main) where

import AivoBlock.Block
import AivoBlock.Validation
import qualified Data.ByteString as BS
import Test.Hspec
import Test.QuickCheck (NonNegative(..), property)
import Data.Word (Word8)

main :: IO ()
main = hspec $ do
  describe "Hash256" $ do
    it "accepts exactly 32 bytes" $
      mkHash256 (BS.replicate 32 7) `shouldBe`
        Right (either (error "unexpected hash error") id
          (mkHash256 (BS.replicate 32 7)))

    it "rejects incorrect lengths" $ property $ \(NonNegative n) ->
      let size = n `mod` 100
      in size == 32 || case mkHash256 (BS.replicate size 0) of
           Left _  -> True
           Right _ -> False

    it "preserves all 32 bytes" $ property $ \(byte :: Word8) ->
      case mkHash256 (BS.replicate 32 byte) of
        Right h -> hash256Bytes h == BS.replicate 32 byte
        Left _ -> False

  describe "Block structure" $ do
    it "recognizes a genesis candidate, but not a child" $ do
      isGenesisCandidate (header 0 zeroHash) `shouldBe` True
      isGenesisCandidate (header 1 zeroHash) `shouldBe` False

    it "accepts a structurally valid genesis block" $
      validateBlockShape (Block (header 0 zeroHash) (BlockBody [] []))
        `shouldBe` Right ()

    it "rejects a genesis block with a nonzero parent" $
      validateBlockShape (Block (header 0 sampleHash) (BlockBody [] []))
        `shouldBe` Left InvalidGenesisParent

    it "rejects duplicate transaction references" $
      validateBlockShape (Block (header 0 zeroHash)
        (BlockBody [sampleHash, sampleHash] []))
        `shouldBe` Left DuplicateTransactionId

    it "rejects duplicate AI proof references" $
      validateBlockShape (Block (header 0 zeroHash)
        (BlockBody [] [sampleHash, sampleHash]))
        `shouldBe` Left DuplicatePuzzleProofId

    it "rejects a zero block version" $
      validateBlockShape (Block ((header 0 zeroHash) { headerVersion = 0 }) (BlockBody [] []))
        `shouldBe` Left InvalidBlockVersion

    it "rejects a zero chain ID" $
      validateBlockShape (Block ((header 0 zeroHash) { headerChainId = 0 }) (BlockBody [] []))
        `shouldBe` Left InvalidChainId

    it "rejects a non-genesis block with a zero parent" $
      validateBlockShape (Block (header 1 zeroHash) (BlockBody [] []))
        `shouldBe` Left InvalidParentHash

    it "accepts distinct transaction and proof IDs" $
      validateBlockShape (Block (header 0 zeroHash)
        (BlockBody [sampleHash, otherHash] [sampleHash, otherHash]))
        `shouldBe` Right ()

    it "rejects a child with a different chain ID" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) ((header 1 sampleHash) { headerChainId = 2 })
        `shouldBe` Left InvalidChainId

    it "rejects a child with a different protocol version" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) ((header 1 sampleHash) { headerVersion = 2 })
        `shouldBe` Left InvalidBlockVersion

    it "rejects a child timestamp that does not increase" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) ((header 1 sampleHash) { headerTimestampMs = 1000 })
        `shouldBe` Left NonIncreasingTimestamp

    it "rejects child height overflow" $
      validateChildHeader 1 1 sampleHash
        ((header 0 zeroHash) { headerHeight = maxBound })
        (header 1 sampleHash)
        `shouldBe` Left HeightOverflow

    it "accepts a correctly linked child header" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) (header 1 sampleHash)
        `shouldBe` Right ()

    it "rejects an incorrect parent commitment" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) (header 1 otherHash)
        `shouldBe` Left InvalidParentHash

    it "rejects a repeated block height" $
      validateChildHeader 1 1 sampleHash
        (header 0 zeroHash) (header 0 sampleHash)
        `shouldBe` Left InvalidHeight

sampleHash :: Hash256
sampleHash = hashOf 1

otherHash :: Hash256
otherHash = hashOf 2

hashOf :: Word8 -> Hash256
hashOf value =
  case mkHash256 (BS.replicate 32 value) of
    Right h -> h
    Left err -> error err

header :: Integer -> Hash256 -> BlockHeader
header heightValue parent =
  BlockHeader
    { headerVersion = 1
    , headerChainId = 1
    , headerHeight = fromInteger heightValue
    , headerParentHash = parent
    , headerTransactionsRoot = zeroHash
    , headerPuzzleProofsRoot = zeroHash
    , headerStateRoot = zeroHash
    , headerTimestampMs = fromInteger heightValue + 1000
    }
