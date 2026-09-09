# Fórmula para o tap `leosebben/tap`. Copiar para
# `homebrew-tap/Formula/claude-usage.rb` e atualizar `url` e `sha256` a cada
# release:
#
#   curl -L https://github.com/leosebben/claude-usage/archive/refs/tags/v0.1.0.tar.gz | shasum -a 256
class ClaudeUsage < Formula
  desc "TUI com o uso de tokens e custo do Claude Code"
  homepage "https://github.com/leosebben/claude-usage"
  url "https://github.com/leosebben/claude-usage/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "724c81f0a78034f25e776865002055142f8b95700a208dec6f6b6f1ebce93632"
  license "MIT"
  head "https://github.com/leosebben/claude-usage.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "claude-usage", shell_output("#{bin}/claude-usage --help")
  end
end
