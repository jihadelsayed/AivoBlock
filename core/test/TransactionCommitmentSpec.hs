module Main (main) where

import AivoBlock.Block (Hash256, hash256Bytes, mkHash256)
import AivoBlock.Ledger (Account(..), Ledger(..))
import AivoBlock.Transaction (Transaction(..), mkAddress, mkAmount)
import AivoBlock.TransactionCommitment
  ( signedTransactionIdV1, transactionRootV1, ledgerStateRootV1 )
import qualified Data.ByteString as BS
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Char (digitToInt)
import Test.Hspec

main :: IO ()
main = hspec $ do
  describe "Canonical signed transaction IDs and Merkle roots" $ do
    it "matches Rust independent 32-byte transaction ID vector" $ do
      fmap toHex (signedTransactionIdV1 tx) `shouldBe`
        Right "a75cb64af25b97c4f7f06feb4a77827ba4d1159426eb45a9c99fded8dcb5b52d"
    it "matches Rust leaf and empty roots" $ do
      case signedTransactionIdV1 tx of
        Left problem -> expectationFailure problem
        Right identifier -> do
          fmap toHex (transactionRootV1 [identifier]) `shouldBe`
            Right "d0ea168dc83df1fd8bf95b46bf20d434d4bd2ae12b1a9ce7cf77cadac19283c3"
          fmap toHex (transactionRootV1 []) `shouldBe`
            Right "4cd0e95f1262d79dd9a479ac1cd9471a64552f5cf44a40ce4d8249b8d9f7f385"
          transactionRootV1 [identifier,identifier]
            `shouldBe` Left "Duplicate transaction ID"
    it "matches Rust even- and odd-leaf Merkle vectors" $ do
      let h n = case mkHash256 (BS.replicate 32 n) of
            Right value -> value
            Left err -> error err
      fmap toHex (transactionRootV1 [h 1,h 2]) `shouldBe`
        Right "59cdd667d6549503267625933df3bfa85016948f59f8bb91af0b945f926a6477"
      fmap toHex (transactionRootV1 [h 1,h 2,h 3]) `shouldBe`
        Right "0dbe0e823658b8f97440d5b79bc1b1aa7b0943ab43a1e9e930f1b4c881f2e822"
      transactionRootV1 [h 1,h 2,h 3] `shouldNotBe`
        transactionRootV1 [h 1,h 3,h 2]
    it "rejects signed envelope tampering" $ do
      signedTransactionIdV1 (tx {txFee=mkAmount 3}) `shouldSatisfy` isLeft
      signedTransactionIdV1 (tx {txNonce=1}) `shouldSatisfy` isLeft
      signedTransactionIdV1 (tx {txSignature=BS.replicate 64 0})
        `shouldSatisfy` isLeft
    it "commits to the exact typed ledger state" $ do
      let base = Ledger (Map.singleton (txSender tx) (Account 100 0))
            0 Set.empty Set.empty
          altered = base {ledgerFeePool=1}
      ledgerStateRootV1 base `shouldSatisfy` isRight
      ledgerStateRootV1 base `shouldNotBe` ledgerStateRootV1 altered
      fmap toHex (ledgerStateRootV1 base) `shouldBe`
        Right "13d9ce9abec0d2ac1900bf3b53336956a01c6122768634da63f4a0233ab847a9"

isRight :: Either String a -> Bool
isRight (Right _) = True
isRight _ = False

isLeft :: Either String a -> Bool
isLeft (Left _) = True
isLeft _ = False

toHex :: Hash256 -> String
toHex value = concatMap (\b -> let digits = "0123456789abcdef"
                                in [digits !! fromIntegral (b `div` 16),
                                    digits !! fromIntegral (b `mod` 16)])
                 (BS.unpack (hash256Bytes value))

fromHex :: String -> BS.ByteString
fromHex (a:b:more) = fromIntegral (16*digitToInt a + digitToInt b)
  `BS.cons` fromHex more
fromHex [] = BS.empty
fromHex _ = error "Odd hex literal"

pub :: BS.ByteString
pub = fromHex "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"

tx :: Transaction
tx = Transaction
  { txVersion=1, txChainId=1,
    txSender=address pub, txRecipient=address (BS.replicate 32 2),
    txAmount=mkAmount 10, txFee=mkAmount 2, txNonce=0,
    txReference=Nothing, txPublicKey=pub,
    txSignature=fromHex
      "f1cc31b91b3485d813bbe245ee742795eb94dcd7cfdba90d30d1a4da9ec18ca9822a6ef0a36c54a58a956de172eee42816ec51b2a0dfbcff7422d62deb952405"
  }
  where
    address raw = case mkAddress raw of
      Left problem -> error problem
      Right result -> result
