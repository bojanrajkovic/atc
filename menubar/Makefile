# ATC Menubar — the latest ATC jobs in the macOS menu bar
#
#   make           debug-build into ./build/ATCMenubar.app
#   make run       build and launch
#   make test      run the SwiftPM tests
#   make install   release-build and copy to /Applications
#   make dist      Developer ID-signed, notarized, stapled zip in ./dist
#
# Built by `swift build` + ./build.sh. No Xcode project, no xcodebuild.

APP := build/ATCMenubar.app

# Release signing. `make dist TEAM_ID=XXXXXXXXXX` signs with that team's
# Developer ID certificate from the keychain.
VERSION        ?= 0.1.0
TEAM_ID        ?=
CERT_NAME      ?= $(if $(TEAM_ID),Developer ID Application: Bojan Rajkovic ($(TEAM_ID)),)
# Keychain profile holding the notary credentials. Create one with
# `xcrun notarytool store-credentials <name> --team-id <team>`.
NOTARY_PROFILE ?= hermes-notary
RELEASE_ZIP    := dist/ATCMenubar-$(VERSION)-macos.zip

.PHONY: build release run test install clean dist

build:
	./build.sh debug

release:
	./build.sh release

run: build
	open $(APP)

test:
	swift test

install: release
	rm -rf /Applications/ATCMenubar.app
	cp -R $(APP) /Applications/

clean:
	rm -rf build .build dist

# Apple's notary service takes a zip, and stapling writes the ticket into the
# .app, so the zip to distribute is a second one made after stapling.
dist:
	@if [ -z "$(CERT_NAME)" ]; then echo "✗ TEAM_ID required: make dist TEAM_ID=XXXXXXXXXX"; exit 1; fi
	SIGN_IDENTITY="$(CERT_NAME)" VERSION="$(VERSION)" ./build.sh release
	codesign --verify --strict --verbose=2 $(APP)
	@mkdir -p dist
	ditto -c -k --keepParent $(APP) dist/notary.zip
	xcrun notarytool submit dist/notary.zip --keychain-profile "$(NOTARY_PROFILE)" --wait
	xcrun stapler staple $(APP)
	rm -f dist/notary.zip $(RELEASE_ZIP)
	ditto -c -k --keepParent $(APP) $(RELEASE_ZIP)
	spctl --assess --type execute --verbose $(APP)
	@echo "✓ $(RELEASE_ZIP)"
