#!/usr/bin/env python3
"""Fetch the Aurora glass typefaces into web/public/fonts and write web/src/fonts.css.

The app renders offline, so its fonts ship in the repository; this script is
how they got there and how to refresh them. It asks the Google Fonts CSS API
for exactly the families, weights and scripts the design uses, downloads each
woff2 once, copies the unicode ranges Google publishes (so a Greek face is only
ever used for Greek), and fetches each family's SIL OFL text, which the licence
requires next to distributed copies.

Run from anywhere: python3 web/scripts/fetch-fonts.py
"""

import re
import sys
import urllib.request
from pathlib import Path

# The CSS API picks a format per family from the user agent, and that choice
# is not consistent across families: a Safari 17 UA got real woff2 for Barlow,
# IBM Plex Mono and Sofia Sans but WOFF 1.0 for JetBrains Mono (checked
# 2026-09-14). A current desktop Chrome UA gets woff2 for all four families
# the design uses, with the same weights and unicode ranges either UA returns
# for the other three, so this fetches as Chrome and validates the result
# below rather than trusting the API to honour one UA consistently.
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
)

WEB_DIR = Path(__file__).resolve().parent.parent
FONTS_DIR = WEB_DIR / "public" / "fonts"
CSS_PATH = WEB_DIR / "src" / "fonts.css"

# family -> (css2 query, subsets to keep, directory under google/fonts/ofl)
FAMILIES = {
    "Barlow": ("Barlow:wght@400;500;600", {"latin"}, "barlow"),
    "IBM Plex Mono": ("IBM+Plex+Mono:wght@500", {"latin"}, "ibmplexmono"),
    "Sofia Sans": ("Sofia+Sans:wght@400;500;600;900", {"latin", "greek"}, "sofiasans"),
    "JetBrains Mono": ("JetBrains+Mono:wght@500", {"greek"}, "jetbrainsmono"),
}

FACE_PATTERN = re.compile(r"/\*\s*([a-z-]+)\s*\*/\s*@font-face\s*\{([^}]*)\}")

# The four bytes every real WOFF2 file opens with (the "wOF2" signature). A UA
# that reports "format('woff2')" but ships a WOFF 1.0 body is caught here
# rather than at load time in the app, where it would silently fall back.
WOFF2_MAGIC = b"wOF2"


def fetch(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def required(pattern: str, body: str, what: str) -> str:
    match = re.search(pattern, body)
    if match is None:
        sys.exit(f"@font-face without {what}: {body!r}")
    return match.group(1).strip()


def faces_for(family: str, query: str, subsets: set[str]) -> list[dict]:
    css = fetch(f"https://fonts.googleapis.com/css2?family={query}&display=swap").decode()

    # Variable families return the same file for every weight. Group by URL so
    # each file downloads once and declares the weight range it covers.
    by_url: dict[str, dict] = {}
    for subset, body in FACE_PATTERN.findall(css):
        if subset not in subsets:
            continue

        url = required(r"url\((https://[^)]+)\)", body, "src url")
        declared_format = required(r"format\(['\"]([^'\"]+)['\"]\)", body, "src format")
        if declared_format != "woff2":
            sys.exit(f"{family} ({subset}): Google declared format {declared_format!r}, not woff2")

        face = by_url.setdefault(
            url,
            {
                "family": family,
                "subset": subset,
                "url": url,
                "range": required(r"unicode-range:\s*([^;]+);", body, "unicode-range"),
                "weights": [],
            },
        )
        face["weights"].append(int(required(r"font-weight:\s*([^;]+);", body, "font-weight")))

    found = {face["subset"] for face in by_url.values()}
    if found != subsets:
        sys.exit(f"{family}: expected subsets {sorted(subsets)}, got {sorted(found)}")

    return list(by_url.values())


def weights(face: dict) -> list[int]:
    return sorted(set(face["weights"]))


def file_name(face: dict) -> str:
    covered = weights(face)
    weight = str(covered[0]) if len(covered) == 1 else "variable"
    return f"{face['family'].replace(' ', '')}-{weight}-{face['subset']}.woff2"


def font_face_css(face: dict) -> str:
    covered = weights(face)
    weight = str(covered[0]) if len(covered) == 1 else f"{covered[0]} {covered[-1]}"

    return (
        "@font-face {\n"
        f'  font-family: "{face["family"]}";\n'
        f'  src: url("/fonts/{file_name(face)}") format("woff2");\n'
        f"  font-weight: {weight};\n"
        "  font-style: normal;\n"
        "  font-display: swap;\n"
        f"  unicode-range: {face['range']};\n"
        "}\n"
    )


def main() -> None:
    FONTS_DIR.mkdir(parents=True, exist_ok=True)
    blocks = []

    for family, (query, subsets, ofl_dir) in FAMILIES.items():
        for face in faces_for(family, query, subsets):
            data = fetch(face["url"])
            if data[:4] != WOFF2_MAGIC:
                sys.exit(
                    f"{family} ({face['subset']}): downloaded bytes are not WOFF2, "
                    f"got magic {data[:4]!r} from {face['url']}"
                )

            (FONTS_DIR / file_name(face)).write_bytes(data)
            blocks.append(font_face_css(face))
            print(f"public/fonts/{file_name(face)}")

        licence = fetch(f"https://raw.githubusercontent.com/google/fonts/main/ofl/{ofl_dir}/OFL.txt")
        licence_name = f"OFL-{family.replace(' ', '')}.txt"
        (FONTS_DIR / licence_name).write_bytes(licence)
        print(f"public/fonts/{licence_name}")

    header = (
        "/* Generated by web/scripts/fetch-fonts.py. Do not edit by hand.\n"
        "   Barlow and IBM Plex Mono carry no Greek glyphs. The Greek faces are\n"
        "   scoped by unicode-range and sit next in the stacks in index.css. */\n\n"
    )
    CSS_PATH.write_text(header + "\n".join(blocks))
    print(f"src/{CSS_PATH.name}")


if __name__ == "__main__":
    main()
