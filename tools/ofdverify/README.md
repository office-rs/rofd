# rofd-ofdverify

Validates OFD packages with **ofdrw's own signature validator** — the
reference implementation of the archive chain rofd-sign writes for. It is
rofd's independent cross-check (spec 2026-11-23 §14): ofdrw parsed the
samples rofd's verification was tested against, so it is the arbiter for
whether rofd's output is acceptable to that chain. It has caught defects
rofd's own tests could not see (a seal structure ofdrw refuses, a
`FileRef` naming a directory).

Each file is checked with every validate container ofdrw ships (SESV4,
GBT35275, SESV1, Digital), the way a verifier that tries them in turn does.
A file that mixes seals with digital signatures is validated once per type,
on a copy whose signature list holds only that type's records (the list is
never protected, so the copy is legitimate); ofdrw's validator alone cannot
do it in one pass.

## Run

Needs a JDK (11+), Maven and network access to Maven Central (or a warm
`~/.m2` — then pass `-o` to Maven). From this directory:

```bash
mvn -q dependency:build-classpath -Dmdep.outputFile=cp.txt
mkdir -p out
javac -encoding UTF-8 -cp "$(cat cp.txt)" -d out src/main/java/Verify.java
java -Dfile.encoding=UTF-8 -cp "out;$(cat cp.txt)" Verify <file.ofd>...
```

(`out;` is the Windows path separator; use `out:` on macOS/Linux.)

Output, one line per file: `VALID via <container>  <file>`,
`VALID (mixed: …)  <file>`, `INVALID (…)  <file>` with the first error, or
`UNSIGNED (declares no signature list)`. Set `TRACE=1` for the stack traces.
The exit code is 0 when every file is valid, 1 otherwise.

## What it says and what it does not

- `VALID` means ofdrw's validator accepted the file: structure, seal
  consistency and the SM2 signature value.
- Known-good controls: rofd's `crates/sign/tests/fixtures/SESV4SignDoc.ofd`
  and `SESV1SignDoc.ofd`.
- A sample ofdrw cannot read at all (`DigitalSign.ofd`, the deprecated bare
  r/s container without its certificate) comes out `INVALID`; that is a
  limit of the sample, not of rofd.

See `docs/agents/sign.md` for when to run it.
