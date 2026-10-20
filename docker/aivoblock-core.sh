#!/bin/sh
# Run the Haskell core with the shared libraries copied from its build image.
LD_LIBRARY_PATH="/opt/aivoblock/core/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  exec /opt/aivoblock/core/aivoblock-core.bin "$@"
