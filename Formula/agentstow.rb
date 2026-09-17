# Frozen at 2.0.6, the farewell release. agentstow is now agent-sync, and
# scripts/update-formula.sh regenerates Formula/agent-sync.rb instead — this
# file is no longer generated and is left here deliberately.
#
# Deleting it would make `brew upgrade` fail with "No available formula" for
# everyone who already installed agentstow, which says nothing about where the
# project went. A deprecated formula still installs, and prints the reason.
#
#   brew tap agent-sync-sh/tap https://github.com/agent-sync-sh/agent-sync
#   brew install agent-sync
class Agentstow < Formula
  desc "One canonical .agents/ folder, fanned out to all your AI coding agents"
  homepage "https://agent-sync.sh"
  license "MIT"

  deprecate! date: "2026-09-17", because: "it was renamed to `agent-sync`"

  on_macos do
    on_arm do
      url "https://github.com/agentstow/agentstow/releases/download/v2.0.6/agentstow-2.0.6-darwin-arm64.tar.gz"
      sha256 "64be078f8cbb8ecb7ab17fadb923132d744b3e589a3c4215aba81abeb114a8fe"
    end
    on_intel do
      url "https://github.com/agentstow/agentstow/releases/download/v2.0.6/agentstow-2.0.6-darwin-x64.tar.gz"
      sha256 "6f32629d1e169f2daba2a4624b34df0602eb6a351088bf68f1e11a7c46cf8672"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/agentstow/agentstow/releases/download/v2.0.6/agentstow-2.0.6-linux-arm64.tar.gz"
      sha256 "30eb0bf5d23674adffb3e7e839b9888a7296539d03a23898fccd2b914fc332e1"
    end
    on_intel do
      url "https://github.com/agentstow/agentstow/releases/download/v2.0.6/agentstow-2.0.6-linux-x64.tar.gz"
      sha256 "02236e3a27986d1f7a8af1ebc4d2a9965fe92bdbcfbb84a374201c6a02533113"
    end
  end

  def install
    bin.install "agentstow"
  end

  test do
    assert_match "agentstow #{version}", shell_output("#{bin}/agentstow --version")
  end
end
