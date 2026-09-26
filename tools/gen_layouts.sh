#!/bin/sh
# Regenerates prime_defs/layouts/<VERSION>.json.gz from a `-g` decomp build.
# See doc/multi-version.md for producing the ELF.
#
# Usage: tools/gen_layouts.sh <path/to/dtk> <path/to/main.elf> <VERSION>
#   e.g. tools/gen_layouts.sh ../decomp-toolkit/target/release/dtk \
#          ../prime-decomp/build_debug/GM8E01_00/main.elf GM8E01_00
#
# `dtk dwarf types` is not upstream yet: it lives on the `dwarf-json` branch
# of decomp-toolkit.
set -eu

if [ $# -ne 3 ]; then
  echo "usage: $0 <dtk> <main.elf> <VERSION>" >&2
  exit 1
fi

dtk=$1
elf=$2
version=$3
out="$(dirname "$0")/../prime_defs/layouts/$version.json.gz"

mkdir -p "$(dirname "$out")"
# dtk logs (conflict warnings) to stdout, so write through a file, not a pipe.
json=$(mktemp)
trap 'rm -f "$json"' EXIT
"$dtk" dwarf types "$elf" -o "$json"
# -n drops the timestamp so regenerating from the same ELF is byte-identical.
gzip -9n < "$json" > "$out.tmp"
mv "$out.tmp" "$out"
echo "wrote $out"
