#!/usr/bin/env bash
# Gera a fórmula do Homebrew para uma versão a partir dos checksums dos
# tarballs de binários.
#
#   scripts/formula.sh 0.1.1 dist > claude-usage.rb
#
# `dist` precisa conter os arquivos `claude-usage-v<versão>-<target>.tar.gz.sha256`
# gerados pelo workflow de release (um por target).
set -euo pipefail

version="${1:?uso: formula.sh <versão> <pasta com os .sha256>}"
dir="${2:?uso: formula.sh <versão> <pasta com os .sha256>}"
repo="https://github.com/leosebben/claude-usage"

sha() {
  local file="$dir/claude-usage-v${version}-$1.tar.gz.sha256"
  [ -f "$file" ] || { echo "faltando $file" >&2; exit 1; }
  cut -d' ' -f1 "$file"
}

url() {
  echo "$repo/releases/download/v${version}/claude-usage-v${version}-$1.tar.gz"
}

cat <<EOF
# Gerada por scripts/formula.sh a partir da release v${version}. Não editar à mão.
class ClaudeUsage < Formula
  desc "TUI com o uso de tokens e custo do Claude Code"
  homepage "$repo"
  license "MIT"

  on_macos do
    on_arm do
      url "$(url aarch64-apple-darwin)"
      sha256 "$(sha aarch64-apple-darwin)"
    end
    on_intel do
      url "$(url x86_64-apple-darwin)"
      sha256 "$(sha x86_64-apple-darwin)"
    end
  end

  on_linux do
    on_arm do
      url "$(url aarch64-unknown-linux-gnu)"
      sha256 "$(sha aarch64-unknown-linux-gnu)"
    end
    on_intel do
      url "$(url x86_64-unknown-linux-gnu)"
      sha256 "$(sha x86_64-unknown-linux-gnu)"
    end
  end

  def install
    bin.install "claude-usage"
  end

  test do
    assert_match "claude-usage", shell_output("#{bin}/claude-usage --help")
  end
end
EOF
