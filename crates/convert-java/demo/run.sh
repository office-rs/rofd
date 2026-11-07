#!/usr/bin/env bash
# Two-act acceptance demo (phase-2 gate): Java pooled conversions must be
# byte-identical to the rofd-convert CLI output on the same machine with
# the same font state. Run from the repo root:
#
#   bash crates/convert-java/demo/run.sh
#
# ACT 1 (library-path tier): the demo runs against the thin jar with
#   -Djava.library.path=target/release — NativeLoader's System.loadLibrary
#   fallback tier.
# ACT 2 (zero-config fat jar): mvn repackages the jar with the native
#   embedded (natives/<platform>/<lib>), then the same demo runs with NO
#   library path or property — NativeLoader's classpath-resource tier.
#   Skipped with exit 0 when mvn is absent; act 1 always runs.
#
# The font is fixed to SimSun (C:/Windows/Fonts/simsun.ttc) and MUST
# exist: silently picking another font would compare two different font
# states and void the byte-identity assertion.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"          # crates/convert-java/demo
CRATE="$(cd "$HERE/.." && pwd)"                # crates/convert-java
ROOT="$(cd "$CRATE/../.." && pwd)"             # workspace root

FIXTURE="$ROOT/test/ru-yuan-ji-lu.ofd"
FONT="C:/Windows/Fonts/simsun.ttc"

# --- preconditions ----------------------------------------------------
[ -f "$FIXTURE" ] || { echo "FAIL: missing fixture $FIXTURE (local-only real sample)"; exit 1; }
[ -f "$FONT" ] || { echo "FAIL: missing font $FONT — refusing to substitute another font: byte identity is only meaningful when both sides run on the same font state"; exit 1; }

echo "=== building release CLI + JNI dll ==="
cargo build -p rofd-convert-cli -p rofd-convert-java --release

# cargo names the CLI bin rofd-convert; Windows appends .exe.
CLI_BIN="$ROOT/target/release/rofd-convert.exe"
[ -f "$CLI_BIN" ] || CLI_BIN="$ROOT/target/release/rofd-convert"
[ -f "$CLI_BIN" ] || { echo "FAIL: rofd-convert binary not found in $ROOT/target/release"; exit 1; }

case "$(uname -s):$(uname -m)" in
  Linux*:*)            PLATFORM=linux-x86_64;   DLL=librofdconvert.so ;;
  Darwin*:arm64)       PLATFORM=macos-aarch64;  DLL=librofdconvert.dylib ;;
  Darwin*:*)           PLATFORM=macos-x86_64;   DLL=librofdconvert.dylib ;;
  *:x86_64|*:amd64)    PLATFORM=windows-x86_64; DLL=rofdconvert.dll ;;
  *) echo "FAIL: unsupported local platform for act 2 staging"; exit 1 ;;
esac
[ -f "$ROOT/target/release/$DLL" ] || { echo "FAIL: $DLL not found in $ROOT/target/release"; exit 1; }

# --- CLI reference PDF -------------------------------------------------
REF="$ROOT/target/demo-cli-reference.pdf"
echo "=== CLI reference: $CLI_BIN -> $REF (--font simsun.ttc) ==="
"$CLI_BIN" "$FIXTURE" "$REF" --font "$FONT"
command -v sha256sum >/dev/null 2>&1 && sha256sum "$REF" || true

# --- compile the demo (Java 8) -----------------------------------------
DEMO_SRC="$HERE/io/github/officers/demo"
DEMO_CLASSES="$HERE/classes"
rm -rf "$DEMO_CLASSES"
mkdir -p "$DEMO_CLASSES"

HAVE_MVN=0
command -v mvn >/dev/null 2>&1 && HAVE_MVN=1

# Thin jar via plain `mvn package` (no -Dnative.dir -> the embed-natives
# profile stays inactive). Purge any natives/ left in target/classes by a
# previous fat packaging, or the "thin" jar would silently contain the
# resource and act 1 would no longer exercise the library-path tier.
# Without mvn, compile the L1/L2 sources alongside the demo instead (the
# rword posture) so act 1 still runs.
JAR=""
if [ "$HAVE_MVN" = "1" ]; then
  rm -rf "$CRATE/target/classes/natives"
  ( cd "$CRATE" && mvn -q -DskipTests package )
  # Discover the produced jar instead of hardcoding its name — the pom
  # version moves with the release lockstep. Exactly one match is
  # required: zero = the build produced no jar, more than one = stale
  # jars of other versions; both fail fast instead of racing the pick.
  JAR_CANDIDATES=( "$CRATE"/target/rofd-convert-*.jar )
  if [ ! -f "${JAR_CANDIDATES[0]}" ] || [ "${#JAR_CANDIDATES[@]}" -ne 1 ]; then
    echo "FAIL: expected exactly one target/rofd-convert-*.jar under $CRATE/target, found: ${JAR_CANDIDATES[*]}"
    exit 1
  fi
  JAR="${JAR_CANDIDATES[0]}"
  # javac defaults to the platform encoding (GBK on zh-CN Windows) and the
  # source is UTF-8 — the flag is mandatory on that class of machines.
  javac --release 8 -encoding UTF-8 -cp "$JAR" -d "$DEMO_CLASSES" "$DEMO_SRC/InteropMain.java"
else
  echo "NOTE: mvn absent — compiling L1/L2 sources with the demo (no thin jar)"
  # mapfile keeps every hit a single argument; quoting a bare
  # $(find ...) would glue all paths into one newline-joined word.
  mapfile -t DEMO_SOURCES < <(find "$CRATE/java" "$DEMO_SRC" -name '*.java')
  javac --release 8 -encoding UTF-8 -d "$DEMO_CLASSES" "${DEMO_SOURCES[@]}"
fi

# git-bash hands POSIX-style ';'-separated classpath lists to java.exe
# UNCONVERTED (single-path args are path-converted; lists are not), so
# the Windows branch builds the two-entry classpath with cygpath.
# Linux/macOS keep the native ':'-form.
classpath_of() {
  case "$(uname -s)" in
    *MINGW*|*CYGWIN*|*MSYS*) printf '%s;%s' "$(cygpath -w "$1")" "$(cygpath -w "$2")" ;;
    *)                       printf '%s:%s' "$1" "$2" ;;
  esac
}

# --- ACT 1: library-path tier ------------------------------------------
# Ambient-override guard (both acts): the java child runs with
# ROFD_NATIVE_PATH unset (env -u), and rofd.native.path is passed
# EXPLICITLY EMPTY — the loader counts blank as unset, and a command-line
# -D beats one injected via JAVA_TOOL_OPTIONS — so NativeLoader's tier 1
# can never be satisfied from the environment and each act exercises
# exactly its own tier.
echo "=== ACT 1: thin jar + -Djava.library.path=target/release ==="
if [ -n "$JAR" ]; then
  env -u ROFD_NATIVE_PATH java -Drofd.native.path= \
    -Djava.library.path="$ROOT/target/release" \
    -cp "$(classpath_of "$JAR" "$DEMO_CLASSES")" \
    io.github.officers.demo.InteropMain "$FIXTURE" "$FONT" "$REF" "ACT 1"
else
  env -u ROFD_NATIVE_PATH java -Drofd.native.path= \
    -Djava.library.path="$ROOT/target/release" \
    -cp "$DEMO_CLASSES" \
    io.github.officers.demo.InteropMain "$FIXTURE" "$FONT" "$REF" "ACT 1"
fi
echo "=== ACT 1 OK ==="

# --- ACT 2: zero-config fat jar -----------------------------------------
if [ "$HAVE_MVN" != "1" ]; then
  echo "=== ACT 2 SKIPPED (mvn absent) ==="
  exit 0
fi
echo "=== ACT 2: zero-config fat jar (native embedded, no library path) ==="
STAGE="$HERE/native-stage"
rm -rf "$STAGE"
mkdir -p "$STAGE/$PLATFORM"
cp "$ROOT/target/release/$DLL" "$STAGE/$PLATFORM/"
( cd "$CRATE" && mvn -q -DskipTests package \
    -Dnative.dir="$STAGE" -Dskip.natives.check=true )
# Same jar path as act 1 — the fat packaging above overwrote the thin jar
# (classes + the embedded natives/ tree). NO -Djava.library.path, NO
# rofd.native.path (same ambient-override guard as act 1): the embedded
# native must load via the resource tier.
env -u ROFD_NATIVE_PATH java -Drofd.native.path= \
  -cp "$(classpath_of "$JAR" "$DEMO_CLASSES")" \
  io.github.officers.demo.InteropMain "$FIXTURE" "$FONT" "$REF" "ACT 2"
echo "=== ACT 2 OK — PHASE-2 DEMO PASS ==="
