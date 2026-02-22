#!/bin/bash
# Creates AccessPilot-debug.app from the debug build so you can add it to Accessibility.
# Run from agent-app/: ./scripts/make-debug-app.sh

set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
AGENT_APP="$(cd "$SCRIPT_DIR/.." && pwd)"
DEBUG_DIR="$AGENT_APP/src-tauri/target/debug"
BINARY="$DEBUG_DIR/accesspilot_agent"
APP_DIR="$DEBUG_DIR/AccessPilot-debug.app"

if [ ! -f "$BINARY" ]; then
  echo "Debug binary not found. Run 'npm run tauri dev' first to build it."
  exit 1
fi

echo "Creating $APP_DIR"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

cp "$BINARY" "$APP_DIR/Contents/MacOS/accesspilot_agent"

# Launcher: set DYLD_LIBRARY_PATH so binary finds libvosk.dylib (from /usr/local/lib)
LAUNCHER="$APP_DIR/Contents/MacOS/AccessPilot"
cat > "$LAUNCHER" << 'LAUNCHER_SCRIPT'
#!/bin/bash
export DYLD_LIBRARY_PATH="/usr/local/lib:${DYLD_LIBRARY_PATH}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
exec "$SCRIPT_DIR/accesspilot_agent"
LAUNCHER_SCRIPT
chmod +x "$LAUNCHER"

cat > "$APP_DIR/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>AccessPilot</string>
  <key>CFBundleIdentifier</key>
  <string>com.accesspilot.agent.debug</string>
  <key>CFBundleName</key>
  <string>AccessPilot</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
</dict>
</plist>
PLIST

# Remove quarantine and ad-hoc sign so macOS will open it
xattr -cr "$APP_DIR" 2>/dev/null || true
codesign --force --deep --sign - "$APP_DIR" 2>/dev/null || true

echo ""
echo "Done!"
echo ""
echo "ADD TO ACCESSIBILITY:"
echo "  1. System Settings → Privacy & Security → Accessibility → click +"
echo "  2. Press Cmd+Shift+G"
echo "  3. Paste: $APP_DIR"
echo "  4. Press Enter, select AccessPilot-debug.app, click Open"
echo "  5. Toggle it ON"
echo ""
echo "TO USE (with press keys working):"
echo "  - Terminal 1: cd agent-app && npm run dev   (starts Vite)"
echo "  - Double-click: $APP_DIR"
echo "  (Re-run this script after rebuilding with 'npm run tauri dev')"
echo ""
