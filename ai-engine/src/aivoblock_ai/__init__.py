"""AivoBlock AI puzzle execution package.

This package implements local AI computation for AivoBlock's custom
blockchain. It does not validate consensus, authorize rewards, or
communicate with other blockchain nodes directly.

The Rust node coordinates workers; the Haskell core defines
consensus-critical verification rules.
"""

__version__ = "0.1.0"

__all__ = ["__version__"]
