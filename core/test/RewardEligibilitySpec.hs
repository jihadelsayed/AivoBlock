module Main (main) where

import AivoBlock.Block (Hash256, mkHash256)
import AivoBlock.PuzzleProof
import AivoBlock.RewardEligibility
import AivoBlock.Rewards
import qualified Data.ByteString as BS
import Data.Word (Word8, Word64)
import Test.Hspec

main :: IO ()
main = hspec $ do
  describe "Reward eligibility (trusted inputs only)" $ do
    it "turns non-overlapping verified work into deterministic weights" $ do
      eligibleContributions 10 [work 2 4 2 2 Verified, work 1 0 2 1 Verified,
                                work 1 2 1 1 Verified]
        `shouldBe` Right [Contribution (hash 1) 3, Contribution (hash 2) 2]

    it "rejects unverified results" $
      eligibleContributions 10 [work 1 0 1 1 PendingVerification]
        `shouldBe` Left "Proof not independently verified"

    it "rejects rejected results" $
      eligibleContributions 10 [work 1 0 1 1 Rejected]
        `shouldBe` Left "Proof not independently verified"

    it "rejects overlapping ranges and exact replay" $ do
      eligibleContributions 10 [work 1 0 2 1 Verified, work 2 1 2 1 Verified]
        `shouldBe` Left "Duplicate or overlapping verified work"
      eligibleContributions 10 [work 1 0 1 1 Verified, work 1 0 1 1 Verified]
        `shouldBe` Left "Duplicate or overlapping verified work"

    it "rejects out-of-range work" $
      eligibleContributions 10 [work 1 4 2 1 Verified]
        `shouldBe` Left "Proof extends beyond the puzzle range"

    it "rejects expired work" $
      eligibleContributions 101 [work 1 0 1 1 Verified]
        `shouldBe` Left "Puzzle deadline exceeded"

    it "accepts a proof exactly at the deadline" $
      eligibleContributions 100 [work 1 0 1 1 Verified]
        `shouldBe` Right [Contribution (hash 1) 1]

    it "does not combine overlapping ranges from distinct puzzles" $
      eligibleContributions 10 [work 1 0 2 1 Verified, work 1 0 2 2 Verified]
        `shouldBe` Right [Contribution (hash 1) 4]

    it "rejects conflicting definitions for one puzzle" $
      let first = work 1 0 1 1 Verified
          second = (work 2 1 1 1 Verified)
            { workPuzzle = (workPuzzle (work 2 1 1 1 Verified))
                { puzzleDeadlineHeight = 99 } }
      in eligibleContributions 10 [first, second]
           `shouldBe` Left "Conflicting definitions for the same puzzle"

hash :: Word8 -> Hash256
hash byte = case mkHash256 (BS.replicate 32 byte) of
  Right h -> h
  Left err -> error err

work :: Word8 -> Word64 -> Word64 -> Word8 -> ProofStatus -> VerifiedWork
work worker start count puzzleNumber status =
  let puzzle = Puzzle
        { puzzleId = hash puzzleNumber
        , puzzleVersion = 1
        , puzzleModelCommitment = hash 10
        , puzzleInputCommitment = hash 11
        , puzzleRulesCommitment = hash 12
        , puzzleUnits = 5
        , puzzleDeadlineHeight = 100
        }
      proof = PuzzleProof
        { proofPuzzleId = hash puzzleNumber
        , proofWorkerId = hash worker
        , proofStartUnit = start
        , proofUnitCount = count
        , proofResultCommitment = hash 13
        , proofWitnessCommitment = Nothing
        }
  in VerifiedWork puzzle proof status
