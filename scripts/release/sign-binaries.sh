#!/usr/bin/env bash
# Sign red, redd and red-helper with the Developer ID identity from
# import-certificate.sh and have Apple notarize them.
#
#   scripts/release/sign-binaries.sh <dir-with-binaries>
#
# Without APPLE_SIGNING_IDENTITY the binaries keep the ad-hoc signature the
# linker gave them. Notarization runs when APPLE_ID, APPLE_PASSWORD (an
# app-specific password) and APPLE_TEAM_ID are set.
set -euo pipefail

dir=$1
if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "no signing identity; leaving the binaries ad-hoc signed"
  exit 0
fi

for bin in red redd red-helper; do
  codesign --force --options runtime --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dir/$bin"
  codesign --verify --strict "$dir/$bin"
done

if [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  zip="$RUNNER_TEMP/red-notarize-$$.zip"
  ditto -c -k "$dir" "$zip"
  # Bare executables cannot be stapled; Gatekeeper looks the ticket up online.
  xcrun notarytool submit "$zip" --wait \
    --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID"
  rm -f "$zip"
else
  echo "no notarization credentials; signed but not notarized"
fi
