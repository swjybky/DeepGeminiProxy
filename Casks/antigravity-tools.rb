cask "antigravity-tools" do
  version "1.0.7"
  sha256 :no_check

  name "DeepGeminiProxy"
  desc "Professional Account Management for AI Services"
  homepage "https://github.com/swjybky/DeepGeminiProxy"

  on_macos do
    arch intel: "x64", arm: "aarch64"

    url "https://github.com/swjybky/DeepGeminiProxy/releases/download/v#{version}/DeepGeminiProxy_#{version}_#{arch}.dmg"

    app "DeepGeminiProxy.app"
    binary "#{appdir}/DeepGeminiProxy.app/Contents/MacOS/antigravity-tools"
    binary "#{appdir}/DeepGeminiProxy.app/Contents/MacOS/antigravity-tools", target: "at"

    zap trash: [
      "~/Library/Application Support/com.lbjlaq.antigravity-tools",
      "~/Library/Caches/com.lbjlaq.antigravity-tools",
      "~/Library/Preferences/com.lbjlaq.antigravity-tools.plist",
      "~/Library/Saved Application State/com.lbjlaq.antigravity-tools.savedState",
    ]
  end

  on_linux do
    arch arm: "aarch64", intel: "amd64"

    url "https://github.com/swjybky/DeepGeminiProxy/releases/download/v#{version}/DeepGeminiProxy_#{version}_#{arch}.AppImage"
    binary "DeepGeminiProxy_#{version}_#{arch}.AppImage", target: "antigravity-tools"

    preflight_steps do
      set_permissions "DeepGeminiProxy_{{version}}_{{arch}}.AppImage", "0755"
    end
  end
end
