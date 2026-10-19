#!/usr/bin/env bash
# Import a Developer ID Application certificate into a temporary keychain
# on a macOS runner and export its identity as APPLE_SIGNING_IDENTITY.
#
# Needs APPLE_CERTIFICATE (base64 of the .p12) and
# APPLE_CERTIFICATE_PASSWORD. Does nothing when APPLE_CERTIFICATE is empty.
set -euo pipefail

if [ -z "${APPLE_CERTIFICATE:-}" ]; then
  echo "no signing certificate; builds stay ad-hoc signed"
  exit 0
fi

keychain="$RUNNER_TEMP/red-signing.keychain-db"
password=$(openssl rand -hex 16)
echo "$APPLE_CERTIFICATE" | base64 --decode > "$RUNNER_TEMP/certificate.p12"

security create-keychain -p "$password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$RUNNER_TEMP/certificate.p12" -k "$keychain" \
  -P "$APPLE_CERTIFICATE_PASSWORD" -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple: -s -k "$password" "$keychain" >/dev/null
# Word splitting is wanted: keep the existing keychains in the search list.
# shellcheck disable=SC2046
security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
rm -f "$RUNNER_TEMP/certificate.p12"

identity=$(security find-identity -v -p codesigning "$keychain" \
  | grep -o '"Developer ID Application: [^"]*"' | head -1 | tr -d '"')
if [ -z "$identity" ]; then
  echo "no Developer ID Application identity in the certificate" >&2
  exit 1
fi
echo "signing as $identity"
echo "APPLE_SIGNING_IDENTITY=$identity" >> "$GITHUB_ENV"
