.PHONY: wasm scenarios build serve clean

# Compiles crates/sdts-wasm to a browser ES module and drops it in
# docs/pkg/, served as a static asset (gitignored — see
# .github/workflows/pages.yml, which builds this fresh on deploy).
# wasm-pack writes a `pkg/.gitignore` that excludes everything in the
# directory (sensible for an npm-publish workflow, redundant here since
# the parent .gitignore already covers docs/pkg/), so it's removed right
# after the build to avoid a confusing nested .gitignore.
wasm:
	wasm-pack build crates/sdts-wasm --target web --out-dir ../../docs/pkg
	rm -f docs/pkg/.gitignore

# Copies the scenarios/ directory (source of truth) into docs/scenarios/
# (gitignored, generated) and creates docs/scenarios/manifest.json
# (file/name/description for the browser's scenario picker), so docs/ has
# no runtime dependency on anything outside itself.
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
