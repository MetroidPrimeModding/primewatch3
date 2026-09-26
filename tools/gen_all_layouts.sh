#!/usr/bin/env bash
# Regenerates prime_defs/layouts/ and prime_defs/symbols/ for every GameCube revision
# whose disc image (or extracted files) is in prime-decomp's orig/<VERSION>/.
# See doc/multi-version.md.
#
# Usage: tools/gen_all_layouts.sh [VERSION...]    (default: all GameCube revisions)
#
# Environment:
#   DECOMP  prime-decomp checkout   (default: ../prime-decomp)
#   DTK     dtk with `dwarf types`  (default: ../decomp-toolkit/target/release/dtk,
#           built from the decomp-toolkit `dwarf-json` branch if missing)
#
# Leaves prime-decomp's configure.py patched with tools/prime-decomp-debug.patch;
# build.ninja, objdiff.json and compile_commands.json are restored on exit.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(dirname "$here")
decomp=$(realpath "${DECOMP:-$root/../prime-decomp}")
dtk=${DTK:-$root/../decomp-toolkit/target/release/dtk}

if [ $# -gt 0 ]; then
  versions=("$@")
else
  versions=(GM8E01_00 GM8E01_01 GM8E01_48 GM8P01_00 GM8J01_00 GM8E01_02)
fi

if [ ! -x "$dtk" ]; then
  echo "== building dtk"
  (cd "$(dirname "$dtk")/../.." && cargo build --release)
fi
if ! "$dtk" dwarf types --help > /dev/null 2>&1; then
  echo "$dtk has no \`dwarf types\` command; build decomp-toolkit's dwarf-json branch" >&2
  exit 1
fi
dtk=$(realpath "$dtk")

cd "$decomp"

# Upstream `--debug` doesn't add `-sym on` to the game code's flags and breaks zlib by
# defining DEBUG (doc/multi-version.md has the details).
if ! grep -q 'cflags_retro.append("-sym on")' configure.py; then
  echo "== patching configure.py"
  git apply "$here/prime-decomp-debug.patch"
fi

saved=$(mktemp -d)
for f in build.ninja objdiff.json compile_commands.json; do
  if [ -e "$f" ]; then cp "$f" "$saved/"; fi
done
restore() {
  cp "$saved"/* "$decomp/" 2> /dev/null || true
  rm -rf "$saved"
}
trap restore EXIT

done_versions=()
skipped=()
failed=()
for v in "${versions[@]}"; do
  if [ -z "$(find "orig/$v" -mindepth 1 -not -name .gitkeep -print -quit 2> /dev/null)" ]; then
    skipped+=("$v")
    continue
  fi
  echo "== $v"
  if python3 configure.py --version "$v" --debug --build-dir build_debug &&
    ninja "build_debug/$v/main.elf" &&
    "$here/gen_layouts.sh" "$dtk" "build_debug/$v/main.elf" "$v"; then
    done_versions+=("$v")
  else
    failed+=("$v")
  fi
done

echo "== symbols"
python3 "$here/gen_symbols.py" "$decomp" "${versions[@]}"

echo
echo "layouts regenerated: ${done_versions[*]:-none}"
if [ ${#skipped[@]} -gt 0 ]; then
  echo "skipped (no disc image in $decomp/orig/<VERSION>/): ${skipped[*]}"
fi
if [ ${#failed[@]} -gt 0 ]; then
  echo "FAILED: ${failed[*]}" >&2
  exit 1
fi
