#!/bin/sh
# Regenerate the models of the classic part headlessly (needs Blender; env BLENDER_BIN names it). Run from anywhere:
#   sh classic/tools/blender/export_all.sh
# This is platform/tools/blender/export_all.sh --part classic: it writes only classic/assets/models/ and nothing else
# (see that script's header for what each part writes). The catalogs are in classic/tools/blender/.
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
exec sh "$DIR/../../../platform/tools/blender/export_all.sh" --part classic
