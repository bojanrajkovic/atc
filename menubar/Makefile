# ATC Menubar — the latest ATC jobs in the macOS menu bar
#
#   make           debug-build into ./build/ATCMenubar.app
#   make run       build and launch
#   make test      run the SwiftPM tests
#   make install   release-build and copy to /Applications
#
# Built by `swift build` + ./build.sh. No Xcode project, no xcodebuild.

APP := build/ATCMenubar.app

.PHONY: build release run test install clean

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
	rm -rf build .build
