# Building Quillstone from source

Most people should use the downloadable kit from the
[Releases page](https://github.com/Tsarbomba/quillstone/releases) (see the [README](README.md)).
This page is for building the engine yourself.

## Set up Rust (one time)

**Windows**
1. Download and run `rustup-init.exe` from <https://rustup.rs>.
2. If it asks for the Visual Studio C++ Build Tools, accept. In the installer,
   tick **Desktop development with C++** (this is the linker Rust needs).
3. Open a *new* terminal and check: `cargo --version`

**Linux / macOS**
```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```
(Linux also needs a C compiler: `sudo apt install build-essential`. macOS: `xcode-select --install`.)

## Set up FFmpeg for video (one time)

The player plays video with FFmpeg. Building it needs an FFmpeg 8.x *shared*
build (use an **LGPL** one so games can ship it) and libclang.

**Windows**
1. Download `ffmpeg-n8.1-latest-win64-lgpl-shared-8.1.zip` from
   <https://github.com/BtbN/FFmpeg-Builds/releases> and unzip it to `C:\ffmpeg`
   (so `C:\ffmpeg\include` and `C:\ffmpeg\lib` exist).
2. Install LLVM: `winget install LLVM.LLVM`.
3. Set two variables, then open a *new* terminal:
   `setx FFMPEG_DIR C:\ffmpeg` and `setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"`.
4. At run time the FFmpeg DLLs must be found: add `C:\ffmpeg\bin` to `PATH`, or
   copy its `.dll` files next to `quillstone-player.exe` (that is also how a game ships).

**Linux / WSL**
1. Download `ffmpeg-n8.1-latest-linux64-lgpl-shared-8.1.tar.xz` from the same page and unpack it.
2. Install libclang: `sudo apt install libclang-dev`.
3. `export FFMPEG_DIR=...` (the unpacked folder).
4. To run: `export LD_LIBRARY_PATH=$FFMPEG_DIR/lib`.

**macOS**
1. `brew install ffmpeg pkg-config llvm`
2. `export LIBCLANG_PATH=$(brew --prefix llvm)/lib`

FFmpeg is found through pkg-config there, so no `FFMPEG_DIR` is needed. Homebrew's FFmpeg is a
GPL build: fine for developing, but ship an LGPL build with a game.

No FFmpeg? Build without video: `cargo run --release --no-default-features --features text,audio -- ...`
(videos then show a placeholder).

## Build and run

```
cargo test --workspace                          # run all tests
cargo run -p quillstone -- run   examples/demo.quill.md   # play the demo in the terminal
cargo run -p quillstone -- dump  examples/demo.quill.md   # show the compiled ops
cargo run -p quillstone -- check examples/demo.quill.md   # validate script + assets
cargo run -p quillstone -- test  examples/demo.quill.md   # play every branch, report story bugs
```

The commands are the ones in the README (`cargo run -p quillstone -- <command> ...` in place of `quillstone <command> ...`).

### The player

The player is its own small project so the engine crates stay dependency-free:

```
cd player
cargo run --release -- ../examples/demo.quill.md
```

The first build downloads and compiles wgpu, winit, cosmic-text and kira (a few minutes).

The player has optional features, all on by default: `text`, `audio`, `video` and `gamepad`.
Turning some off narrows a problem down:

| Build with | You get |
| --- | --- |
| `--no-default-features --features text` | no sound or video |
| `--no-default-features` | just the window and images |
| `--no-default-features --features text,audio,video` | no gamepads |

For example: `cargo run --release --no-default-features --features text -- ../examples/demo.quill.md`

- Gamepads need libudev's development files on Linux (`sudo apt install libudev-dev`), or the
  last build in the table.
- Build errors about FFmpeg: see [Set up FFmpeg](#set-up-ffmpeg-for-video-one-time).

For a game to ship, build the player without a console window:

```
cd player
cargo build --release --features no-console
cd ..
cargo run --release -p quillstone -- package examples/demo.quill.md --player player/target/release/quillstone-player.exe --libs C:\ffmpeg\bin --out MyGame
```

With `no-console` there is no console window behind the game on Windows. Messages then go
nowhere, except startup errors and crashes, which land in `error.txt` next to the exe.

## Slim FFmpeg for shipping

The full FFmpeg download carries every codec FFmpeg has (~130 MB of DLLs).
`tools/build-ffmpeg.sh` builds a slim one, which the release kits use. It holds:

- only what Quillstone plays: VP8 and VP9 video in WebM, Vorbis and Opus sound, and `.opus`
  sound files;
- only the five libraries the player loads.

On Windows, run it in the MSYS2 **UCRT64** shell (the packages to install are listed at the top
of the script):

```
cd /c/Users/you/path/to/quillstone
tools/build-ffmpeg.sh ffmpeg-slim
```

Then package with `--libs ffmpeg-slim\bin` (Linux: `ffmpeg-slim/lib`).

The player doesn't need rebuilding: the slim DLLs replace the full ones as long as the FFmpeg
version matches (8.1). The output also has `include/` and `lib/` (on Windows, the `.lib` import
libraries), so `FFMPEG_DIR=<ffmpeg-slim>` builds the player too, which is what the release does.
Day to day, developing against the full download is fine.

## Making a release

Push a tag that starts with `v` (for example `v0.2.0`). That runs
`.github/workflows/release.yml`, which:

1. runs the tests (`cargo test --workspace`);
2. builds the kits for Windows and Linux;
3. publishes them on the Releases page with generated notes.

Each kit holds `quillstone`, `quillstone-player`, the five FFmpeg 8.1 libraries from the slim
build, the licenses, the examples and the docs. A kit is an archive with one folder inside,
named like the archive, and is uploaded as it is, not zipped again.

The slim FFmpeg is built from source on each runner (MSYS2 UCRT64 on Windows). It is cached by
OS and the hash of `tools/build-ffmpeg.sh`, so it rebuilds only when that script changes. The
kit's `licenses/FFmpeg.txt` names the source and links the script; games packaged from the kit
carry it too.

Every push to `main` also runs the tests and clippy on Linux and Windows
(`.github/workflows/test.yml`). Tag only when that run is green.

To try it before tagging, run the workflow by hand from the Actions tab ("Run workflow"). It
builds the same kits and keeps them as workflow artifacts without making a release.

## Layout

| Crate | Job |
| --- | --- |
| `quillstone-script` | parser/compiler (values, expressions, ops), no dependencies |
| `quillstone-runtime` | pure state machine; scene, call stack and variables in `State`; rollback (`Session`) |
| `quillstone-video` | `Decoder` trait, decoder threads + frame queue (`Clip`), which clips play (`Videos`), a test-pattern decoder |
| `quillstone-render` | virtual 1920x1080 screen, letterboxing, `View` (sprites, fades, typewriter) + `Draw` list, `build_frame` (quads + text jobs), image prep (mipmaps, alpha bleeding) |
| `quillstone-audio` | `Mixer`: runtime commands -> audio actions (music crossfades, voice per line, rollback-safe) on music/sfx/voice channels |
| `quillstone` | the CLI (`check`, `test`, `graph`, `voices`, `lang export`, `dump`, `package`, `unpack`, text-mode `run`) |
| `quillstone-ui` | `Game`: input routing per screen, settings (+ file), backlog, quick menu, auto-forward, skip mode, save/load slots, confirm dialogs |
| `player/` | the window: winit + wgpu draws the frame, cosmic-text rasterizes text, kira plays sound, FFmpeg decodes video |

The renderer is split on purpose: `View` turns runtime commands into a plain
list of `Draw` items (images, rectangles, text), and a backend only has to
execute that list. A video frame is one more textured quad.
