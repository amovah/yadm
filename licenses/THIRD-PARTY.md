# Third-party software bundled with YADM

YADM itself is MIT licensed. Release builds ship these separate programs next
to the YADM executable. They run as their own processes and keep their own
licenses.

| Program | License | Source |
|---|---|---|
| aria2 (`aria2c`) | GPL-2.0-or-later with an OpenSSL exception (`GPL-2.0.txt`) | https://github.com/aria2/aria2 |
| yt-dlp | The Unlicense (`yt-dlp-LICENSE.txt`) | https://github.com/yt-dlp/yt-dlp |
| Deno | MIT (`deno-LICENSE.md`) | https://github.com/denoland/deno |
| FFmpeg (`ffmpeg`, `ffprobe`) | LGPL-2.1-or-later (`LGPL-2.1.txt`) | https://ffmpeg.org/download.html |
| LAME (inside FFmpeg) | LGPL-2.0-or-later (`LGPL-2.1.txt`) | https://lame.sourceforge.io |

## FFmpeg build

On macOS and Linux, FFmpeg is built by `scripts/build-ffmpeg.sh` from the
unmodified FFmpeg and LAME release tarballs. The script holds the exact versions
and `configure` flags. The build is LGPL only: no GPL or nonfree components are
enabled.

On Windows, YADM ships the LGPL build from https://github.com/BtbN/FFmpeg-Builds
(the `win64-lgpl` variant). Its source and build scripts are published there.

To use a different FFmpeg, point Settings → Video → ffmpeg at it, or replace the
bundled `ffmpeg` and `ffprobe` executables.
