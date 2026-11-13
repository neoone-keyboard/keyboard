#!/bin/sh
# Writes the export options `make testflight` uploads with, from apps/ios/Signing.xcconfig (the
# Makefile reads it and passes the values):
#   export-options.sh automatic <team> <out.plist>
#     signed by Xcode's account (Xcode > Settings > Accounts), the profiles managed for it;
#   export-options.sh manual <team> <bundle id> <profile prefix> <out.plist>
#     signed by hand, for an App Store Connect API key: the local Apple Distribution
#     certificate and the profiles named "<profile prefix> <bundle id>" for the app and
#     "<profile prefix> <bundle id>.extension" for the keyboard.
set -eu

style=$1
team=$2
case $style in
  automatic) out=$3 ;;
  manual) app=$3; prefix=$4; out=$5 ;;
  *) echo "usage: $0 automatic <team> <out> | manual <team> <bundle id> <profile prefix> <out>" >&2; exit 2 ;;
esac
test -n "$team" || { echo "DEVELOPMENT_TEAM is empty (apps/ios/Signing.xcconfig)" >&2; exit 1; }

mkdir -p "$(dirname "$out")"
{
  cat <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>method</key>
	<string>app-store-connect</string>
	<key>destination</key>
	<string>upload</string>
	<key>teamID</key>
	<string>$team</string>
	<key>signingStyle</key>
	<string>$style</string>
EOF
  if [ "$style" = manual ]; then
    cat <<EOF
	<key>signingCertificate</key>
	<string>Apple Distribution</string>
	<key>provisioningProfiles</key>
	<dict>
		<key>$app</key>
		<string>$prefix $app</string>
		<key>$app.extension</key>
		<string>$prefix $app.extension</string>
	</dict>
EOF
  fi
  cat <<EOF
	<key>uploadSymbols</key>
	<true/>
	<key>manageAppVersionAndBuildNumber</key>
	<false/>
</dict>
</plist>
EOF
} > "$out"
plutil -lint -s "$out"
