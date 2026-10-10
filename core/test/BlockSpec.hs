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

  describe "Canonical V1 block-header encoding" $ do
    it "encodes the published genesis fixture as exactly 178 bytes" $ do
      let encoded = encodeBlockHeaderV1 (header 0 zeroHash)
      fmap BS.length encoded `shouldBe` Right 178

    it "matches the specified 178-byte preimage field by field" $ do
      let domain = BS.pack (map (fromIntegral . fromEnum) "AIVOBLOCK-BLOCK-HEADER-V1") 
          expected = BS.concat
            [ domain, BS.singleton 0
            , BS.pack [0,0,0,1]    -- version, uint32 big-endian
            , BS.pack [0,0,0,1]    -- chain ID, uint32 big-endian
            , BS.replicate 8 0     -- genesis height, uint64 big-endian
            , BS.replicate 128 0   -- four raw 32-byte commitments
            , BS.pack [0,0,0,0,0,0,3,232] -- timestamp 1000
            ]
      BS.length expected `shouldBe` 178
      encodeBlockHeaderV1 (header 0 zeroHash) `shouldBe` Right expected

    it "matches the published SHA-256 genesis header vector" $ do
      let expected = "c1b8e653731eeafeacb557e46669d277096ecc7ceff7ec911ab29b25a8a55c22"
      fmap (hexBytes . hash256Bytes) (hashBlockHeaderV1 (header 0 zeroHash))
        `shouldBe` Right expected

    it "changes the SHA-256 identifier when the header changes" $ do
      let original = header 0 zeroHash
      hashBlockHeaderV1 original
        `shouldNotBe` hashBlockHeaderV1 (original { headerTimestampMs = 1001 })

    it "does not hash an unsupported V1 header" $
      hashBlockHeaderV1 ((header 0 zeroHash) { headerVersion = 2 })
        `shouldBe` Left "Unsupported V1 header version"

    it "uses raw hash bytes rather than hexadecimal strings" $ do
      let encoded = encodeBlockHeaderV1 (header 1 sampleHash)
      fmap (BS.take 32 . BS.drop 42) encoded
        `shouldBe` Right (BS.replicate 32 1)

    it "changes the encoded bytes when a field changes" $ do
      let original = header 0 zeroHash
      encodeBlockHeaderV1 original
        `shouldNotBe` encodeBlockHeaderV1 (original { headerTimestampMs = 1001 })

    it "rejects unsupported header versions" $
      encodeBlockHeaderV1 ((header 0 zeroHash) { headerVersion = 2 })
        `shouldBe` Left "Unsupported V1 header version"

    it "rejects chain ID zero" $
      encodeBlockHeaderV1 ((header 0 zeroHash) { headerChainId = 0 })
        `shouldBe` Left "Chain ID must not be zero"

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

-- | Lowercase hexadecimal, for comparing against the published digest.
hexBytes :: BS.ByteString -> String
hexBytes = concatMap toHex . BS.unpack
  where
    alphabet = "0123456789abcdef"
    toHex b = [alphabet !! fromIntegral (b `div` 16), alphabet !! fromIntegral (b `mod` 16)]
