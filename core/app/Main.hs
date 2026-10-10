-- | Local diagnostics and a bounded IPC health endpoint for the Rust node.
-- A fixture-based block shape IPC check is available; full validation
-- and network consensus are not implemented.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (when)
import AivoBlock.Block (Block(..), BlockBody(..), BlockHeader(..), zeroHash)
import AivoBlock.Validation (validateBlockShape)
import Data.Bits (shiftL, shiftR, (.&.))
import qualified Data.ByteString as BS
import AivoBlock.Block (mkHash256, Hash256)
import Data.Char (chr, ord)
import Data.Aeson (Value, eitherDecodeStrict', encode, object, (.=), (.:), (.:?), withObject)
import Data.Aeson.Types (Parser, parseEither)
import qualified Data.ByteString.Lazy as BL
import Data.Char (digitToInt, isHexDigit)
import Data.Word (Word32, Word64)

import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, hSetBinaryMode, stderr, stdin, stdout)

version :: String
version = "0.1.0.0"

maxIpcBytes :: Integer
maxIpcBytes = 4 * 1024 * 1024

healthRequest :: String
healthRequest = "{\"protocol_version\":1,\"type\":\"health\"}"

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

handleIpcRequest :: String -> Maybe String
handleIpcRequest request
  | request == healthRequest = Just healthResponse
  | otherwise =
      case eitherDecodeStrict' (BS.pack (map (fromIntegral . ord) request)) :: Either String Value of
        Left _ -> Nothing
        Right value ->
          case parseEither parseFixture value of
            Right (Just True) -> Just (shapeResponse True)
            Right (Just False) -> Just (shapeResponse False)
            _ -> fmap (map chr . BS.unpack) (dynamicRequest request)
  where
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
