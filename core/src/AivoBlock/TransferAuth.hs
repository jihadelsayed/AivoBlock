-- | Candidate V1 Ed25519 transfer authentication, including sender
-- ownership and domain-separated, replay-resistant signing bytes.
-- Consensus authorization and fork finality remain out of scope.
module AivoBlock.TransferAuth
  ( transferSigningBytesV1
  , verifySignedTransferV1
  ) where

import AivoBlock.Transaction
  ( Transaction(..), addressBytes, amountValue, validateTransactionShape )
import Crypto.Error (CryptoFailable(..))
import qualified Crypto.PubKey.Ed25519 as Ed25519
import Data.Bits (shiftR)
import qualified Data.ByteString as BS
import Data.Word (Word8, Word32, Word64)

domain :: BS.ByteString
domain = BS.pack (map (fromIntegral . fromEnum)
  "AIVOBLOCK-TRANSFER-V1") `BS.snoc` 0

be32 :: Word32 -> BS.ByteString
be32 n = BS.pack [fromIntegral (n `shiftR` shift)
  | shift <- [24, 16, 8, 0]]

be64 :: Word64 -> BS.ByteString
be64 n = BS.pack [fromIntegral (n `shiftR` shift)
  | shift <- [56, 48, 40, 32, 24, 16, 8, 0]]

-- | V1 only permits chain 1, version 1, and no reference extension.
-- The sender is exactly the 32-byte raw Ed25519 public key.
-- 118-byte signing preimage, never JSON or hex text.
transferSigningBytesV1 :: Transaction -> Either String BS.ByteString
transferSigningBytesV1 tx = do
  if txVersion tx /= 1 || txChainId tx /= 1 || txReference tx /= Nothing
    then Left "Unsupported transaction version, chain, or reference"
    else Right ()
  if amountValue (txAmount tx) == 0
    then Left "Transfer amount must be positive"
    else Right ()
  if BS.length (txPublicKey tx) /= 32
    then Left "Invalid Ed25519 public key length"
    else Right ()
  if addressBytes (txSender tx) /= txPublicKey tx
    then Left "Sender address does not match signing key"
    else Right ()
  pure (BS.concat
    [ domain
    , be32 1
    , be32 1
    , txPublicKey tx
    , addressBytes (txRecipient tx)
    , be64 (amountValue (txAmount tx))
    , be64 (amountValue (txFee tx))
    , be64 (txNonce tx)
    ])

verifySignedTransferV1 :: Transaction -> Either String ()
verifySignedTransferV1 tx = do
  validateTransactionShape tx
  bytes <- transferSigningBytesV1 tx
  if BS.length (txSignature tx) /= 64
    then Left "Invalid Ed25519 signature length"
    else Right ()
  key <- case Ed25519.publicKey (txPublicKey tx) of
    CryptoPassed result -> Right result
    CryptoFailed _ -> Left "Invalid Ed25519 public key"
  sig <- case Ed25519.signature (txSignature tx) of
    CryptoPassed result -> Right result
    CryptoFailed _ -> Left "Invalid Ed25519 signature"
  if Ed25519.verify key bytes sig
    then Right ()
    else Left "Invalid Ed25519 signature"
