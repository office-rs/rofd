# Phase-3 acceptance demo, Python side (convert spec §9 ③): register
# font -> pooled concurrent converts -> byte-identical to the
# rofd-convert CLI output produced on the same machine with the same
# font state. Plain script (no test framework) — run:
#   py -3.1 crates/convert-py/demo/run.py
"""rofd-convert phase-3 acceptance demo (pooled == CLI bytes)."""

import hashlib
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from threading import local

ROOT = Path(__file__).resolve().parents[3]
DOC = ROOT / "test" / "ru-yuan-ji-lu.ofd"
CLI = ROOT / "target" / "release" / "rofd-convert.exe"
FONT = Path("C:/Windows/Fonts/simsun.ttc")
CLI_PDF = ROOT / "target" / "demo-py-cli.pdf"


def fail(message: str) -> None:
    print(f"FAIL: {message}", file=sys.stderr)
    sys.exit(1)


def main() -> None:
    if not DOC.exists():
        fail(f"missing fixture {DOC}")
    if not FONT.exists():
        fail(
            "missing font C:/Windows/Fonts/simsun.ttc "
            "(no silent substitution: the demo compares against a CLI run "
            "with this exact font)"
        )

    import rofd_convert

    # CLI baseline via the Rust binary (same machine, same font state).
    cli = subprocess.run(
        [str(CLI), str(DOC), str(CLI_PDF), "--font", str(FONT)],
        stderr=subprocess.DEVNULL,
    )
    if cli.returncode != 0:
        fail(f"CLI exited {cli.returncode}")
    cli_pdf = CLI_PDF.read_bytes()

    # Python side: register -> one Converter per thread (threading.local,
    # spec D15-safe shape) -> 8 concurrent converts must all match.
    if not rofd_convert.register_font_file(str(FONT)):
        fail("font registration returned False")
    print(f"font registered: {FONT}")

    state = local()

    def convert_one(_: int) -> bytes:
        if not hasattr(state, "converter"):
            state.converter = rofd_convert.Converter()
        pdf, _warnings = state.converter.convert(DOC.read_bytes())
        return pdf

    with ThreadPoolExecutor(max_workers=8) as pool:
        outputs = list(pool.map(convert_one, range(8)))

    for i, out in enumerate(outputs):
        if out != cli_pdf:
            fail(f"worker output {i} differs from CLI baseline")

    digest = hashlib.sha256(cli_pdf).hexdigest()
    print(f"hash: {digest}")
    print(f"bytes: {len(cli_pdf)}")
    print("workers: 8/8 BYTES MATCH CLI")


if __name__ == "__main__":
    main()
