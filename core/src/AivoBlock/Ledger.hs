-- | Deterministic account-ledger state transitions for AivoBlock.
--
-- IMPORTANT: This module does NOT verify cryptographic signatures.
-- A caller must first verify transaction signatures, chain ID, protocol
-- version, and canonical transaction encoding before applying a transfer.
module AivoBlock.Ledger
  ( Account(..)
  , Ledger(..)
  , emptyLedger
  , balanceOf
  , nonceOf
  , applyVerifiedTransfer
  ) where

import AivoBlock.Block (Hash256)
import AivoBlock.Transaction
  ( Address, Amount, Transaction(..), amountValue )
import Data.Map.Strict (Map)
import qualified Data.Map.Strict as Map
import Data.Set (Set)
import qualified Data.Set as Set
import Data.Word (Word64)

-- | Balances are stored in the smallest indivisible token unit.
data Account = Account
  { accountBalance :: !Word64
  , accountNonce   :: !Word64
  } deriving (Eq, Show)

-- | Fee pool retains transfer fees until rewards are distributed.
-- Supply creation and genesis allocation are separate protocol operations.
data Ledger = Ledger
  { ledgerAccounts :: !(Map Address Account)
  , ledgerFeePool  :: !Word64
  , ledgerSettledRewardBatches :: !(Set Hash256)
  , ledgerPaidWorkReceipts :: !(Set Hash256)
  } deriving (Eq, Show)

emptyLedger :: Ledger
emptyLedger = Ledger Map.empty 0 Set.empty Set.empty

lookupAccount :: Address -> Ledger -> Account
lookupAccount address ledger =
  Map.findWithDefault (Account 0 0) address (ledgerAccounts ledger)

balanceOf :: Address -> Ledger -> Word64
balanceOf address ledger = accountBalance (lookupAccount address ledger)

nonceOf :: Address -> Ledger -> Word64
nonceOf address ledger = accountNonce (lookupAccount address ledger)

checkedAdd :: Word64 -> Word64 -> Either String Word64
checkedAdd x y
  | y > maxBound - x = Left "Unsigned 64-bit overflow"
  | otherwise = Right (x + y)

-- | Apply an already-authenticated transfer using pure ledger rules.
-- Nonces must match the current account nonce, starting from zero.
-- Fees accumulate in the ledger's fee pool rather than disappearing.
-- Even a self-transfer pays its fee and advances its nonce.
applyVerifiedTransfer :: Transaction -> Ledger -> Either String Ledger
applyVerifiedTransfer tx ledger = do
  let sender = txSender tx
      recipient = txRecipient tx
      amount = amountValue (txAmount tx)
      fee = amountValue (txFee tx)
      senderAccount = lookupAccount sender ledger
      currentNonce = accountNonce senderAccount

  if amount == 0
    then Left "Transfer amount must be positive"
    else Right ()
  if txNonce tx /= currentNonce
    then Left "Invalid account nonce"
    else Right ()
  if currentNonce == maxBound
    then Left "Account nonce exhausted"
    else Right ()

  debit <- checkedAdd amount fee
  if accountBalance senderAccount < debit
    then Left "Insufficient funds"
    else Right ()

  newFeePool <- checkedAdd (ledgerFeePool ledger) fee
  let afterDebit = senderAccount
        { accountBalance = accountBalance senderAccount - debit
        , accountNonce = currentNonce + 1
        }
      debitedAccounts = Map.insert sender afterDebit (ledgerAccounts ledger)
      recipientAccount = Map.findWithDefault (Account 0 0) recipient debitedAccounts
  recipientBalance <- checkedAdd (accountBalance recipientAccount) amount
  let creditedAccounts = Map.insert recipient
        (recipientAccount { accountBalance = recipientBalance })
        debitedAccounts
  pure Ledger
    { ledgerAccounts = creditedAccounts
    , ledgerFeePool = newFeePool
    , ledgerSettledRewardBatches = ledgerSettledRewardBatches ledger
    , ledgerPaidWorkReceipts = ledgerPaidWorkReceipts ledger
    }
