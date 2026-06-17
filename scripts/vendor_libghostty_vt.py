from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import tarfile
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path


@dataclass
class VendorMetadata:
    source_repo: str
    source_commit: str
    dist_archive: str
    extracted_dir: str


def parse_archive_root(archive: Path) -> str:
    with tarfile.open(archive, "r:gz") as tar:
        roots = {
            member.name.split("/", 1)[0]
            for member in tar.getmembers()
            if member.name and member.name != "."
        }
    if len(roots) != 1:
        raise ValueError(f"expected exactly one archive root in {archive}, found {sorted(roots)}")
    return next(iter(roots))


def git_head(repo: Path) -> str:
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip()


def ensure_dist_archive(source_repo: Path) -> Path:
    head = git_head(source_repo)[:9]
    subprocess.run(
        ["zig", "build", "dist", "-Demit-lib-vt", "-Doptimize=ReleaseFast"],
        cwd=source_repo,
        check=True,
    )
    dist_dir = source_repo / "zig-out" / "dist"
    archives = sorted(dist_dir.glob(f"libghostty-vt-*+{head}.tar.gz"))
    if not archives:
        raise FileNotFoundError(
            f"no libghostty-vt dist archive for HEAD {head} found in {dist_dir}"
        )
    return archives[-1]


def zig_global_cache_dir() -> Path:
    """Return zig's global package cache directory via `zig env`.

    `zig env` emits ZON (`.{ .global_cache_dir = "..." }`) as of zig 0.15, not
    JSON, so parse the field directly rather than with a JSON/ZON loader.
    """
    out = subprocess.check_output(["zig", "env"], text=True)
    match = re.search(r'\.global_cache_dir\s*=\s*"((?:[^"\\]|\\.)*)"', out)
    if not match:
        raise ValueError(f"could not find global_cache_dir in `zig env` output:\n{out}")
    # Unescape ZON string escapes (e.g. \" and \\) that may appear in paths.
    return Path(match.group(1).encode().decode("unicode_escape"))


def localize_wuffs(destination: Path) -> None:
    """Replace the fetched-from-URL `wuffs` dependency with a local path dep.

    The upstream wuffs tarball ships deliberately malformed image fixtures
    under `test/` that an on-access antivirus (e.g. Bitdefender) flags and
    blocks reads of, which breaks the build on developer machines. The only
    file this package actually compiles is the single `release/c/wuffs-vX.c`
    amalgamation, so we vendor just that file as `pkg/wuffs/upstream` and
    rewrite the dependency to point at it. This keeps the malformed fixtures
    off the machine entirely (they are never fetched).

    Idempotent and version-tracking: the compiled `wuffs-vX.c` filename is
    read from `pkg/wuffs/build.zig` and the matching source is copied out of
    zig's global package cache (populated by `zig build dist` above), so a
    wuffs version bump upstream is picked up automatically.
    """
    wuffs_pkg = destination / "pkg" / "wuffs"
    build_zig = wuffs_pkg / "build.zig"
    zon = wuffs_pkg / "build.zig.zon"
    if not zon.exists():
        raise FileNotFoundError(f"expected wuffs build.zig.zon at {zon}")

    # Which amalgamation does build.zig compile? e.g. release/c/wuffs-v0.4.c
    source_rel_match = re.search(
        r'release/c/(wuffs-v[\d.]+\.c)', build_zig.read_text()
    )
    if not source_rel_match:
        raise ValueError(f"could not find wuffs source file reference in {build_zig}")
    source_filename = source_rel_match.group(1)

    zon_text = zon.read_text()

    # Already localized (e.g. re-run): nothing fetched, nothing to do beyond
    # ensuring the vendored source is present.
    if ".path = \"upstream\"" in zon_text and (
        wuffs_pkg / "upstream" / "release" / "c" / source_filename
    ).exists():
        return

    # Extract the upstream wuffs package hash so we can find it in the cache.
    hash_match = re.search(
        r'\.wuffs\s*=\s*\.\{[^}]*?\.hash\s*=\s*"([^"]+)"',
        zon_text,
        re.DOTALL,
    )
    if not hash_match:
        raise ValueError(
            f"could not find wuffs dependency hash in {zon} "
            "(is the dependency already localized without a vendored source?)"
        )
    wuffs_hash = hash_match.group(1)

    cached = zig_global_cache_dir() / "p" / wuffs_hash / "release" / "c"
    cached_source = cached / source_filename
    if not cached_source.exists():
        raise FileNotFoundError(
            f"wuffs source {cached_source} not found in zig cache; "
            "run `zig build dist` first so the dependency is fetched"
        )

    upstream_c = wuffs_pkg / "upstream" / "release" / "c"
    if upstream_c.parent.parent.exists():
        shutil.rmtree(upstream_c.parent.parent)
    upstream_c.mkdir(parents=True)
    shutil.copy2(cached_source, upstream_c / source_filename)
    readme = cached / "README.md"
    if readme.exists():
        shutil.copy2(readme, upstream_c / "README.md")

    # Rewrite the wuffs dependency (URL + hash + lazy) to a local path dep.
    new_dep = (
        "// google/wuffs\n"
        "        //\n"
        "        // Vendored locally as `./upstream` (only the single\n"
        "        // `release/c/" + source_filename + "` amalgamation this package\n"
        "        // compiles) instead of fetching the full upstream tarball, which\n"
        "        // ships deliberately malformed image fixtures under `test/` that\n"
        "        // an on-access antivirus (Bitdefender) flags and blocks reads of,\n"
        "        // breaking the build. None of those fixtures are used here.\n"
        "        // Re-applied automatically by scripts/vendor_libghostty_vt.py.\n"
        "        .wuffs = .{\n"
        "            .path = \"upstream\",\n"
        "        },"
    )
    new_zon = re.sub(
        r'//\s*google/wuffs\s*\n\s*\.wuffs\s*=\s*\.\{.*?\},',
        new_dep,
        zon_text,
        count=1,
        flags=re.DOTALL,
    )
    if new_zon == zon_text:
        raise ValueError(f"failed to rewrite wuffs dependency block in {zon}")
    zon.write_text(new_zon)


def vendor_libghostty_vt(source_repo: Path, destination: Path) -> VendorMetadata:
    archive = ensure_dist_archive(source_repo)
    root = parse_archive_root(archive)

    with tempfile.TemporaryDirectory() as temp_dir:
        temp_dir_path = Path(temp_dir)
        with tarfile.open(archive, "r:gz") as tar:
            tar.extractall(temp_dir_path)

        extracted = temp_dir_path / root
        if not extracted.exists():
            raise FileNotFoundError(f"expected extracted root {extracted}")

        if destination.exists():
            shutil.rmtree(destination)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(extracted, destination)

    localize_wuffs(destination)

    return VendorMetadata(
        source_repo=str(source_repo),
        source_commit=git_head(source_repo),
        dist_archive=archive.name,
        extracted_dir=root,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Vendor the pinned libghostty-vt source dist into herdr")
    parser.add_argument(
        "--source-repo",
        default="/home/can/Projects/ghostty",
        help="Path to a local ghostty checkout",
    )
    parser.add_argument(
        "--destination",
        default="vendor/libghostty-vt",
        help="Destination directory for the extracted libghostty-vt source dist",
    )
    parser.add_argument(
        "--metadata",
        default="vendor/libghostty-vt.vendor.json",
        help="Path to write vendoring metadata JSON",
    )
    args = parser.parse_args()

    repo = Path(args.source_repo).resolve()
    destination = Path(args.destination).resolve()
    metadata_path = Path(args.metadata).resolve()

    metadata = vendor_libghostty_vt(repo, destination)
    metadata_path.parent.mkdir(parents=True, exist_ok=True)
    metadata_path.write_text(json.dumps(asdict(metadata), indent=2) + "\n")

    print(f"vendored {metadata.extracted_dir} from {metadata.source_commit} into {destination}")


if __name__ == "__main__":
    main()
