#!/usr/bin/env python3
"""Regenerate data/wpt-spec-meta.json.

Maps each WPT test directory (area) that has a META.yml with a `spec:` URL to
a human-readable spec title and link, joining:
  - META.yml files from the web-platform-tests/wpt repository
  - spec titles from the w3c/browser-specs registry

Usage: scripts/update-wpt-spec-meta.py
"""

import glob
import json
import os
import re
import subprocess
import tempfile
import urllib.request

WPT_REPO_URL = "https://github.com/web-platform-tests/wpt.git"
BROWSER_SPECS_URL = "https://raw.githubusercontent.com/w3c/browser-specs/main/index.json"

repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
out_file = os.path.join(repo_root, "data", "wpt-spec-meta.json")


def fetch(url):
    req = urllib.request.Request(url, headers={"User-Agent": "blitz-website-spec-meta"})
    with urllib.request.urlopen(req) as resp:
        return resp.read().decode("utf-8")


def norm_url(url):
    return re.sub(r"^http:", "https:", url.split("#")[0].rstrip("/"))


def main():
    print("Fetching browser-specs registry ...")
    specs = json.loads(fetch(BROWSER_SPECS_URL))
    title_by_url = {}
    for spec in specs:
        series = spec.get("series", {})
        title = series.get("shortTitle") or series.get("title") or spec.get("shortTitle") or spec["title"]
        urls = {
            spec.get("url", ""),
            (spec.get("nightly") or {}).get("url", ""),
            series.get("nightlyUrl", ""),
            series.get("releaseUrl", ""),
        }
        for url in urls:
            if url:
                title_by_url.setdefault(norm_url(url), title)

    print("Fetching META.yml files from web-platform-tests/wpt ...")
    with tempfile.TemporaryDirectory() as tmp:
        # Shallow blobless sparse checkout: downloads only the tree
        # objects plus the META.yml blobs
        subprocess.run(
            ["git", "clone", "--depth=1", "--filter=blob:none", "--sparse", WPT_REPO_URL, tmp],
            check=True,
        )
        subprocess.run(
            ["git", "-C", tmp, "sparse-checkout", "set", "--no-cone", "**/META.yml"],
            check=True,
        )
        results = []
        for path in glob.glob(os.path.join(tmp, "**/META.yml"), recursive=True):
            content = open(path).read()
            match = re.search(r"^spec:\s*(\S+)", content, re.MULTILINE)
            results.append((os.path.relpath(path, tmp), match.group(1) if match else None))

    out = {}
    for path, url in sorted(results):
        if not url:
            continue
        area = os.path.dirname(path)
        if not area:
            continue
        entry = {"spec": url}
        title = title_by_url.get(norm_url(url))
        if title:
            entry["title"] = title
        out[area] = entry

    with open(out_file, "w") as f:
        json.dump(out, f, indent=1, sort_keys=True)
        f.write("\n")
    titled = sum(1 for entry in out.values() if "title" in entry)
    print(f"Wrote {len(out)} areas ({titled} with titles) to {out_file}")


if __name__ == "__main__":
    main()
