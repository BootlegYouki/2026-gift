import os
import json
import subprocess
import sys

# Configure stdout and stderr to support UTF-8 encoding (prevents charmap crash on Windows)
if sys.stdout.encoding != 'utf-8':
    try:
        sys.stdout.reconfigure(encoding='utf-8')
    except AttributeError:
        pass
if sys.stderr.encoding != 'utf-8':
    try:
        sys.stderr.reconfigure(encoding='utf-8')
    except AttributeError:
        pass

def install_package(package):
    try:
        subprocess.check_call([sys.executable, "-m", "pip", "install", package])
    except Exception as e:
        print(f"Failed to install package {package}: {e}")

try:
    import yt_dlp
except ImportError:
    print("Installing yt-dlp...")
    install_package("yt-dlp")
    import yt_dlp

import shutil
import urllib.request
import zipfile
import io

def download_ffmpeg():
    print("FFmpeg not found. Downloading static binaries for Windows...")
    url = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip"
    try:
        req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
        with urllib.request.urlopen(req) as response:
            zip_data = response.read()
        print("Extracting FFmpeg binaries...")
        with zipfile.ZipFile(io.BytesIO(zip_data)) as z:
            for member in z.namelist():
                if member.endswith("ffmpeg.exe") or member.endswith("ffprobe.exe"):
                    filename = os.path.basename(member)
                    with z.open(member) as source, open(filename, "wb") as target:
                        shutil.copyfileobj(source, target)
        print("FFmpeg and FFprobe successfully downloaded and extracted.")
        return True
    except Exception as e:
        print(f"Failed to automatically download FFmpeg: {e}")
        return False

def download_tracks():
    json_path = os.path.join("src", "public", "albums.json")
    if not os.path.exists(json_path):
        print(f"Error: Could not find albums.json at {json_path}")
        return

    with open(json_path, "r", encoding="utf-8") as f:
        albums = json.load(f)

    print(f"Loaded {len(albums)} albums from JSON.")

    # Check if FFmpeg is installed/present
    has_ffmpeg = shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None
    if not has_ffmpeg:
        has_ffmpeg = download_ffmpeg()

    if has_ffmpeg:
        print("FFmpeg and FFprobe ready. Downloading and converting tracks to MP3.")
    else:
        print("Warning: Falling back to raw audio formats directly (no MP3 conversion).")

    for album in albums:
        album_title = album.get("album_title", "Unknown Album")
        print(f"\nProcessing Album: {album_title}")
        
        # Create output directory for this album (cleaning illegal path chars)
        clean_album_title = album_title
        for char in ['<', '>', ':', '"', '/', '\\', '|', '?', '*']:
            clean_album_title = clean_album_title.replace(char, '')
        
        output_dir = os.path.join("downloads", clean_album_title)
        os.makedirs(output_dir, exist_ok=True)

        for track in album.get("tracks", []):
            track_num = track.get("track_number")
            track_name = track.get("track_name")
            
            filename_template = f"{track_num:02d} - {track_name}"
            # Clean filename
            for char in ['<', '>', ':', '"', '/', '\\', '|', '?', '*']:
                filename_template = filename_template.replace(char, '')

            # Check if track already exists with any typical audio extension
            already_exists = False
            for ext in ['.mp3', '.webm', '.m4a', '.opus', '.ogg', '.wav']:
                if os.path.exists(os.path.join(output_dir, f"{filename_template}{ext}")):
                    already_exists = True
                    break

            if already_exists:
                print(f"  [Skipped] {track_name} already exists.")
                continue

            print(f"  [Downloading] Track {track_num}: {track_name}...")
            
            # Formulate search query
            query = f"ytsearch:BTS {track_name} audio"

            if has_ffmpeg:
                ydl_opts = {
                    'format': 'bestaudio/best',
                    'outtmpl': os.path.join(output_dir, filename_template),
                    'postprocessors': [{
                        'key': 'FFmpegExtractAudio',
                        'preferredcodec': 'mp3',
                        'preferredquality': '192',
                    }],
                    'quiet': False,
                }
            else:
                ydl_opts = {
                    'format': 'bestaudio/best',
                    'outtmpl': os.path.join(output_dir, f"{filename_template}.%(ext)s"),
                    'quiet': False,
                }

            try:
                with yt_dlp.YoutubeDL(ydl_opts) as ydl:
                    ydl.download([query])
                print(f"    Success!")
            except Exception as e:
                print(f"    Failed to download {track_name}: {e}")

if __name__ == "__main__":
    download_tracks()
