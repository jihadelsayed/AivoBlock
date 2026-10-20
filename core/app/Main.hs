{-# LANGUAGE OverloadedStrings #-}

-- | Local diagnostics and a bounded IPC health endpoint for the Rust node.
-- A fixture-based block shape IPC check is available; full validation
-- and network consensus are not implemented.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (when)
import AivoBlock.Block (Block(..), BlockBody(..), BlockHeader(..), zeroHash, hashBlockHeaderV1)
import AivoBlock.Validation (validateBlockShape, validateChildHeader)
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as BS
import AivoBlock.Block (mkHash256, hash256Bytes, Hash256)
import AivoBlock.PuzzleProof (Puzzle(..), PuzzleProof(..), ProofStatus(..))
import AivoBlock.RewardEligibility (VerifiedWork(..), eligibleContributions)
import AivoBlock.Rewards (Contribution(..))
import AivoBlock.ReceiptId (canonicalReceiptIdV1)
import AivoBlock.Settlement (RewardEntitlement(..), settleFundedRewards)
import AivoBlock.Issuance (ledgerSupply, mkIssuancePolicy, settleIssuedRewards)
import AivoBlock.Ledger (Ledger(..), Account(..), applyVerifiedTransfer)
import AivoBlock.LocalLedgerSnapshot (decodeLedgerSnapshot, encodeLedgerSnapshot)
import AivoBlock.TransferAuth (verifySignedTransferV1)
import AivoBlock.TransactionCommitment (signedTransactionIdV1, transactionRootV1, ledgerStateRootV1)
import AivoBlock.BatchExecution (applySignedBatchV1)
import AivoBlock.Transaction (Transaction(..), mkAddress, addressBytes, mkAmount)
import qualified Data.Map.Strict as Map
import qualified Data.Set as Set
import Data.Char (chr, ord)
import Data.Aeson (Value, eitherDecodeStrict', encode, object, (.=), (.:), (.:?), withObject)
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isHexDigit)
import Numeric (showHex)
import Data.Word (Word32, Word64)

import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, hSetBinaryMode, stderr, stdin, stdout)

version :: String
version = "0.1.0.0"

maxIpcBytes :: Integer
maxIpcBytes = 4 * 1024 * 1024

healthResponse :: String
healthResponse =
  "{\"ok\":true,\"type\":\"health\",\"protocol_version\":1,\"core\":\"aivoblock-core\",\"status\":\"ready_for_protocol_development\"}"

data Command = Help | Version | Status | IpcHealth

parseCommand :: [String] -> Either String Command
parseCommand []             = Right Status
parseCommand ["status"]     = Right Status
parseCommand ["ipc-health"] = Right IpcHealth
parseCommand ["--help"]     = Right Help
parseCommand ["-h"]         = Right Help
parseCommand ["--version"]  = Right Version
parseCommand ["-v"]         = Right Version
parseCommand _              = Left "Unknown command or arguments. Use --help."

printHelp :: IO ()
printHelp = mapM_ putStrLn
  [ "AivoBlock Core " ++ version
  , ""
  , "Usage: aivoblock-core [COMMAND]"
  , ""
  , "Commands:"
  , "  status       Show the current implementation status (default)"
  , "  ipc-health   Exchange one length-prefixed health or block-shape request"
  , "  --version    Print the core version"
  , "  --help       Show this help message"
  ]

printStatus :: IO ()
printStatus = mapM_ putStrLn
  [ "AivoBlock Core " ++ version
  , "Component: Haskell protocol and ledger validation"
  , "State: Initial development"
  , "Node networking: Rust (separate component)"
  , "AI puzzle execution: Python (separate component)"
  , "No blockchain node is started by this command."
  ]

-- | Read one bounded binary frame with the shared Rust framing contract.
-- Short reads, zero length, oversized frames and trailing bytes are rejected.
readFrame :: IO (Either String String)
readFrame = do
  header <- readExactly 4
  if BS.length header /= 4
    then pure (Left "Incomplete IPC header")
    else do
      let count = BS.foldl' (\n byte -> (n `shiftL` 8) + toInteger byte) 0 header
      if count <= 0 || count > maxIpcBytes
        then pure (Left "Invalid IPC payload length")
        else do
          payload <- readExactly (fromInteger count)
          if toInteger (BS.length payload) /= count
            then pure (Left "Incomplete IPC payload")
            else do
              extra <- BS.hGet stdin 1
              pure $ if BS.null extra
                then Right (map (chr . fromIntegral) (BS.unpack payload))
                else Left "Unexpected trailing IPC bytes"

readExactly :: Int -> IO BS.ByteString
readExactly amount = go amount []
  where
    go remaining chunks
      | remaining <= 0 = pure (BS.concat (reverse chunks))
      | otherwise = do
          chunk <- BS.hGetSome stdin remaining
          if BS.null chunk
            then pure (BS.concat (reverse chunks))
            else go (remaining - BS.length chunk) (chunk : chunks)

writeFrame :: String -> IO ()
writeFrame body = do
  let payload = BS.pack (map (fromIntegral . ord) body)
      n = BS.length payload
      header = BS.pack [fromIntegral ((n `shiftR` s) .&. 255) | s <- [24,16,8,0]]
  BS.hPut stdout header
  BS.hPut stdout payload

-- | Deliberately fixed local fixtures until canonical JSON/Protobuf decoding
-- and authenticated block commitments are defined. The result is a real
-- call to validateBlockShape, but only for these predetermined candidates.
fixtureBlock :: Bool -> Block
fixtureBlock validGenesis =
  let nonzeroHash = case mkHash256 (BS.replicate 32 1) of
        Right value -> value
        Left err -> error err
      header = BlockHeader
        { headerVersion = 1
        , headerChainId = 1
        , headerHeight = 0
        , headerParentHash = if validGenesis then zeroHash else nonzeroHash
        , headerTransactionsRoot = zeroHash
        , headerPuzzleProofsRoot = zeroHash
        , headerStateRoot = zeroHash
        , headerTimestampMs = 0
        }
  in Block header (BlockBody [] [])

shapeResponse :: Bool -> String
shapeResponse validGenesis =
  let verdict = validateBlockShape (fixtureBlock validGenesis)
      valid = either (const False) (const True) verdict
      reason = either show (const "none") verdict
  in map (chr . fromIntegral) . BS.unpack $ BL.toStrict (encode (object
       [ "ok" .= True
       , "type" .= ("validate_block_shape" :: String)
       , "protocol_version" .= (1 :: Int)
       , "valid" .= valid
       , "reason" .= reason
       , "scope" .= ("structural_fixture_only" :: String)
       ]))

-- This endpoint checks structure only, not signatures, roots, finality or AI proofs.
parseHash :: String -> Parser Hash256
parseHash hex
  | length hex /= 64 || not (all isHexDigit hex) = fail "Expected 64 hex digits"
  | otherwise = either fail pure (mkHash256 (BS.pack (decodeHex hex)))
  where
    decodeHex (a:b:rest) = fromIntegral (16 * digitToInt a + digitToInt b) : decodeHex rest
    decodeHex _ = []

parseBlock :: Value -> Parser Block
parseBlock = withObject "block" $ \o -> do
  h <- o .: "header"
  b <- o .: "body"
  header <- withObject "header" (\v -> do
    ver <- v .: "version" :: Parser Word32
    chain <- v .: "chain_id" :: Parser Word32
    height <- v .: "height" :: Parser Word64
    parent <- v .: "parent_hash" >>= parseHash
    txRoot <- v .: "transactions_root" >>= parseHash
    proofRoot <- v .: "puzzle_proofs_root" >>= parseHash
    stateRoot <- v .: "state_root" >>= parseHash
    timestamp <- v .: "timestamp_ms" :: Parser Word64
    pure (BlockHeader ver chain height parent txRoot proofRoot stateRoot timestamp)) h
  body <- withObject "body" (\v -> do
    tx <- v .: "transaction_ids" :: Parser [String]
    proofs <- v .: "puzzle_proof_ids" :: Parser [String]
    when (length tx > 10000 || length proofs > 10000) (fail "Too many block IDs")
    ids <- traverse parseHash tx
    proofIds <- traverse parseHash proofs
    pure (BlockBody ids proofIds)) b
  pure (Block header body)

jsonResponse :: Bool -> String -> BS.ByteString
jsonResponse valid reason = BL.toStrict (encode (object
  [ "ok" .= True
  , "type" .= ("validate_block_shape" :: String)
  , "protocol_version" .= (1 :: Int)
  , "valid" .= valid
  , "reason" .= reason
  , "scope" .= ("structural_only" :: String)
  ]))

dynamicBlockResponse :: Value -> BS.ByteString
dynamicBlockResponse value =
  case parseEither parseBlock value of
    Left _ -> BL.toStrict (encode (object ["ok" .= False, "error" .= ("invalid_block" :: String)]))
    Right block ->
      let result = validateBlockShape block
      in jsonResponse (either (const False) (const True) result)
          (either show (const "none") result)

dynamicRequest :: String -> Maybe BS.ByteString
dynamicRequest raw =
  case eitherDecodeStrict' (BS.pack (map (fromIntegral . ord) raw)) :: Either String Value of
    Left _ -> Nothing
    Right value ->
      case parseEither parseIpcBlock value of
        Right (Just block) -> Just (dynamicBlockResponse block)
        _ -> Nothing
  where
    parseIpcBlock = withObject "request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      block <- o .:? "block" :: Parser (Maybe Value)
      if protocol == 1 && kind == "validate_block_shape"
        then pure block
        else pure Nothing


-- | Validate two supplied blocks' shapes, derive the parent's SHA-256
-- header ID locally, then check the child's chain, version, ancestry,
-- height and timestamp. The parent must already be trusted by the caller.
-- This does not verify Merkle roots, signatures, AI proofs or consensus.
childHeaderResponse :: Value -> BS.ByteString
childHeaderResponse value =
  case parseEither parsePair value of
    Left _ -> failure "invalid_parent_or_child"
    Right (parent, child) ->
      let parentHeader = blockHeader parent
          childHeader = blockHeader child
          verdict = do
            either (Left . show) Right (validateBlockShape parent)
            either (Left . show) Right (validateBlockShape child)
            parentHash <- either (Left . const "invalid_parent_header") Right
              (hashBlockHeaderV1 parentHeader)
            either (Left . show) Right
              (validateChildHeader
                (headerChainId parentHeader)
                (headerVersion parentHeader)
                parentHash parentHeader childHeader)
      in case verdict of
        Left reason -> decision False reason
        Right () -> decision True "none"
  where
    parsePair = withObject "validate child request" $ \o -> do
      parentValue <- o .: "parent"
      childValue <- o .: "child"
      parent <- parseBlock parentValue
      child <- parseBlock childValue
      pure (parent, child)
    failure reason = BL.toStrict (encode (object
      [ "ok" .= False, "error" .= (reason :: String) ]))
    decision valid reason = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("validate_child_header" :: String)
      , "protocol_version" .= (1 :: Int)
      , "valid" .= valid
      , "reason" .= (reason :: String)
      , "scope" .= ("structural_parent_link_only" :: String)
      ]))

childRequest :: Value -> Maybe BS.ByteString
childRequest value =
  case parseEither parseKind value of
    Right True -> Just (childHeaderResponse value)
    _ -> Nothing
  where
    parseKind = withObject "request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (protocol == 1 && kind == "validate_child_header")

-- | This framed endpoint screens caller-asserted verification results.
-- The Rust caller MUST authenticate and independently verify each receipt.
-- Passing "verified":true is not cryptographic proof or payment authority.
parseRewardWork :: Value -> Parser VerifiedWork
parseRewardWork = withObject "verified work" $ \o -> do
  puzzleIdHex <- o .: "puzzle_id" :: Parser String
  workerHex <- o .: "worker_id" :: Parser String
  resultHex <- o .: "result_commitment" :: Parser String
  puzzleIdValue <- parseHash puzzleIdHex
  worker <- parseHash workerHex
  resultHash <- parseHash resultHex
  units <- o .: "puzzle_units" :: Parser Word64
  deadline <- o .: "deadline_height" :: Parser Word64
  start <- o .: "start" :: Parser Word64
  count <- o .: "count" :: Parser Word64
  verified <- o .: "verified" :: Parser Bool
  let puzzle = Puzzle puzzleIdValue 1 zeroHash zeroHash zeroHash units deadline
      proof = PuzzleProof puzzleIdValue worker start count resultHash Nothing
  pure (VerifiedWork puzzle proof (if verified then Verified else Rejected))

hexHash :: Hash256 -> String
hexHash value = concatMap byteHex (BS.unpack (hash256Bytes value))
  where
    byteHex byte = let digits = showHex byte "" in
      if length digits == 1 then '0' : digits else digits

rewardEligibilityResponse :: Value -> BS.ByteString
rewardEligibilityResponse value =
  case parseEither parser value of
    Left _ -> response False "invalid_reward_eligibility_request" []
    Right (height, works) ->
      case eligibleContributions height works of
        Left err -> response False err []
        Right entries -> response True "none" entries
  where
    parser = withObject "reward eligibility request" $ \o -> do
      height <- o .: "height" :: Parser Word64
      workValues <- o .: "works" :: Parser [Value]
      when (length workValues > 1024) (fail "Too many work receipts")
      works <- traverse parseRewardWork workValues
      pure (height, works)
    response :: Bool -> String -> [Contribution] -> BS.ByteString
    response eligible reason entries = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("reward_eligibility" :: String)
      , "protocol_version" .= (1 :: Int)
      , "eligible" .= eligible
      , "reason" .= reason
      , "scope" .= ("caller_attested_verification_only" :: String)
      , "contributions" .=
          [ object [ "worker_id" .= hexHash (contributionWorker entry)
                   , "units" .= contributionWeight entry ]
          | entry <- entries ]
      ]))

rewardRequest :: Value -> Maybe BS.ByteString
rewardRequest value =
  case parseEither parseKind value of
    Right True -> Just (rewardEligibilityResponse value)
    _ -> Nothing
  where
    parseKind = withObject "request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (protocol == 1 && kind == "reward_eligibility")

-- | An isolated, explicitly unfunded-by-chain fixture for settlement testing.
-- The caller supplies the synthetic fee pool and attests every verification.
-- Never treat this endpoint as an authorization to pay or mint real tokens.
settlementFixtureResponse :: Value -> BS.ByteString
settlementFixtureResponse value =
  case parseEither parser value of
    Left _ -> rejected "invalid_settlement_fixture_request"
    Right (chainId, height, feePool, budget, batchId, claimed, works) ->
      let verdict = do
            _ <- eligibleContributions height works
            receiptIds <- traverse receiptIdFor works
            if receiptIds /= claimed
              then Left "Canonical receipt ID mismatch"
              else Right ()
            entitlements <- traverse makeEntitlement (zip works receiptIds)
            let initial = Ledger Map.empty feePool Set.empty Set.empty
            settled <- settleFundedRewards batchId budget entitlements initial
            pure (settled, receiptIds)
          receiptIdFor work =
            let proof = workProof work in
            canonicalReceiptIdV1 chainId (proofPuzzleId proof)
              (proofWorkerId proof) (proofStartUnit proof)
              (proofUnitCount proof) (proofResultCommitment proof)
          makeEntitlement (work, receiptId) = do
            let proof = workProof work
            address <- mkAddress (hash256Bytes (proofWorkerId proof))
            pure (RewardEntitlement receiptId (proofWorkerId proof)
              address (proofUnitCount proof))
      in case verdict of
        Left reason -> rejected reason
        Right (settled, receiptIds) ->
          BL.toStrict (encode (object
            [ "ok" .= True
            , "type" .= ("settlement_fixture" :: String)
            , "protocol_version" .= (1 :: Int)
            , "settled" .= True
            , "scope" .= ("caller_attested_synthetic_fee_pool_only" :: String)
            , "fee_pool_remaining" .= ledgerFeePool settled
            , "paid_receipt_ids" .= map hexHash receiptIds
            , "credits" .=
                [ object [ "recipient" .= hexBytes (addressBytes address)
                         , "amount" .= accountBalance account ]
                | (address, account) <- Map.toAscList (ledgerAccounts settled) ]
            ]))
  where
    parser = withObject "settlement fixture" $ \o -> do
      chainId <- o .: "chain_id" :: Parser Word32
      height <- o .: "height" :: Parser Word64
      feePool <- o .: "fee_pool" :: Parser Word64
      budget <- o .: "budget" :: Parser Word64
      batchIdHex <- o .: "batch_id" :: Parser String
      batchId <- parseHash batchIdHex
      values <- o .: "works" :: Parser [Value]
      when (null values || length values > 1024)
        (fail "Expected 1 to 1024 verified receipts")
      claims <- traverse parseClaimedId values
      works <- traverse parseRewardWork values
      pure (chainId, height, feePool, budget, batchId, claims, works)
    parseClaimedId = withObject "receipt claim" $ \o -> do
      hex <- o .: "receipt_id" :: Parser String
      parseHash hex
    rejected :: String -> BS.ByteString
    rejected reason = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("settlement_fixture" :: String)
      , "protocol_version" .= (1 :: Int)
      , "settled" .= False
      , "reason" .= reason
      , "scope" .= ("caller_attested_synthetic_fee_pool_only" :: String)
      ]))

hexBytes :: BS.ByteString -> String
hexBytes bytes = concatMap byteHex (BS.unpack bytes)
  where
    byteHex byte = let digits = showHex byte "" in
      if length digits == 1 then '0' : digits else digits

-- | One fee-funded local reward transition starting from an actual typed
-- ledger snapshot, NOT from a caller-invented fee-pool balance. The Rust
-- caller must have independently authenticated assignments and recomputed
-- the AI work before it can attest "verified":true. Haskell recomputes IDs,
-- eligibility and funded payout, never mints a subsidy.
ledgerRewardBlockResponse :: Value -> BS.ByteString
ledgerRewardBlockResponse value =
  case parseEither parser value of
    Left _ -> reply False "invalid_reward_block_request" Nothing
    Right (chainId, height, snapshot, budget, batchId, claimed, works) ->
      let outcome = do
            prior <- decodeLedgerSnapshot snapshot
            _ <- eligibleContributions height works
            actual <- traverse (receiptIdFor chainId) works
            when (actual /= claimed) (Left "Canonical receipt ID mismatch")
            entitlements <- traverse makeEntitlement (zip works actual)
            next <- settleFundedRewards batchId budget entitlements prior
            encoded <- encodeLedgerSnapshot next
            root <- ledgerStateRootV1 next
            pure (hexBytes encoded, hexHash root, map hexHash actual)
      in case outcome of
        Left reason -> reply False reason Nothing
        Right result -> reply True "none" (Just result)
  where
    parser = withObject "local reward block" $ \o -> do
      chainId <- o .: "chain_id" :: Parser Word32
      height <- o .: "height" :: Parser Word64
      when (chainId /= 1 || height == 0) (fail "Unsupported chain or genesis height")
      snapshotHex <- o .: "state_hex" :: Parser String
      when (length snapshotHex > 2 * 1024 * 1024) (fail "Snapshot too large")
      snapshot <- decodeRaw snapshotHex
      budget <- o .: "budget" :: Parser Word64
      batchHex <- o .: "batch_id" :: Parser String
      batchId <- parseHash batchHex
      values <- o .: "works" :: Parser [Value]
      when (null values || length values > 64) (fail "Expected 1..64 works")
      claims <- traverse claimedId values
      works <- traverse parseRewardWork values
      pure (chainId, height, snapshot, budget, batchId, claims, works)
    decodeRaw hex
      | odd (length hex) || not (all isHexDigit hex) =
          fail "Invalid hex snapshot"
      | otherwise = pure (BS.pack (decodeBytes hex))
    decodeBytes (a:b:rest) =
      fromIntegral (16 * digitToInt a + digitToInt b) : decodeBytes rest
    decodeBytes _ = []
    claimedId = withObject "claimed work" $ \o -> do
      receiptHex <- o .: "receipt_id" :: Parser String
      parseHash receiptHex
    receiptIdFor chainId work =
      let proof = workProof work
      in canonicalReceiptIdV1 chainId (proofPuzzleId proof)
           (proofWorkerId proof) (proofStartUnit proof)
           (proofUnitCount proof) (proofResultCommitment proof)
    makeEntitlement (work, receiptId) = do
      let proof = workProof work
      address <- mkAddress (hash256Bytes (proofWorkerId proof))
      pure (RewardEntitlement receiptId (proofWorkerId proof)
            address (proofUnitCount proof))
    reply :: Bool -> String -> Maybe (String, String, [String]) -> BS.ByteString
    reply accepted reason result = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("ledger_reward_block_fixture" :: String)
      , "protocol_version" .= (1 :: Int)
      , "accepted" .= accepted
      , "reason" .= (reason :: String)
      , "scope" .= ("caller_attested_fee_funded_reward_block_only" :: String)
      , "state_hex" .= fmap (\(a,_,_) -> a) result
      , "state_root" .= fmap (\(_,b,_) -> b) result
      , "receipt_ids" .= fmap (\(_,_,c) -> c) result
      ]))

-- | Settle one consensus-finalized V3 AI-work block from the exact prior
-- typed ledger. Haskell recomputes receipt IDs, eligibility, the scheduled
-- Sayed Token issuance for this height and the funded payout. The Rust
-- caller must have authenticated the task, assignments and worker
-- signatures and recomputed every result before attesting "verified":true.
ledgerAiWorkBlockV3Response :: Value -> BS.ByteString
ledgerAiWorkBlockV3Response value =
  case parseEither parser value of
    Left _ -> reply False "invalid_ai_work_block_request" Nothing
    Right (chainId, height, snapshot, feeBudget, issuance, policy, batchId, claimed, works) ->
      let outcome = do
            prior <- decodeLedgerSnapshot snapshot
            _ <- eligibleContributions height works
            actual <- traverse (receiptIdFor chainId) works
            when (actual /= claimed) (Left "Canonical receipt ID mismatch")
            entitlements <- traverse makeEntitlement (zip works actual)
            next <- settleIssuedRewards policy height batchId feeBudget issuance
                      entitlements prior
            encoded <- encodeLedgerSnapshot next
            root <- ledgerStateRootV1 next
            pure ( hexBytes encoded, hexHash root, map hexHash actual
                 , ledgerSupply prior, ledgerSupply next )
      in case outcome of
        Left reason -> reply False reason Nothing
        Right result -> reply True "none" (Just result)
  where
    parser = withObject "ai work block v3" $ \o -> do
      chainId <- o .: "chain_id" :: Parser Word32
      height <- o .: "height" :: Parser Word64
      when (chainId /= 1 || height == 0) (fail "Unsupported chain or genesis height")
      snapshotHex <- o .: "state_hex" :: Parser String
      when (length snapshotHex > 2 * 1024 * 1024) (fail "Snapshot too large")
      snapshot <- decodeRaw snapshotHex
      feeBudget <- o .: "fee_budget" :: Parser Word64
      issuance <- o .: "issuance" :: Parser Word64
      policyValue <- o .: "policy"
      policy <- withObject "issuance policy" (\p -> do
        initial <- p .: "initial_reward" :: Parser Word64
        interval <- p .: "halving_interval" :: Parser Word64
        cap <- p .: "max_supply" :: Parser Word64
        either fail pure (mkIssuancePolicy initial interval cap)) policyValue
      batchHex <- o .: "batch_id" :: Parser String
      batchId <- parseHash batchHex
      values <- o .: "works" :: Parser [Value]
      when (null values || length values > 64) (fail "Expected 1..64 works")
      claims <- traverse claimedId values
      works <- traverse parseRewardWork values
      pure (chainId, height, snapshot, feeBudget, issuance, policy, batchId, claims, works)
    decodeRaw hex
      | odd (length hex) || not (all isHexDigit hex) =
          fail "Invalid hex snapshot"
      | otherwise = pure (BS.pack (decodeBytes hex))
    decodeBytes (a:b:rest) =
      fromIntegral (16 * digitToInt a + digitToInt b) : decodeBytes rest
    decodeBytes _ = []
    claimedId = withObject "claimed work" $ \o -> do
      receiptHex <- o .: "receipt_id" :: Parser String
      parseHash receiptHex
    receiptIdFor chainId work =
      let proof = workProof work
      in canonicalReceiptIdV1 chainId (proofPuzzleId proof)
           (proofWorkerId proof) (proofStartUnit proof)
           (proofUnitCount proof) (proofResultCommitment proof)
    makeEntitlement (work, receiptId) = do
      let proof = workProof work
      address <- mkAddress (hash256Bytes (proofWorkerId proof))
      pure (RewardEntitlement receiptId (proofWorkerId proof)
            address (proofUnitCount proof))
    reply :: Bool -> String -> Maybe (String, String, [String], Integer, Integer)
          -> BS.ByteString
    reply accepted reason result = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("ledger_ai_work_block_v3" :: String)
      , "protocol_version" .= (1 :: Int)
      , "accepted" .= accepted
      , "reason" .= (reason :: String)
      , "scope" .= ("rust_recomputed_ai_work_with_scheduled_issuance" :: String)
      , "state_hex" .= fmap (\(a,_,_,_,_) -> a) result
      , "state_root" .= fmap (\(_,b,_,_,_) -> b) result
      , "receipt_ids" .= fmap (\(_,_,c,_,_) -> c) result
      , "supply_before" .= fmap (\(_,_,_,d,_) -> d) result
      , "supply_after" .= fmap (\(_,_,_,_,e) -> e) result
      ]))

ledgerAiWorkBlockV3Request :: Value -> Maybe BS.ByteString
ledgerAiWorkBlockV3Request value =
  case parseEither parser value of
    Right True -> Just (ledgerAiWorkBlockV3Response value)
    _ -> Nothing
  where
    parser = withObject "ai work block request" $ \o -> do
      version <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (version == 1 && kind == "ledger_ai_work_block_v3")

ledgerRewardBlockRequest :: Value -> Maybe BS.ByteString
ledgerRewardBlockRequest value =
  case parseEither parser value of
    Right True -> Just (ledgerRewardBlockResponse value)
    _ -> Nothing
  where
    parser = withObject "reward block request" $ \o -> do
      version <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (version == 1 && kind == "ledger_reward_block_fixture")

settlementRequest :: Value -> Maybe BS.ByteString
settlementRequest value =
  case parseEither parseKind value of
    Right True -> Just (settlementFixtureResponse value)
    _ -> Nothing
  where
    parseKind = withObject "request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (protocol == 1 && kind == "settlement_fixture")

-- | Transfer fixture: the Rust caller supplies the stored previous
-- snapshot plus a signed V1 transaction. Haskell authenticates Ed25519
-- ownership and signature before running pure transfer rules; consensus is
-- not implemented.
ledgerTransitionResponse :: Value -> BS.ByteString
ledgerTransitionResponse value =
  case parseEither parseTransferFixture value of
    Left _ -> decision False "invalid_ledger_transition_request" Nothing
    Right (priorBytes, tx) ->
      let verdict = do
            previous <- decodeLedgerSnapshot priorBytes
            verifySignedTransferV1 tx
            identifier <- signedTransactionIdV1 tx
            transactionRoot <- transactionRootV1 [identifier]
            updated <- applyVerifiedTransfer tx previous
            stateRoot <- ledgerStateRootV1 updated
            encoded <- encodeLedgerSnapshot updated
            pure (encoded, identifier, transactionRoot, stateRoot)
      in case verdict of
        Left problem -> decision False problem Nothing
        Right (bytes, identifier, transactionRoot, stateRoot) ->
          decision True "none" (Just
            (hexBytes bytes, hexHash identifier, hexHash transactionRoot,
             hexHash stateRoot))
  where
    parseTransferFixture = withObject "ledger transfer fixture" $ \o -> do
      version <- o .: "version" :: Parser Word64
      chain <- o .: "chain_id" :: Parser Word64
      when (version /= 1 || chain /= 1)
        (fail "Unsupported transfer version or chain")
      snapshotHex <- o .: "state_hex" :: Parser String
      when (length snapshotHex > 2 * 1024 * 1024)
        (fail "Ledger snapshot exceeds prototype limit")
      prior <- parseHexString snapshotHex
      transferValue <- o .: "transfer"
      tx <- withObject "transfer" (\t -> do
        senderHex <- t .: "sender" :: Parser String
        recipientHex <- t .: "recipient" :: Parser String
        sender <- parseAddress senderHex
        recipient <- parseAddress recipientHex
        amount <- t .: "amount" :: Parser Word64
        fee <- t .: "fee" :: Parser Word64
        nonce <- t .: "nonce" :: Parser Word64
        publicHex <- t .: "public_key" :: Parser String
        signatureHex <- t .: "signature" :: Parser String
        publicKey <- parseFixedHex 32 publicHex
        signature <- parseFixedHex 64 signatureHex
        pure (Transaction version chain sender recipient
                (mkAmount amount) (mkAmount fee) nonce Nothing
                publicKey signature)) transferValue
      pure (prior, tx)
    parseAddress string = do
      digest <- parseHash string
      either fail pure (mkAddress (hash256Bytes digest))
    parseFixedHex size hex = do
      when (length hex /= size * 2) (fail "Invalid signature or key width")
      parseHexString hex
    parseHexString hex
      | odd (length hex) || not (all isHexDigit hex) =
          fail "Invalid hex ledger snapshot"
      | otherwise = pure (BS.pack (decodeHex hex))
      where
        decodeHex (a:b:rest) =
          fromIntegral (16 * digitToInt a + digitToInt b) : decodeHex rest
        decodeHex _ = []
    decision :: Bool -> String -> Maybe (String, String, String, String)
             -> BS.ByteString
    decision accepted reason result = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("ledger_signed_transfer_fixture" :: String)
      , "protocol_version" .= (1 :: Int)
      , "accepted" .= accepted
      , "reason" .= (reason :: String)
      , "scope" .= ("ed25519_verified_local_transfer_only" :: String)
      , "state_hex" .= fmap (\(a,_,_,_) -> a) result
      , "transaction_id" .= fmap (\(_,b,_,_) -> b) result
      , "transactions_root" .= fmap (\(_,_,c,_) -> c) result
      , "state_root" .= fmap (\(_,_,_,d) -> d) result
      ]))

ledgerTransitionRequest :: Value -> Maybe BS.ByteString
ledgerTransitionRequest value =
  case parseEither parser value of
    Right True -> Just (ledgerTransitionResponse value)
    _ -> Nothing
  where
    parser = withObject "ledger transition request" $ \o -> do
      version <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (version == 1 && kind == "ledger_signed_transfer_fixture")

-- | Isolated signed-batch fixture. Every transaction is independently
-- authenticated; execution is pure and aborts as a whole on any error.
ledgerBatchResponse :: Value -> BS.ByteString
ledgerBatchResponse value =
  case parseEither parseBatch value of
    Left _ -> answer False "invalid_signed_batch_request" Nothing
    Right (snapshot, transfers) ->
      let outcome = do
            previous <- decodeLedgerSnapshot snapshot
            (next, identifiers, txRoot, stateRoot) <-
              applySignedBatchV1 previous transfers
            bytes <- encodeLedgerSnapshot next
            pure (hexBytes bytes, map hexHash identifiers,
                  hexHash txRoot, hexHash stateRoot)
      in case outcome of
        Left reason -> answer False reason Nothing
        Right computed -> answer True "none" (Just computed)
  where
    parseBatch = withObject "signed batch" $ \o -> do
      version <- o .: "version" :: Parser Word64
      chain <- o .: "chain_id" :: Parser Word64
      when (version /= 1 || chain /= 1)
        (fail "Unsupported signed-batch version or chain")
      snapshotHex <- o .: "state_hex" :: Parser String
      when (length snapshotHex > 2 * 1024 * 1024)
        (fail "Ledger snapshot too large")
      previous <- parseRawHex snapshotHex
      supplied <- o .: "transactions" :: Parser [Value]
      when (null supplied || length supplied > 64)
        (fail "Signed-batch count must be 1 to 64")
      transactions <- traverse (parseTransfer version chain) supplied
      pure (previous, transactions)

    parseTransfer version chain = withObject "signed transfer" $ \t -> do
      senderHex <- t .: "sender" :: Parser String
      recipientHex <- t .: "recipient" :: Parser String
      senderHash <- parseHash senderHex
      recipientHash <- parseHash recipientHex
      sender <- either fail pure (mkAddress (hash256Bytes senderHash))
      recipient <- either fail pure (mkAddress (hash256Bytes recipientHash))
      amount <- t .: "amount" :: Parser Word64
      fee <- t .: "fee" :: Parser Word64
      nonce <- t .: "nonce" :: Parser Word64
      keyHex <- t .: "public_key" :: Parser String
      signatureHex <- t .: "signature" :: Parser String
      when (length keyHex /= 64 || length signatureHex /= 128)
        (fail "Incorrect Ed25519 field size")
      key <- parseRawHex keyHex
      signature <- parseRawHex signatureHex
      pure (Transaction version chain sender recipient (mkAmount amount)
            (mkAmount fee) nonce Nothing key signature)

    parseRawHex hex
      | odd (length hex) || not (all isHexDigit hex) =
          fail "Invalid hex encoding"
      | otherwise = pure (BS.pack (decodeBytes hex))
    decodeBytes (a:b:rest) =
      fromIntegral (16 * digitToInt a + digitToInt b) : decodeBytes rest
    decodeBytes _ = []

    answer :: Bool -> String -> Maybe (String,[String],String,String)
           -> BS.ByteString
    answer accepted reason result = BL.toStrict (encode (object
      [ "ok" .= True
      , "type" .= ("ledger_signed_batch_fixture" :: String)
      , "protocol_version" .= (1 :: Int)
      , "accepted" .= accepted
      , "reason" .= reason
      , "scope" .= ("ed25519_verified_ordered_batch_only" :: String)
      , "state_hex" .= fmap (\(a,_,_,_) -> a) result
      , "transaction_ids" .= fmap (\(_,b,_,_) -> b) result
      , "transactions_root" .= fmap (\(_,_,c,_) -> c) result
      , "state_root" .= fmap (\(_,_,_,d) -> d) result
      ]))

ledgerBatchRequest :: Value -> Maybe BS.ByteString
ledgerBatchRequest value =
  case parseEither parser value of
    Right True -> Just (ledgerBatchResponse value)
    _ -> Nothing
  where
    parser = withObject "signed batch request" $ \o -> do
      version <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (version == 1 && kind == "ledger_signed_batch_fixture")

handleIpcRequest :: String -> Maybe String
handleIpcRequest request
  | otherwise =
      case eitherDecodeStrict' (BS.pack (map (fromIntegral . ord) request)) :: Either String Value of
        Left _ -> Nothing
        Right value ->
          case parseEither parseHealth value of
            Right True -> Just healthResponse
            _ -> case parseEither parseFixture value of
              Right (Just True) -> Just (shapeResponse True)
              Right (Just False) -> Just (shapeResponse False)
              _ -> fmap (map (chr . fromIntegral) . BS.unpack) (case childRequest value of
                Just result -> Just result
                Nothing -> case rewardRequest value of
                  Just result -> Just result
                  Nothing -> case ledgerAiWorkBlockV3Request value of
                   Just result -> Just result
                   Nothing -> case ledgerRewardBlockRequest value of
                    Just result -> Just result
                    Nothing -> case settlementRequest value of
                      Just result -> Just result
                      Nothing -> case ledgerBatchRequest value of
                        Just result -> Just result
                        Nothing -> case ledgerTransitionRequest value of
                          Just result -> Just result
                          Nothing -> dynamicRequest request)
  where
    parseHealth = withObject "health request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      pure (protocol == 1 && kind == "health")

    parseFixture = withObject "request" $ \o -> do
      protocol <- o .: "protocol_version" :: Parser Int
      kind <- o .: "type" :: Parser String
      fixture <- o .:? "case" :: Parser (Maybe String)
      pure $ if protocol == 1 && kind == "validate_block_shape"
        then case fixture of
          Just "valid_genesis" -> Just True
          Just "invalid_genesis_parent" -> Just False
          _ -> Nothing
        else Nothing

ipcHealth :: IO ()
ipcHealth = do
  hSetBinaryMode stdin True
  hSetBinaryMode stdout True
  result <- try readFrame :: IO (Either IOException (Either String String))
  case result of
    Left _ -> failIpc "Incomplete IPC frame"
    Right (Left problem) -> failIpc problem
    Right (Right request) ->
      case handleIpcRequest request of
        Just response -> writeFrame response
        Nothing -> failIpc "Unsupported IPC request"

failIpc :: String -> IO ()
failIpc message = do
  hPutStrLn stderr ("AivoBlock core IPC error: " ++ message)
  exitFailure

main :: IO ()
main = do
  args <- getArgs
  case parseCommand args of
    Right Help      -> printHelp
    Right Version   -> putStrLn version
    Right Status    -> printStatus
    Right IpcHealth -> ipcHealth
    Left err        -> do
      hPutStrLn stderr err
      exitFailure
