# Build pipeline. Outputs land in web/, which the Worker serves as static assets.

PROFILE ?= wasm-release
PROFILE_DIR := $(if $(filter dev,$(PROFILE)),debug,$(PROFILE))

# wasm-bindgen-cli must match the wasm-bindgen crate in Cargo.lock, so install it locally, one
# folder per version: a version bump installs the new one, other Cargo.lock changes don't.
WASM_BINDGEN_VERSION := $(shell awk '/^name = "wasm-bindgen"$$/{getline; gsub(/"/, "", $$3); print $$3; exit}' Cargo.lock)
WASM_BINDGEN := .tools/wasm-bindgen-$(WASM_BINDGEN_VERSION)/bin/wasm-bindgen
# wasm-opt comes with the pinned emsdk.
WASM_OPT := emulator/.cache/emsdk/upstream/bin/wasm-opt

.PHONY: client netplay emulator emulator-remote upload-emulator upload-rom dev deploy preview editor

$(WASM_BINDGEN):
	cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --root .tools/wasm-bindgen-$(WASM_BINDGEN_VERSION) --locked

$(WASM_OPT):
	./emulator/emsdk.sh

# Bevy client -> web/pkg/ (https://github.com/bevyengine/bevy/tree/latest/examples#wasm),
# plus its art -> web/assets/.
client: $(WASM_BINDGEN) $(WASM_OPT)
	rm -rf web/assets && cp -R assets web/assets
	cargo build -p client --profile $(PROFILE) --target wasm32-unknown-unknown
	$(WASM_BINDGEN) --out-dir web/pkg --target web \
		target/wasm32-unknown-unknown/$(PROFILE_DIR)/client.wasm
ifeq ($(PROFILE),wasm-release)
	$(WASM_OPT) -Oz --output web/pkg/client_bg.opt.wasm web/pkg/client_bg.wasm
	mv web/pkg/client_bg.opt.wasm web/pkg/client_bg.wasm
endif

# GGRS rollback for the emulator worker (netplay/src/lib.rs) -> web/netplay/.
netplay: $(WASM_BINDGEN) $(WASM_OPT)
	cargo build -p netplay --profile wasm-release --target wasm32-unknown-unknown
	$(WASM_BINDGEN) --out-dir web/netplay --target web \
		target/wasm32-unknown-unknown/wasm-release/netplay.wasm
	$(WASM_OPT) -Oz --output web/netplay/netplay_bg.opt.wasm web/netplay/netplay_bg.wasm
	mv web/netplay/netplay_bg.opt.wasm web/netplay/netplay_bg.wasm

# FBNeo cores -> emulator/dist/<core>/, then into local R2 (served at /fbneo/<core>/*).
emulator:
	./emulator/build.sh
	$(MAKE) upload-emulator R2_TARGET=--local

# Production R2 (create the bucket once: cd server && npx wrangler r2 bucket create vab).
emulator-remote:
	$(MAKE) upload-emulator R2_TARGET=--remote

# A ROM set (or its start-up .state) into R2, served at /roms/<file>:
# make upload-rom ROM=$HOME/Downloads/mk2.zip (add R2_TARGET=--remote for production, and
# R2_BUCKET=vab-preview for the preview Worker's bucket).
R2_TARGET ?= --local
R2_BUCKET ?= vab
upload-rom:
	cd server && npx wrangler r2 object put $(R2_BUCKET)/roms/$(notdir $(ROM)) $(R2_TARGET) \
		--file $(abspath $(ROM)) \
		--content-type $(if $(filter %.zip,$(ROM)),application/zip,application/octet-stream)

upload-emulator:
	cd server && for dir in ../emulator/dist/*/; do core=$$(basename $$dir); \
		npx wrangler r2 object put $(R2_BUCKET)/fbneo/$$core/fbneo.mjs $(R2_TARGET) \
			--file $$dir/fbneo.mjs --content-type text/javascript && \
		npx wrangler r2 object put $(R2_BUCKET)/fbneo/$$core/fbneo.wasm $(R2_TARGET) \
			--file $$dir/fbneo.wasm --content-type application/wasm || exit 1; \
	done

# Worker + Durable Object, serving web/ (http://localhost:8787)
dev: client netplay
	cd server && npx wrangler dev

deploy: client netplay
	cd server && npx wrangler deploy

# The preview Worker (server/wrangler.toml). Its bucket gets cores and ROMs like the main one:
# make emulator-remote R2_BUCKET=vab-preview, make upload-rom R2_TARGET=--remote R2_BUCKET=vab-preview ...
preview: client netplay
	cd server && npx wrangler deploy --env preview

# Bar layout editor (desktop dev tool, not deployed). Saves assets/maps/bar.ron.
editor:
	cargo run -p editor
