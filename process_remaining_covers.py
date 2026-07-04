import sys
import os
sys.path.insert(0, os.path.dirname(__file__))

# Set stdout to utf-8
import io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

from img_to_ascii import image_to_pixels, save_json, generate_rust, JSON_PATH, RUST_PATH

pub_dir = "src/public"
all_files = os.listdir(pub_dir)

# Find the two remaining covers
love_file = next((f for f in all_files if "Love" in f), None)
beautiful_file = next((f for f in all_files if "Beautiful" in f or "Young" in f), None)

print(f"Love Yourself file: {love_file}")
print(f"Beautiful Moment file: {beautiful_file}")

albums_to_process = []
if love_file:
    albums_to_process.append((os.path.join(pub_dir, love_file), "Love Yourself \u7d50 'Answer'"))
if beautiful_file:
    albums_to_process.append((os.path.join(pub_dir, beautiful_file), "The Most Beautiful Moment in Life: Young Forever"))

W, H = 38, 76

for img_path, album_name in albums_to_process:
    print(f"Processing '{album_name}' from {img_path} at {W}x{H}...")
    rows = image_to_pixels(img_path, W, H)
    data = save_json(album_name, W, H, rows)
    print(f"  Saved to JSON: {len(data)} total albums")

rust_src = generate_rust(data)
with open(RUST_PATH, "w", encoding="utf-8") as f:
    f.write(rust_src)
print(f"Done! {len(data)} album(s) in {RUST_PATH}")
