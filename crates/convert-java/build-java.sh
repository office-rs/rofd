#!/usr/bin/env bash
# Maven-free compile path for the Java L1/L2 classes (convert spec §6.4;
# Task 6 adds pom.xml, this script stays the zero-dependency local path).
#
# Compiles java/ + test/ with the pinned JDK 11 javac targeting Java 8
# bytecode (major 52, kernel D10) into the repo-root target/java-classes/
# directory (cargo's gitignored target tree), then prints how to run the
# loader test.
#
# Usage:   bash crates/convert-java/build-java.sh   (from the repo root)
# Test:    java -cp target/java-classes io.github.officers.NativeLoaderTest
set -euo pipefail

crate_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$crate_dir/../.." && pwd)"
out_dir="$repo_root/target/java-classes"

cd "$crate_dir"
mkdir -p "$out_dir"

echo "javac --release 8 -encoding UTF-8 -d target/java-classes java/**/*.java test/**/*.java"
find java test -name '*.java' -print0 \
    | xargs -0 -r javac --release 8 -encoding UTF-8 -d "$out_dir"

echo "compiled classes: $out_dir"
echo "loader test:      java -cp target/java-classes io.github.officers.NativeLoaderTest"
