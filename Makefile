APP := rustcord
CARGO ?= cargo
TARGET ?= $(shell rustc -vV | sed -n 's/^host: //p')

.PHONY: all build run check profile deps clean fmt

all: build

## Build a size-optimized release binary.
build:
	$(CARGO) build --release --target $(TARGET)

## Run the release build.
run:
	$(CARGO) run --release --target $(TARGET)

## Fast type-check while developing.
check:
	$(CARGO) check

## Resource profiling: build release with debug symbols, then run under massif.
## Requires: pacman -S valgrind
profile:
	CARGO_PROFILE_RELEASE_DEBUG=true $(CARGO) build --release
	valgrind --tool=massif --massif-out-file=massif.out \
		./target/release/$(APP)
	@echo "Open with: ms_print massif.out | less"

## Arch Linux build dependencies. Run once on a fresh install.
deps:
	sudo pacman -S --needed --noconfirm \
		base-devel rust cargo pkgconf \
		wayland wayland-protocols libxkbcommon \
		libx11 libxcursor libxi libxrandr libxinerama \
		mesa vulkan-icd-loader \
		dbus gnome-keyring \
		valgrind

fmt:
	$(CARGO) fmt

clean:
	$(CARGO) clean
