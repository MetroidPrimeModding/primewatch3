#!/usr/bin/env bash
# Regenerates prime_defs/layouts/ and prime_defs/symbols/ with prime-decomp's
# scripts/export_types.py and scripts/export_data_symbols.py. See doc/multi-version.md.
#
# Usage: tools/gen_decomp_data.sh [VERSION...]    (default: all GameCube revisions)
#
# Environment:
#   DECOMP  prime-decomp checkout                        (default: ../prime-decomp)
#   DTK     decomp-toolkit source dir or binary with `dwarf types`, passed to
#           configure.py --dtk                           (default: ../decomp-toolkit)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
decomp=$(realpath "${DECOMP:-$root/../prime-decomp}")
dtk=$(realpath "${DTK:-$root/../decomp-toolkit}")

if [ $# -gt 0 ]; then
  versions=("$@")
else
  versions=(GM8E01_00 GM8E01_01 GM8E01_48 GM8P01_00 GM8J01_00 GM8E01_02)
fi

version_args=()
for v in "${versions[@]}"; do
  version_args+=(-v "$v")
done

# Symbols don't need a build, so write them even if a layout export fails.
status=0
python3 "$decomp/scripts/export_types.py" -o "$root/prime_defs/layouts" "${version_args[@]}" \
  -- --dtk "$dtk" || status=$?
python3 "$decomp/scripts/export_data_symbols.py" -o "$root/prime_defs/symbols" \
  --aliases "$root/tools/symbol_aliases.txt" "${versions[@]}"
exit $status
