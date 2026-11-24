#!/bin/sh
# Builds target/wingroup.app; with "install" also puts it in /Applications.
# Menu-bar only, no Dock icon. An app bundle so the
# Accessibility permission attaches to wingroup itself, not to your terminal.
set -e
cd "$(dirname "$0")"
cargo build --release
APP=target/wingroup.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
mkdir -p "$APP/Contents/Resources"
cp target/release/wingroup "$APP/Contents/MacOS/wingroup"
cp resources/AppIcon.icns resources/menubar.png "$APP/Contents/Resources/"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>dev.local.wingroup</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleName</key><string>wingroup</string>
  <key>CFBundleExecutable</key><string>wingroup</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>LSUIElement</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>wingroup hides the Dock in full mode.</string>
</dict></plist>
PLIST
# Pin the designated requirement to the bundle id. Ad-hoc signing alone pins it
# to the code hash, which changes on every build and makes macOS forget the
# Accessibility grant each time.
codesign --force --sign - -r='designated => identifier "dev.local.wingroup"' "$APP"
echo "Built $APP"
if [ "$1" = install ]; then
  pkill -x wingroup || true
  ditto "$APP" /Applications/wingroup.app
  open /Applications/wingroup.app
  echo "Installed in /Applications"
fi
