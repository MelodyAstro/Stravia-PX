#!/usr/bin/env bash
set -euo pipefail

native_dependencies_present() {
  command -v cmake >/dev/null &&
    command -v clang >/dev/null &&
    command -v pkg-config >/dev/null &&
    pkg-config --exists openssl &&
    ldconfig -p | grep -E 'libclang(-[0-9]+)?\.so' >/dev/null
}

# Hosted runners normally include this toolchain. Verify it before requesting
# packages, avoiding an unnecessary mirror/DNS dependency on every build.
if native_dependencies_present; then
  cmake --version | head -n 1
  clang --version | head -n 1
  pkg-config --modversion openssl
  exit 0
fi

sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends cmake clang libclang-dev pkg-config libssl-dev
native_dependencies_present
