-- | Local diagnostics and a bounded IPC health endpoint for the Rust node.
-- No blockchain block validation or network consensus is provided yet.
module Main (main) where

import Control.Exception (IOException, try)
import Control.Monad (replicateM, when)
import Data.Bits (shiftR, (.&.))
import Data.Char (chr, ord)
import Data.List (foldl')
import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hIsEOF, hPutStrLn, hSetBinaryMode, stderr, stdin, stdout)

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
  , "  ipc-health   Exchange one length-prefixed IPC health request"
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

-- Binary handles use byte-valued characters; protocol health JSON is ASCII.
-- This endpoint deliberately supports one exact diagnostic message only.
readFrame :: IO (Either String String)
readFrame = do
  header <- replicateM 4 (hGetCharByte)
  let count = foldl' (\total byte -> total * 256 + toInteger byte) 0 header
  if count <= 0 || count > maxIpcBytes
    then pure (Left "Invalid IPC payload length")
    else do
      payload <- replicateM (fromInteger count) hGetCharByte
      extra <- hIsEOF stdin
      pure $ if extra then Right (map chr payload)
                      else Left "Unexpected trailing IPC bytes"

hGetCharByte :: IO Int
hGetCharByte = ord <$> getChar

writeFrame :: String -> IO ()
writeFrame body = do
  let lengthBytes = length body
      byteAt shift = chr ((lengthBytes `shiftR` shift) .&. 255)
  mapM_ putChar [byteAt 24, byteAt 16, byteAt 8, byteAt 0]
  putStr body

ipcHealth :: IO ()
ipcHealth = do
  hSetBinaryMode stdin True
  hSetBinaryMode stdout True
  result <- try readFrame :: IO (Either IOException (Either String String))
  case result of
    Left _ -> failIpc "Incomplete IPC frame"
    Right (Left problem) -> failIpc problem
    Right (Right request) ->
      if request == healthRequest
        then writeFrame healthResponse
        else failIpc "Unsupported IPC request"

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
