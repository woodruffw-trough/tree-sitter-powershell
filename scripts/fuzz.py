#!/usr/bin/env python3
"""Run the pinned corpus fuzzer and check its output, including exit-zero failures.

Tree-sitter 0.27.1 logs corpus failures without propagating a failing exit status.
Its seed trace is the only positive indication that random trials actually ran.
Keep that trace in the log while streaming case names and diagnostics to the console.
"""

import argparse
import datetime
import json
import os
import re
import shlex
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLI_VERSION = "0.27.1"
USIZE_MAX = (1 << (8 * struct.calcsize("P"))) - 1
CASE = re.compile(r"  ([0-9]+)\. powershell - (\S.*)")
START = re.compile(r"  start seed: ([0-9]+)")
TRIAL = re.compile(r"   ([0-9]+)\.([0-9]+)\s+seed: ([0-9]+)")
REMOVED_ENV = (
    "TREE_SITTER_LOG",
    "TREE_SITTER_LOG_GRAPHS",
    "TREE_SITTER_DUMP_EDITS",
    "TREE_SITTER_EXAMPLE_INCLUDE",
    "TREE_SITTER_EXAMPLE_EXCLUDE",
    "TREE_SITTER_LANGUAGE",
    "TREE_SITTER_LIBDIR",
)


def integer(value):
    try:
        number = int(value)
    except ValueError as error:
        raise argparse.ArgumentTypeError("must be an integer") from error
    if not 0 <= number <= USIZE_MAX:
        raise argparse.ArgumentTypeError(f"must be between 0 and {USIZE_MAX}")
    return number


def positive_integer(value):
    number = integer(value)
    if number == 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=integer, default=0)
    parser.add_argument("--iterations", type=positive_integer, default=100)
    parser.add_argument("--edits", type=positive_integer, default=3)
    parser.add_argument("--include", metavar="REGEX", help="select corpus cases for a replay")
    parser.add_argument("--log-dir", type=Path, default=ROOT / "work/fuzz")
    args = parser.parse_args(argv)
    if args.seed + args.iterations - 1 > USIZE_MAX:
        parser.error("seed + iterations - 1 must fit in the platform's unsigned pointer integer")
    return args


class OutputCheck:
    """Accept only the pinned CLI's complete, successful corpus/seed protocol.

    stdout case names may arrive after stderr trial traces due to buffering, so
    track them separately. Any additional output is a failure; this also catches
    multiline error logs and failure summaries that the CLI prints at exit zero.
    """

    def __init__(self, seed, iterations):
        self.seed = seed
        self.iterations = iterations
        self.starts = 0
        self.cases = {}
        self.trials = {}
        self.problems = []

    def problem(self, message):
        # A corrupt parser can produce a huge diff; the complete text is in the log.
        if len(self.problems) < 20:
            self.problems.append(message)

    def line(self, line):
        """Consume a line and return whether to stream it to the console."""
        line = line.rstrip("\r\n")
        if not line.strip():
            return True
        if match := START.fullmatch(line):
            self.starts += 1
            if int(match[1]) != self.seed:
                self.problem(f"unexpected start seed: {match[1]}")
            return True
        if match := TRIAL.fullmatch(line):
            case, trial, seed = map(int, match.groups())
            expected_trial = self.trials.get(case, 0)
            if trial != expected_trial or trial >= self.iterations:
                self.problem(f"unexpected trial {case}.{trial}; expected {expected_trial}")
            if seed != self.seed + trial:
                self.problem(f"unexpected seed {seed} for trial {case}.{trial}")
            self.trials[case] = expected_trial + 1
            return False
        if match := CASE.fullmatch(line):
            case, name = int(match[1]), match[2]
            if case in self.cases:
                self.problem(f"duplicate corpus case index {case}")
            self.cases[case] = name
            if name.endswith(" - SKIPPED"):
                self.problem(f"skipped corpus case: {name}")
            return True
        self.problem(f"unexpected fuzzer output: {line[:240]}")
        return True

    def finish(self, returncode):
        if returncode:
            self.problem(f"tree-sitter exited with status {returncode}")
        if self.starts != 1:
            self.problem(f"expected one start-seed trace, received {self.starts}")
        if not self.cases:
            self.problem("no corpus cases ran")
        elif set(self.cases) != set(range(len(self.cases))):
            self.problem("corpus case indices are incomplete")
        if self.trials.keys() - self.cases.keys():
            self.problem("trial traces have no corresponding corpus case")
        for case in self.cases:
            count = self.trials.get(case, 0)
            if count != self.iterations:
                self.problem(f"case {case} ran {count} of {self.iterations} requested trials")
        return self.problems


def environment(args, run_dir):
    env = os.environ.copy()
    for name in REMOVED_ENV:
        env.pop(name, None)
    overrides = {
        "TREE_SITTER_JS_RUNTIME": "native",
        "TREE_SITTER_SEED": str(args.seed),
        "TREE_SITTER_ITERATIONS": str(args.iterations),
        "TREE_SITTER_EDITS": str(args.edits),
        "TREE_SITTER_LOG_SEED": "1",
        "NO_COLOR": "1",
        # The CLI caches parsers by language; isolate simultaneous runs.
        "XDG_CACHE_HOME": str(run_dir / "cache"),
    }
    env.update(overrides)
    return env, overrides


def git_settings():
    """Record the source snapshot without saving remotes or local configuration."""
    commands = (["rev-parse", "--verify", "HEAD"], ["status", "--porcelain"])
    output = []
    try:
        for command in commands:
            result = subprocess.run(
                ["git", "--no-optional-locks", *command],
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                encoding="utf-8",
                timeout=10,
                check=False,
            )
            if result.returncode:
                return {"git_revision": None, "git_dirty": None}
            output.append(result.stdout.strip())
    except (OSError, subprocess.TimeoutExpired):
        return {"git_revision": None, "git_dirty": None}
    return {"git_revision": output[0], "git_dirty": bool(output[1])}


def main(argv=None):
    args = arguments(argv)
    log_base = args.log_dir.expanduser().resolve()
    log_base.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = Path(tempfile.mkdtemp(prefix=f"{timestamp}-seed-{args.seed}-", dir=log_base))
    command = [
        "tree-sitter",
        "fuzz",
        "--rebuild",
        "--iterations",
        str(args.iterations),
        "--edits",
        str(args.edits),
    ]
    replay = [
        "python3",
        "scripts/fuzz.py",
        "--seed",
        str(args.seed),
        "--iterations",
        str(args.iterations),
        "--edits",
        str(args.edits),
        "--log-dir",
        str(log_base),
    ]
    if args.include is not None:
        command.extend(["--include", args.include])
        replay.extend(["--include", args.include])
    replay_text = f"cd {shlex.quote(str(ROOT))}\n{shlex.join(replay)}\n"
    (run_dir / "replay.txt").write_text(replay_text, encoding="utf-8")
    env, overrides = environment(args, run_dir)
    settings = {
        "tree_sitter_version": CLI_VERSION,
        "seed": args.seed,
        "iterations": args.iterations,
        "edits": args.edits,
        "include": args.include,
        "cwd": str(ROOT),
        "command": command,
        "environment_overrides": overrides,
        "environment_removed": list(REMOVED_ENV),
        "replay": replay_text,
        "status": "running",
        **git_settings(),
    }
    settings_path = run_dir / "settings.json"

    def save_settings():
        settings_path.write_text(json.dumps(settings, indent=2) + "\n", encoding="utf-8")

    save_settings()
    print(
        f"Fuzz seed={args.seed} iterations={args.iterations} edits={args.edits}\n"
        f"Logs: {run_dir}\nReplay: {shlex.join(replay)}",
        flush=True,
    )
    check = OutputCheck(args.seed, args.iterations)
    try:
        version = subprocess.run(
            ["tree-sitter", "--version"],
            cwd=ROOT,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            timeout=10,
            check=False,
        )
        (run_dir / "version.log").write_bytes(version.stdout)
        settings["version_output"] = version.stdout.decode("utf-8", errors="replace").strip()
        if version.returncode or version.stdout.strip() != f"tree-sitter {CLI_VERSION}".encode():
            raise RuntimeError(
                f"expected tree-sitter {CLI_VERSION}, got {settings['version_output']!r} "
                f"(exit {version.returncode})"
            )
        with (run_dir / "combined.log").open("wb") as log:
            with subprocess.Popen(
                command, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
            ) as process:
                for raw_line in process.stdout:
                    log.write(raw_line)
                    line = raw_line.decode("utf-8", errors="replace")
                    if check.line(line):
                        log.flush()
                        print(line, end="", flush=True)
                returncode = process.wait()
        problems = check.finish(returncode)
        settings["returncode"] = returncode
    except (OSError, RuntimeError, subprocess.TimeoutExpired) as error:
        problems = [str(error)]
    settings["cases"] = len(check.cases)
    settings["trials"] = sum(check.trials.values())
    settings["problems"] = problems
    settings["status"] = "failed" if problems else "passed"
    save_settings()
    if problems:
        print("Fuzz failed:\n" + "\n".join(f"  {problem}" for problem in problems), file=sys.stderr)
        print(f"Full logs and replay: {run_dir}", file=sys.stderr)
        return 1
    print(f"Fuzz passed: {len(check.cases)} cases, {settings['trials']} trials.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
