#!/usr/bin/env python3
"""Plant known bugs in a kernel crate, one at a time, and check that its tests catch each one.

Each crate's bugs are listed in `tools/planted-bugs/<crate>.py` as `BUGS`, a list of
`(name, file, old, new)`: the file is relative to the crate's directory, and `old` must occur in
it exactly once. For every bug, the runner replaces `old` with `new`, runs the crate's tests,
restores the file, and reports:

- CAUGHT    some test failed, as it should (the failing tests are listed);
- MISSED    every test passed with the bug in place;
- STALE     `old` isn't in the file exactly once: the list needs updating;
- NO BUILD  the planted bug doesn't compile, so it proves nothing: fix the bug's text;
- SKIPPED   with `--props`, a bug only unit tests can catch.

With `--props`, only the crate's property tests run (the integration tests in `tests/`): unit
tests often catch a bug that a property test's generators never reach, so each is checked alone.
A bug that only a unit test can catch, such as a database setting nothing outside the crate can
observe, ends with a fifth element, "unit", and a comment saying why; `--props` skips it.
With `--ignored`, tests marked `#[ignore]`, such as exhaustive sweeps, run too, as they do in CI.

A bug list may also set `ALSO`, other crates whose tests exercise the crate and run with its own
(keel-sim's simulations, for keel-sync), and `KNOWN_ANSWERS`, the crate's integration tests that
hold known answers rather than properties, which `--props` leaves out.

The runner edits source files in place and restores them from memory, even when interrupted.
Don't edit or build the crate while it runs, or run it in a separate worktree with `--root`.
If a run is killed outright, `git diff` shows the planted bug; `git checkout` the file.

    python3 tools/planted-bugs/run.py keel-domain            # every bug, the whole test suite
    python3 tools/planted-bugs/run.py keel-domain --props    # every bug, property tests only
    python3 tools/planted-bugs/run.py keel-domain comp       # bugs whose name mentions "comp"
"""

import argparse
import os
import pathlib
import re
import runpy
import signal
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("crate", help="the crate whose bugs to plant, such as keel-domain")
    parser.add_argument("filters", nargs="*", help="only bugs whose name contains one of these")
    parser.add_argument("--props", action="store_true", help="run only the property tests")
    parser.add_argument("--ignored", action="store_true",
                        help="also run tests marked #[ignore], such as exhaustive sweeps")
    parser.add_argument("--cases", type=int, default=500, help="proptest cases (default 500)")
    parser.add_argument("--root", type=pathlib.Path, default=HERE.parent.parent,
                        help="the repository to plant bugs in (default: this one)")
    return parser.parse_intermixed_args()


def test_command(crate_dir, crate, props, ignored, also=(), known_answers=()):
    command = ["cargo", "test", "-p", crate, "--no-fail-fast"]
    for other in also:
        command += ["-p", other]
    if props:
        targets = sorted(path.stem for path in (crate_dir / "tests").glob("*.rs")
                         if path.stem not in known_answers)
        for other in also:
            targets += sorted(path.stem for path in (crate_dir.parent / other / "tests").glob("*.rs"))
        if not targets:
            sys.exit(f"{crate} has no property tests in tests/")
        for target in targets:
            command += ["--test", target]
    if ignored:
        command += ["--", "--include-ignored"]
    return command


def failing_tests(output):
    """The names of the tests that failed, from cargo's `---- name stdout ----` headers."""
    return sorted({match.group(1) for match in re.finditer(r"^---- (\S+) stdout ----$", output, re.M)})


def unit_only(bug):
    """Whether only unit tests can catch `bug`: it ends with "unit"."""
    return len(bug) == 5 and bug[4] == "unit"


def run_bug(root, crate_dir, command, env, bug):
    name, file, old, new = bug[:4]
    path = crate_dir / file
    source = path.read_text()
    count = source.count(old)
    if count != 1:
        return "STALE", f"the text to replace occurs {count} times in {file}"
    try:
        path.write_text(source.replace(old, new))
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True)
    finally:
        # Writing the file back gives it a new modification time, so Cargo rebuilds it.
        path.write_text(source)
    output = result.stdout + result.stderr
    if result.returncode == 0:
        return "MISSED", ""
    failed = failing_tests(output)
    if failed:
        return "CAUGHT", ", ".join(failed)
    errors = [line.strip() for line in output.splitlines() if line.startswith("error")]
    return "NO BUILD", (errors[0] if errors else "the tests failed without a failing test")


def main():
    args = parse_args()
    root = args.root.resolve()
    crate_dir = root / "core" / "crates" / args.crate
    bug_file = HERE / f"{args.crate}.py"
    if not crate_dir.is_dir():
        sys.exit(f"no crate at {crate_dir}")
    if not bug_file.is_file():
        sys.exit(f"no bug list at {bug_file}")
    listed = runpy.run_path(str(bug_file))
    bugs = listed["BUGS"]
    malformed = [bug[0] for bug in bugs if len(bug) != 4 and not unit_only(bug)]
    if malformed:
        sys.exit(f"a bug is (name, file, old, new), with \"unit\" after if only unit tests can "
                 f"catch it: {malformed}")
    names = [bug[0] for bug in bugs]
    repeated = {name for name in names if names.count(name) > 1}
    if repeated:
        sys.exit(f"bug names must be unique: {sorted(repeated)}")
    if args.filters:
        bugs = [bug for bug in bugs if any(text in bug[0] for text in args.filters)]

    # Restore the file being mutated even on Ctrl-C or `kill`: both raise KeyboardInterrupt,
    # which the `finally` in run_bug handles.
    signal.signal(signal.SIGTERM, signal.default_int_handler)

    command = test_command(crate_dir, args.crate, args.props, args.ignored,
                           listed.get("ALSO", ()), listed.get("KNOWN_ANSWERS", ()))
    env = {
        **os.environ,
        "PROPTEST_CASES": str(args.cases),
        # Planted bugs mustn't leave their failure seeds in the regression files.
        "PROPTEST_DISABLE_FAILURE_PERSISTENCE": "1",
        "PROPTEST_MAX_SHRINK_ITERS": "50",
        "CARGO_TERM_COLOR": "never",
    }
    scope = "its property tests" if args.props else "all its tests"
    print(f"{len(bugs)} planted bugs in {args.crate}, against {scope}", flush=True)
    tally = {}
    for bug in bugs:
        if args.props and unit_only(bug):
            status, detail = "SKIPPED", "only unit tests can catch it"
        else:
            status, detail = run_bug(root, crate_dir, command, env, bug)
        tally[status] = tally.get(status, 0) + 1
        print(f"{status:8} {bug[0]}" + (f": {detail}" if detail else ""), flush=True)
    summary = ", ".join(f"{count} {status.lower()}" for status, count in sorted(tally.items()))
    print(f"{summary or 'no bugs'}", flush=True)
    return 0 if set(tally) <= {"CAUGHT", "SKIPPED"} else 1


if __name__ == "__main__":
    sys.exit(main())
