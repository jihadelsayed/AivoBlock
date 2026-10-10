module Main (main) where

import AivoBlock.Block (Hash256, mkHash256)
import AivoBlock.Consensus
import qualified Data.ByteString as BS
import Data.Word (Word8, Word64)
import Test.Hspec
import Test.QuickCheck (Positive(..), property)

main :: IO ()
main = hspec $ do
  describe "ValidatorSet" $ do
    it "rejects an empty validator set" $
      mkValidatorSet [] `shouldBe` Left "Validator set must not be empty"

    it "rejects duplicate validators" $
      mkValidatorSet [(alice, 1), (alice, 2)]
        `shouldBe` Left "Duplicate validator ID"

    it "rejects zero voting power" $
      mkValidatorSet [(alice, 0)]
        `shouldBe` Left "Voting power must be positive"

    it "rejects voting-power overflow" $
      mkValidatorSet [(alice, maxBound), (bob, 1)]
        `shouldBe` Left "Total voting power exceeds Word64"

    it "uses a strict greater-than-two-thirds quorum" $ do
      let validators = set [(alice, 1), (bob, 1), (carol, 1)]
      totalVotingPower validators `shouldBe` 3
      requiredQuorumPower validators `shouldBe` 3

    it "calculates quorum for arbitrary positive weights" $
      property $ \(Positive n) ->
        let power = fromIntegral (1 + ((n :: Int) `mod` 100000)) 
            validators = set [(alice, power)]
        in requiredQuorumPower validators ==
             (2 * power) `div` 3 + 1

  describe "Vote tallying" $ do
    let validators = set [(alice, 3), (bob, 2), (carol, 1)]
        target = hashOf 10
        competing = hashOf 11
        round0 = Round 1 0
        yes voter = Vote voter round0 (VoteFor target)

    it "counts only votes for the target block" $
      tallyVotes validators round0 target
        [yes alice, Vote bob round0 (VoteFor competing), Vote carol round0 VoteNil]
        `shouldBe` Right 3

    it "accepts quorum with five out of six voting power" $
      hasQuorum validators round0 target [yes alice, yes bob]
        `shouldBe` Right True

    it "rejects a non-quorum result" $
      hasQuorum validators round0 target [yes alice, yes carol]
        `shouldBe` Right False

    it "rejects duplicate and conflicting votes" $
      tallyVotes validators round0 target [yes alice, Vote alice round0 VoteNil]
        `shouldBe` Left "Duplicate validator vote"

    it "rejects unknown validators" $
      tallyVotes validators round0 target [yes outsider]
        `shouldBe` Left "Unknown validator"

    it "rejects votes from another round" $
      tallyVotes validators round0 target
        [Vote alice (Round 1 1) (VoteFor target)]
        `shouldBe` Left "Vote belongs to another round"

-- Fixed fixtures are deliberately distinct identities.
alice, bob, carol, outsider :: ValidatorId
alice = validator 1
bob = validator 2
carol = validator 3
outsider = validator 4

validator :: Word8 -> ValidatorId
validator byte =
  case mkValidatorId (BS.replicate 32 byte) of
    Right value -> value
    Left err -> error err

hashOf :: Word8 -> Hash256
hashOf byte =
  case mkHash256 (BS.replicate 32 byte) of
    Right value -> value
    Left err -> error err

set :: [(ValidatorId, Word64)] -> ValidatorSet
set entries =
  case mkValidatorSet entries of
    Right value -> value
    Left err -> error err
