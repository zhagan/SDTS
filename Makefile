.PHONY: wasm scenarios build serve clean

# Compiles crates/sdts-wasm to a browser ES module and drops it in
# docs/pkg/, ready to be committed and served as a static asset. wasm-pack
# writes a `pkg/.gitignore` that excludes everything in the directory
# (sensible for an npm-publish workflow, wrong for us — we want docs/pkg/
# checked in for GitHub Pages), so it's removed right after the build.
wasm:
	wasm-pack build crates/sdts-wasm --target web --out-dir ../../docs/pkg
	rm -f docs/pkg/.gitignore

# Copies the checked-in scenarios/ directory into docs/scenarios/ and
# generates docs/scenarios/manifest.json (file/name/description for the
# browser's scenario picker), so docs/ has no runtime dependency on
# anything outside itself.
scenarios:
	./scripts/build-docs-scenarios.sh

build: wasm scenarios

# Serves docs/ with a plain static file server, matching how GitHub Pages
# serves it (no backend, relative paths only).
serve:
	python3 -m http.server 8080 --directory docs

clean:
	cargo clean
	rm -rf docs/pkg
