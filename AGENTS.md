# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs` — Rocket app with upload, background ffmpeg packaging, HLS file serving, and Tera route handlers.
- `templates/` — Tera views for index, upload (drag & drop auto-upload), and watch pages.
- `Rocket.toml` — Server limits (larger multipart uploads) and default address/port.
- `Dockerfile` & `docker-compose.yml` — Multi-stage build with ffmpeg runtime; mounts `./data` to `/data` in the container.
- `data/` — Local dev storage; created automatically. Contains `uploads/` (originals) and `hls/` (packaged outputs).

## Build, Test, and Development Commands
- `cargo check` — Fast compile check.
- `cargo run` — Launch Rocket locally; reads templates from `templates/` and writes to `./data`.
- `ROCKET_PROFILE=release cargo run --release` — Closer-to-prod run.
- `docker-compose up --build` — Build and run inside Docker with ffmpeg installed.

## Coding Style & Naming Conventions
- Rust 2021 edition; prefer `clippy::pedantic` cleanliness when possible (not yet enforced).
- Use `snake_case` for functions/vars, `PascalCase` for types; keep modules lean (single-file app for now).
- Keep inline comments focused on non-obvious logic (e.g., background task behavior).

## Testing Guidelines
- No automated tests yet; add integration tests with `rocket::local::asynchronous` when changing request flows.
- Name test functions `test_<route|behavior>`; place under `tests/` or `src/` modules as appropriate.

## Commit & Pull Request Guidelines
- Commit messages: concise imperative conventional commit messages (e.g., “feat: add background ffmpeg task”, “fix: tweak upload limits”).
- PRs should include: summary of user-visible changes, how to run/validate (`cargo check`, manual upload), and screenshots/GIFs for UI tweaks (upload drag-drop, watch page states).

## Security & Configuration Tips
- Set `DATA_DIR` env to choose storage root (defaults to `./data`); in Docker this is `/data`.
- Ensure `ffmpeg` is available when running outside Docker.
- Upload size is bounded by `Rocket.toml` limits; adjust if needed for larger assets.

## Operational Notes
- Upload flow: request returns immediately; background task writes `hls/<id>/master.m3u8` and `error.txt` on failure.
- Watch page now auto-polls for readiness; upload page shows per-file progress and processing status.
- Uploads are retained by default; original MP4s stay in `data/uploads` after processing.
