-- | Transaction data types for the AivoBlock prototype.
--
-- This module describes transfers of the native ledger asset (Sayed Token).
-- It does not introduce a separate cryptocurrency or integrate another chain.
-- Signature verification, canonical transaction IDs, nonce enforcement,
-- available-balance checks, and fee rules belong to protocol validation.
module AivoBlock.Transaction
  ( Address
  , addressLength
  , mkAddress
  , addressBytes
  , Amount
  , mkAmount
  , amountValue
  , Transaction(..)
  , validateTransactionShape
  ) where

import AivoBlock.Block (Hash256)
import Data.ByteString (ByteString)
import qualified Data.ByteString as BS
import Data.Word (Word64)

-- | Prototype addresses are 32-byte account identifiers.
-- The final address derivation and human-readable format remain unspecified.
newtype Address = Address ByteString
  deriving (Eq, Ord, Show)

addressLength :: Int
addressLength = 32

mkAddress :: ByteString -> Either String Address
mkAddress bytes
  | BS.length bytes == addressLength = Right (Address bytes)
  | otherwise = Left ("Expected 32 bytes for Address, got "
                       ++ show (BS.length bytes))

addressBytes :: Address -> ByteString
addressBytes (Address bytes) = bytes

-- | Unsigned, smallest-unit integer amount; never floating point.
newtype Amount = Amount Word64
  deriving (Eq, Ord, Show)

mkAmount :: Word64 -> Amount
mkAmount = Amount

amountValue :: Amount -> Word64
amountValue (Amount value) = value

-- | A signed transfer envelope. Signature and public-key algorithms
-- must be specified before this type is accepted by consensus.
--
-- 'txReference' is an optional application-level commitment and does
-- not substitute for the canonical transaction ID.
data Transaction = Transaction
  { txVersion   :: !Word64
  , txChainId   :: !Word64
  , txSender    :: !Address
  , txRecipient :: !Address
  , txAmount    :: !Amount
  , txFee       :: !Amount
  , txNonce     :: !Word64
  , txReference :: !(Maybe Hash256)
  , txPublicKey :: !ByteString
  , txSignature :: !ByteString
  } deriving (Eq, Show)

-- | Basic input shape validation only: no ledger or crypto verification.
-- These checks must not be treated as sufficient to accept a transaction.
validateTransactionShape :: Transaction -> Either String ()
validateTransactionShape tx
  | amountValue (txAmount tx) == 0 = Left "Transfer amount must be positive"
  | txVersion tx == 0 = Left "Transaction version must be nonzero"
  | txChainId tx == 0 = Left "Chain ID must be nonzero"
  | BS.null (txPublicKey tx) = Left "Missing public key"
  | BS.null (txSignature tx) = Left "Missing signature"
  | otherwise = Right ()
