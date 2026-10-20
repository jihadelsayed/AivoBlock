-- | Pure atomic, ordered batch execution for signed local transfers.
-- Failed transactions discard the entire computed successor; no mutation.
module AivoBlock.BatchExecution
  ( applySignedBatchV1
  ) where

import AivoBlock.Block (Hash256)
import AivoBlock.Ledger (Ledger, applyVerifiedTransfer)
import AivoBlock.Transaction (Transaction)
import AivoBlock.TransactionCommitment
  (signedTransactionIdV1, transactionRootV1, ledgerStateRootV1)
import Control.Monad (foldM)
import qualified Data.Set as Set

-- | Candidate batch limit for the local framed IPC, not a network rule.
-- Verify each signed envelope and distinct ID before executing any transfer.
-- Execute sequentially against the preceding transaction's ledger.
applySignedBatchV1
  :: Ledger -> [Transaction]
  -> Either String (Ledger, [Hash256], Hash256, Hash256)
applySignedBatchV1 prior transactions
  | null transactions = Left "Empty transaction batch"
  | length transactions > 64 = Left "Transaction batch exceeds 64"
  | otherwise = do
      identifiers <- traverse signedTransactionIdV1 transactions
      if Set.size (Set.fromList identifiers) /= length identifiers
        then Left "Duplicate transaction ID"
        else Right ()
      root <- transactionRootV1 identifiers
      updated <- foldM (flip applyVerifiedTransfer) prior transactions
      stateRoot <- ledgerStateRootV1 updated
      pure (updated, identifiers, root, stateRoot)
