#!/usr/bin/env python3
"""Assemble the PyPI wheels from built binaries.

    scripts/build-wheels.py <out-dir> [target ...]

Each target is the same "<platform>-<arch>" name build-npm.sh uses, and the
binary for a target is read from target/<rust-triple>/release/agent-sync, so a
release packs the *identical* binary that the tarball and the npm package ship.
Nothing is compiled here.

There is no pyproject.toml and no maturin on purpose: maturin would recompile
the crate in a second six-target matrix rather than reuse the artifacts the
build job already produced. A wheel is a zip with a prescribed layout, so it is
cheaper to write that layout than to build everything twice.

The binary goes in <name>-<version>.data/scripts/, which pip installs into the
environment's bin/ (Scripts/ on Windows) and marks executable — that is what
makes `pip install agent-sync-sh` and `uvx --from agent-sync-sh agent-sync` put
a working `agent-sync` on
PATH without any Python entry-point shim in between.
"""

import base64
import hashlib
import os
import re
import stat
import subprocess
import sys
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Rust triple and the wheel platform tag for each target. The tag is the part
# pip matches against; get it wrong and pip reports "no matching distribution"
# rather than anything that points at the cause. The two-tag Linux values are
# the compressed tag sets auditwheel emits, so older pips that only know
# manylinux2014 still match.
TARGETS = {
    "darwin-arm64": ("aarch64-apple-darwin", "macosx_11_0_arm64"),
    "darwin-x64": ("x86_64-apple-darwin", "macosx_10_12_x86_64"),
    "linux-arm64": ("aarch64-unknown-linux-gnu", "manylinux_2_17_aarch64.manylinux2014_aarch64"),
    "linux-x64": ("x86_64-unknown-linux-gnu", "manylinux_2_17_x86_64.manylinux2014_x86_64"),
    "win32-arm64": ("aarch64-pc-windows-msvc", "win_arm64"),
    "win32-x64": ("x86_64-pc-windows-msvc", "win_amd64"),
}

SUMMARY = (
    "Canonical configs, fanned out to all your AI coding agents from the "
    "Commons, the canonical ~/.agents directory"
)

CLASSIFIERS = [
    "Development Status :: 5 - Production/Stable",
    "Environment :: Console",
    "Intended Audience :: Developers",
    "License :: OSI Approved :: MIT License",
    "Operating System :: MacOS",
    "Operating System :: Microsoft :: Windows",
    "Operating System :: POSIX :: Linux",
    "Programming Language :: Rust",
    "Topic :: Software Development :: Build Tools",
    "Topic :: Utilities",
]


def cargo_version():
    with open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8") as fh:
        for line in fh:
            m = re.match(r'^version = "(.*)"', line)
            if m:
                return m.group(1)
    sys.exit("no version in Cargo.toml")


def host_target():
    """This machine's target name, or None if it is not one we ship."""
    import platform

    arch = {
        "arm64": "arm64",
        "aarch64": "arm64",
        "x86_64": "x64",
        "amd64": "x64",
    }.get(platform.machine().lower())
    if sys.platform == "darwin":
        plat = "darwin"
    elif sys.platform.startswith("linux"):
        plat = "linux"
    elif sys.platform in ("win32", "cygwin"):
        plat = "win32"
    else:
        return None
    return f"{plat}-{arch}" if arch else None


def binary_for(target):
    """The built binary for a target, or None when it was not built."""
    triple, _ = TARGETS[target]
    exe = ".exe" if target.startswith("win32-") else ""
    path = os.path.join(ROOT, "target", triple, "release", "agent-sync" + exe)
    if os.path.exists(path):
        return path
    # A plain `cargo build --release` writes the host binary untripled. Accept
    # it so a single-target local run works without cross-compiling — but only
    # for the host's own target. Without that guard a local build would happily
    # seal a macOS binary into a linux wheel, and a wheel published to PyPI
    # cannot be replaced, only yanked.
    if target != host_target():
        return None
    host = os.path.join(ROOT, "target", "release", "agent-sync" + exe)
    return host if os.path.exists(host) else None


def metadata(version):
    with open(os.path.join(ROOT, "README.md"), encoding="utf-8") as fh:
        readme = fh.read()
    lines = [
        "Metadata-Version: 2.1",
        "Name: agent-sync-sh",
        f"Version: {version}",
        f"Summary: {SUMMARY}",
        "License: MIT",
        "Requires-Python: >=3.8",
        "Project-URL: Homepage, https://agent-sync.sh",
        "Project-URL: Repository, https://github.com/agent-sync-sh/agent-sync",
        "Description-Content-Type: text/markdown",
    ]
    lines += [f"Classifier: {c}" for c in CLASSIFIERS]
    return "\n".join(lines) + "\n\n" + readme


def record_line(path, data):
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode()
    return f"{path},sha256={digest},{len(data)}"


def build(target, version, outdir):
    binary = binary_for(target)
    if binary is None:
        return None
    _, platform_tag = TARGETS[target]
    exe = ".exe" if target.startswith("win32-") else ""
    distinfo = f"agent_sync_sh-{version}.dist-info"
    scripts = f"agent_sync_sh-{version}.data/scripts"

    # A stale target/release/ is the one way this script can label a wheel with
    # a version its binary does not carry, and the manual-upload path in the
    # runbook would then put that on PyPI, where it can be yanked but never
    # replaced. Only the host's own binary can be run to check, which is
    # exactly the case the manual path uses.
    if target == host_target():
        try:
            reported = subprocess.run(
                [binary, "--version"], capture_output=True, text=True, timeout=30
            ).stdout.strip()
        except (OSError, subprocess.SubprocessError):
            reported = ""
        if reported and reported != f"agent-sync {version}":
            sys.exit(
                f"{binary} reports {reported!r}, but Cargo.toml says {version} — "
                "rebuild with `cargo build --release` before packaging"
            )

    with open(binary, "rb") as fh:
        binary_bytes = fh.read()

    # One Tag: line per tag in the compressed set, which is what the compound
    # platform tag in the filename expands to.
    tags = "\n".join(f"Tag: py3-none-{t}" for t in platform_tag.split("."))
    files = [
        (f"{scripts}/agent-sync{exe}", binary_bytes, True),
        (f"{distinfo}/METADATA", metadata(version).encode("utf-8"), False),
        (
            f"{distinfo}/WHEEL",
            (
                "Wheel-Version: 1.0\n"
                "Generator: agent-sync build-wheels.py\n"
                "Root-Is-Purelib: false\n" + tags + "\n"
            ).encode("utf-8"),
            False,
        ),
    ]

    record = [record_line(name, data) for name, data, _ in files]
    record.append(f"{distinfo}/RECORD,,")
    files.append((f"{distinfo}/RECORD", ("\n".join(record) + "\n").encode("utf-8"), False))

    name = f"agent_sync_sh-{version}-py3-none-{platform_tag}.whl"
    write_wheel(os.path.join(outdir, name), files)
    return name


def write_wheel(path, files):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for entry, data, executable in files:
            # A fixed timestamp keeps the wheel byte-identical across rebuilds
            # of the same binary; zip has no notion of "no timestamp".
            info = zipfile.ZipInfo(entry, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            # S_IFREG is not decoration. pip decides whether to install a file
            # executable with `stat.S_ISREG(mode) and mode & 0o111`, so a bare
            # 0o755 here fails S_ISREG and pip writes a non-executable binary
            # to the venv's bin/ — an `agent-sync` on PATH that cannot run.
            mode = stat.S_IFREG | (0o755 if executable else 0o644)
            info.external_attr = mode << 16
            zf.writestr(info, data)


FALLBACK_MODULE = '''\
"""Stand-in for platforms with no prebuilt agent-sync binary.

Ships only in the py3-none-any wheel. pip prefers a platform wheel whenever
one matches, so this is reached only where none does.
"""

import platform
import sys

SUPPORTED = (
    "macOS arm64 and x86_64, Linux aarch64 and x86_64 (manylinux 2.17+), "
    "and Windows x64 and arm64"
)


def main():
    sys.stderr.write(
        f"agent-sync: no prebuilt binary for {sys.platform}-{platform.machine()}.\\n"
        "This is the fallback wheel — pip installs it only when no platform "
        "wheel matches your machine.\\n"
        f"Prebuilt wheels exist for {SUPPORTED}.\\n"
        "To build from source instead: cargo install agent-sync-sh\\n"
        "Details: https://agent-sync.sh\\n"
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
'''


def build_fallback(version, outdir):
    """The py3-none-any wheel: no binary, just a command that explains itself.

    Without it, `pip install agent-sync-sh` on an unsupported platform fails with
    pip's generic "no matching distribution found", which names neither the
    platform nor a way forward. The npm launcher already behaves the way this
    does — install succeeds, running it says what it looked for and points at
    `cargo install` — so this keeps the two channels honest with each other.
    A platform tag always outranks `any` in pip's preference order, so this
    can never shadow a real wheel.
    """
    distinfo = f"agent_sync_sh-{version}.dist-info"
    files = [
        ("agent_sync_unsupported.py", FALLBACK_MODULE.encode("utf-8"), False),
        (f"{distinfo}/METADATA", metadata(version).encode("utf-8"), False),
        (
            f"{distinfo}/WHEEL",
            (
                "Wheel-Version: 1.0\n"
                "Generator: agent-sync build-wheels.py\n"
                "Root-Is-Purelib: true\n"
                "Tag: py3-none-any\n"
            ).encode("utf-8"),
            False,
        ),
        (
            f"{distinfo}/entry_points.txt",
            b"[console_scripts]\nagent-sync = agent_sync_unsupported:main\n",
            False,
        ),
    ]
    record = [record_line(name, data) for name, data, _ in files]
    record.append(f"{distinfo}/RECORD,,")
    files.append((f"{distinfo}/RECORD", ("\n".join(record) + "\n").encode("utf-8"), False))

    name = f"agent_sync_sh-{version}-py3-none-any.whl"
    write_wheel(os.path.join(outdir, name), files)
    return name


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: scripts/build-wheels.py <out-dir> [target ...]")
    outdir = sys.argv[1]
    requested = sys.argv[2:] or list(TARGETS)
    for target in requested:
        if target not in TARGETS:
            sys.exit(f"unknown target {target}; known: {' '.join(TARGETS)}")

    os.makedirs(outdir, exist_ok=True)
    version = cargo_version()
    built = []
    for target in requested:
        name = build(target, version, outdir)
        if name is None:
            print(f"  {target}: no binary, skipped")
        else:
            built.append(name)
            print(f"  {target}: {name}")
    if not built:
        sys.exit("no wheels built — is anything in target/<triple>/release?")

    # Always emitted: it needs no binary, and a release that shipped platform
    # wheels without it would regress unsupported platforms back to pip's
    # bare "no matching distribution found".
    built.append(build_fallback(version, outdir))
    print(f"  fallback: {built[-1]}")
    print(f"{len(built)} wheel(s) in {outdir}")


if __name__ == "__main__":
    main()
