#!/usr/bin/env bash
# Copia a fórmula gerada pela release para o tap e faz o push.
#
#   scripts/update-tap.sh v0.1.1 [~/Developer/homebrew-tap]
set -euo pipefail

tag="${1:?uso: update-tap.sh <tag> [pasta do tap]}"
tap="${2:-$HOME/Developer/homebrew-tap}"

gh release download "$tag" --repo leosebben/claude-usage --pattern claude-usage.rb \
  --output "$tap/Formula/claude-usage.rb" --clobber
git -C "$tap" add Formula/claude-usage.rb
git -C "$tap" commit -m "feat: atualizar claude-usage para ${tag#v}"
git -C "$tap" push
