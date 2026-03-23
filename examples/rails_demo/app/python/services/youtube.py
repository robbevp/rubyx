"""YouTube video downloader using yt-dlp.

Install: uv sync (included in base dependencies)
Docs: https://github.com/yt-dlp/yt-dlp

Usage from Ruby:
    yt = Rubyx.import('services.youtube')
    info = yt.video_info("https://youtube.com/watch?v=dQw4w9WgXcQ")
    puts info.to_ruby

    yt.download("https://youtube.com/watch?v=...", "/tmp/videos")
"""

import yt_dlp
import os


def video_info(url):
    """Get video metadata without downloading.

    Returns:
        dict with title, duration, view_count, description, thumbnail, etc.
    """
    opts = {
        "quiet": True,
        "no_warnings": True,
        "extract_flat": False,
    }
    with yt_dlp.YoutubeDL(opts) as ydl:
        info = ydl.extract_info(url, download=False)
        return {
            "title": info.get("title"),
            "duration": info.get("duration"),
            "view_count": info.get("view_count"),
            "like_count": info.get("like_count"),
            "description": (info.get("description") or "")[:500],
            "thumbnail": info.get("thumbnail"),
            "uploader": info.get("uploader"),
            "upload_date": info.get("upload_date"),
            "formats_available": len(info.get("formats", [])),
        }


def download(url, output_dir="/tmp/rubyx_downloads", format="bestaudio[ext=m4a]/bestaudio/best"):
    """Download a video/audio.

    Args:
        url: YouTube URL
        output_dir: Directory to save to
        format: yt-dlp format string (default: best audio)

    Returns:
        dict with filename, filesize, title
    """
    os.makedirs(output_dir, exist_ok=True)
    result = {}

    opts = {
        "format": format,
        "outtmpl": os.path.join(output_dir, "%(title)s.%(ext)s"),
        "quiet": True,
        "no_warnings": True,
    }

    with yt_dlp.YoutubeDL(opts) as ydl:
        info = ydl.extract_info(url, download=True)
        filename = ydl.prepare_filename(info)
        result = {
            "title": info.get("title"),
            "filename": filename,
            "filesize": os.path.getsize(filename) if os.path.exists(filename) else None,
            "duration": info.get("duration"),
            "ext": info.get("ext"),
        }

    return result


def download_with_progress(url, output_dir="/tmp/rubyx_downloads", format="bestaudio[ext=m4a]/bestaudio/best"):
    """Download with progress streaming — yields progress updates.

    Usage from Ruby:
        Rubyx.stream(yt.download_with_progress(url)).each do |update|
          puts update  # "downloading 45.2%", "finished /tmp/file.m4a"
        end
    """
    os.makedirs(output_dir, exist_ok=True)
    progress_data = {"status": "starting"}

    def hook(d):
        if d["status"] == "downloading":
            pct = d.get("_percent_str", "?%").strip()
            speed = d.get("_speed_str", "?")
            progress_data["status"] = f"downloading {pct} ({speed})"
        elif d["status"] == "finished":
            progress_data["status"] = f"finished {d.get('filename', '')}"

    opts = {
        "format": format,
        "outtmpl": os.path.join(output_dir, "%(title)s.%(ext)s"),
        "quiet": True,
        "no_warnings": True,
        "progress_hooks": [hook],
    }

    # yt-dlp is sync, so we run it and yield progress snapshots
    import threading
    import time

    done = False

    def _download():
        nonlocal done
        with yt_dlp.YoutubeDL(opts) as ydl:
            ydl.extract_info(url, download=True)
        done = True

    thread = threading.Thread(target=_download)
    thread.start()

    last = None
    while not done:
        msg = progress_data["status"]
        if msg != last:
            yield msg
            last = msg
        time.sleep(0.1)

    # Final status
    yield progress_data["status"]
    thread.join()


def list_formats(url):
    """List all available formats for a video.

    Returns:
        list of dicts with format_id, ext, resolution, filesize, etc.
    """
    opts = {
        "quiet": True,
        "no_warnings": True,
    }
    with yt_dlp.YoutubeDL(opts) as ydl:
        info = ydl.extract_info(url, download=False)
        formats = []
        for f in info.get("formats", []):
            formats.append({
                "format_id": f.get("format_id"),
                "ext": f.get("ext"),
                "resolution": f.get("resolution"),
                "filesize": f.get("filesize"),
                "vcodec": f.get("vcodec"),
                "acodec": f.get("acodec"),
                "note": f.get("format_note"),
            })
        return formats
