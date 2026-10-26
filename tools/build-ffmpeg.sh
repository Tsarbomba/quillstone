#!/usr/bin/env bash
# Build a slim LGPL FFmpeg with only what Quillstone plays, for shipping games.
#
#   Windows: run in the MSYS2 *UCRT64* shell. First, once:
#     pacman -S --needed curl make diffutils tar xz mingw-w64-ucrt-x86_64-gcc mingw-w64-ucrt-x86_64-nasm
#   Linux: the same tools from your package manager (gcc make nasm).
#
#   tools/build-ffmpeg.sh [output folder]      (default: ./ffmpeg-slim)
#
# What goes in (everything else is left out):
#   video  VP8, VP9 (.webm)
#   audio  Vorbis, Opus                     (the sound inside videos, and .opus sound
#                                            files; other sound files are kira's)
#   files  WebM/Matroska, Ogg (for .opus), read from local files only
#   libs   avcodec avformat avutil swscale swresample (no avfilter, avdevice, programs, network)
# `quillstone check` refuses videos and sounds in other formats, so a game can't depend on them.
#
# The result is a drop-in for the full FFmpeg the player was built against, as long as
# FFMPEG_VERSION has the same major.minor (the player links functions by name).
# Package a game with:  --libs <output>/bin  (Windows)  or  --libs <output>/lib  (Linux)
set -euo pipefail

FFMPEG_VERSION=${FFMPEG_VERSION:-8.1}
# Checksum of the default version (set FFMPEG_SHA256 yourself when changing it).
[ "$FFMPEG_VERSION" = 8.1 ] && FFMPEG_SHA256=${FFMPEG_SHA256:-b072aed6871998cce9b36e7774033105ca29e33632be5b6347f3206898e0756a}
OUT=$(mkdir -p "${1:-ffmpeg-slim}" && cd "${1:-ffmpeg-slim}" && pwd)
WORK=${WORK:-$PWD/ffmpeg-slim-build}
JOBS=$(nproc 2>/dev/null || echo 4)

case "$(uname -s)" in
    MINGW*|MSYS*|UCRT*)
        os=windows
        if [ "${MSYSTEM:-}" != UCRT64 ] && [ "${MSYSTEM:-}" != MINGW64 ]; then
            echo "error: run this in the MSYS2 UCRT64 shell (MSYSTEM is '${MSYSTEM:-}')" >&2
            exit 1
        fi
        # Fold libgcc and winpthread into the DLLs, so they need nothing from MSYS2.
        extra=(--enable-w32threads --extra-ldflags=-static)
        ;;
    Linux)  os=linux; extra=(--enable-pthreads) ;;
    Darwin) os=macos; extra=(--enable-pthreads); echo "note: macOS is untested" >&2 ;;
    *) echo "error: unknown system $(uname -s)" >&2; exit 1 ;;
esac
for tool in curl make gcc nasm; do
    command -v "$tool" >/dev/null || { echo "error: '$tool' is missing (see the top of this script)" >&2; exit 1; }
done

mkdir -p "$WORK"
cd "$WORK"
fetch() { # url file expected-sha256 (kept between runs; delete the file to download it again)
    if [ ! -f "$2" ]; then
        curl -fL --retry 3 -o "$2.part" "$1"
        mv -f "$2.part" "$2"
    fi
    local sum
    sum=$(sha256sum "$2" | cut -d' ' -f1)
    if [ -z "$3" ]; then
        echo "note: no checksum to compare $2 with; it is $sum" >&2
    elif [ "$sum" != "$3" ]; then
        echo "error: $2 has checksum $sum, expected $3 (deleted; run again to re-download)" >&2
        rm -f "$2"
        exit 1
    fi
}
fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" "ffmpeg-$FFMPEG_VERSION.tar.xz" "${FFMPEG_SHA256:-}"

rm -rf "ffmpeg-$FFMPEG_VERSION"
tar xf "ffmpeg-$FFMPEG_VERSION.tar.xz"
cd "ffmpeg-$FFMPEG_VERSION"
./configure --prefix="$OUT" \
    --enable-shared --disable-static \
    --disable-everything --disable-autodetect --disable-programs --disable-doc \
    --disable-avdevice --disable-avfilter --disable-network \
    --enable-swscale --enable-swresample \
    --enable-protocol=file \
    --enable-demuxer=matroska,ogg \
    --enable-decoder=vp8,vp9,vorbis,opus \
    --enable-parser=vp8,vp9,vorbis,opus \
    "${extra[@]}"
make -j"$JOBS"
make install

# The licence players must be shown (`quillstone package` copies it into the
# game's licenses/FFmpeg.txt).
cp COPYING.LGPLv2.1 "$OUT/LICENSE.txt"

echo
if [ "$os" = windows ]; then
    libs="$OUT/bin"
    # Every DLL should need only Windows itself and each other.
    bad=$(objdump -p "$libs"/*.dll | sed -n 's/.*DLL Name: //p' | sort -u \
        | grep -v -i -E '^(av|sw)[a-z]+-[0-9]+\.dll$|^api-ms-win-|^(kernel32|user32|advapi32|bcrypt|ole32|shell32|msvcrt|ucrtbase|ws2_32|secur32|gdi32)\.dll$' || true)
    if [ -n "$bad" ]; then
        echo "warning: the DLLs need these extra files, which players won't have:" >&2
        echo "$bad" >&2
    fi
    ls -l "$libs"/*.dll
else
    libs="$OUT/lib"
    ls -l "$libs"/*.so*
fi
echo
echo "done: package games with  --libs $libs"
