# rofd-convert-py

Headless OFD (GB/T 33190) to PDF conversion — PyO3 bindings over the
`rofd-convert` kernel (convert spec §6.6).

## Install (wheel, abi3-py39+)

```bash
pip install rofd-convert
```

Dev build from the repository root:

```bash
py -3.1 -m maturin build --release -m crates/convert-py/Cargo.toml -o target/wheels
py -3.1 -m pip install --force-reinstall target/wheels/rofd_convert-*.whl
```

## Usage

```python
from rofd_convert import Converter, register_font_file

# Once per process, BEFORE the first Converter (registry snapshot).
# Real documents declaring non-embedded fonts need a registered font.
register_font_file("simsun.ttc")

c = Converter()
pdf, warnings = c.convert(ofd_bytes)
# Content flags are all on by default; flip per call:
pdf2, _ = c.convert(ofd_bytes, watermark=False, max_pages=50)
```

`convert` releases the GIL during the kernel call; one instance serves
one call at a time (a concurrent call raises `ConvertError("converter
in use")`). For throughput, give each thread its own instance
(`threading.local`) — `demo/run.py` shows the ThreadPoolExecutor recipe.

## Acceptance

```bash
py -3.1 crates/convert-py/demo/run.py   # pooled converts == CLI bytes
```
