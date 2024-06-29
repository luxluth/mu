dev:
  pnpm tauri dev

build:
  pnpm install
  just _build-{{os()}}

_build-linux:
  NO_STRIP=true pnpm tauri build -v --no-bundle

_build-macos:
  pnpm tauri build -v
