#!/bin/sh
set -eu

source_binary="$1"
shift
display_binary="$(dirname "$source_binary")/Model Battle"

cp "$source_binary" "$display_binary"
exec "$display_binary" "$@"
