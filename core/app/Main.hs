-- | Entry point for the AivoBlock Haskell core CLI.
-- The blockchain runtime is implemented separately by the Rust node.
-- This executable exposes local diagnostics while the protocol modules
-- are developed; it does not start a blockchain node.
module Main (main) where

import System.Environment (getArgs)
import System.Exit (exitFailure)
import System.IO (hPutStrLn, stderr)

version :: String
version = "0.1.0.0"

data Command
  = Help
  | Version
  | Status

parseCommand :: [String] -> Either String Command
parseCommand []             = Right Status
parseCommand ["status"]     = Right Status
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

main :: IO ()
main = do
  args <- getArgs
  case parseCommand args of
    Right Help    -> printHelp
    Right Version -> putStrLn version
    Right Status  -> printStatus
    Left err      -> do
      hPutStrLn stderr err
      exitFailure
