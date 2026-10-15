"""Exercise the runner against a subprocess with the pinned CLI's output contract."""

import json
import os
import shlex
import struct
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNNER = ROOT / "scripts/fuzz.py"
USIZE_MAX = (1 << (8 * struct.calcsize("P"))) - 1

# A separate executable exercises argv, environment, process exit status, combined
# stdout/stderr, and artifacts. No production parser expressions are reused here.
FAKE_CLI = r"""
import json
import os
import sys
from pathlib import Path

args = sys.argv[1:]
scenario = os.environ.get("FUZZ_TEST_SCENARIO", "success")
record = {
    "args": args,
    "cwd": str(Path.cwd()),
    "environment": {
        key: value for key, value in os.environ.items()
        if key.startswith("TREE_SITTER_") or key in ("HTTPS_PROXY", "NO_COLOR", "XDG_CACHE_HOME")
    },
}
with open(os.environ["FUZZ_TEST_CALLS"], "a", encoding="utf-8") as calls:
    calls.write(json.dumps(record) + "\n")
if args == ["--version"]:
    print("tree-sitter 0.28.0" if scenario == "wrong_version" else "tree-sitter 0.27.1")
    sys.exit(0)
if scenario == "empty":
    print()
    sys.exit(0)
if scenario == "missing_corpus":
    print("Error: No corpus directory found, ensure that you have a `test/corpus` directory "
          "in your grammar directory with at least one test file.", file=sys.stderr)
    sys.exit(0)

seed = int(os.environ["TREE_SITTER_SEED"])
iterations = int(args[args.index("--iterations") + 1])
print(f"  start seed: {seed}", file=sys.stderr)
if scenario == "no_cases":
    sys.exit(0)
if scenario == "skipped":
    print("  0. powershell - corpus - expressions - A skipped test - SKIPPED")
    sys.exit(0)
if scenario == "initial_mismatch":
    print("  0. powershell - corpus - expressions - A test")
    print("Incorrect initial parse for powershell - corpus - expressions - A test")
    print("(program (unexpected))")
    print("1 powershell corpus tests failed fuzzing", file=sys.stderr)
    sys.exit(0)

for case, name in enumerate(("A test", "Another test")):
    # Intentionally leave stdout buffered while stderr traces are emitted first.
    print(f"  {case}. powershell - corpus - expressions - {name}")
    count = iterations
    if scenario == "zero_trials":
        count = 0
    elif scenario == "partial_trials":
        count -= 1
    for trial in range(count):
        trial_seed = seed + trial + (1 if scenario == "wrong_seed" else 0)
        print(f"   {case}.{trial:<2} seed: {trial_seed}", file=sys.stderr)
if scenario == "mismatch":
    print(f"Incorrect parse for powershell - corpus - expressions - A test - seed {seed}")
    print("1 powershell corpus tests failed fuzzing", file=sys.stderr)
elif scenario == "scope_failure":
    print(f"Error: Unexpected scope change in seed {seed} with start seed {seed}",
          file=sys.stderr)
elif scenario == "allocation_failure":
    print("Error: 1 allocations leaked", file=sys.stderr)
elif scenario == "unknown":
    print("The CLI changed its success output")
elif scenario == "mixed_skip":
    print("  2. powershell - corpus - expressions - Skipped test - SKIPPED")
elif scenario == "duplicate_trial":
    print(f"   0.0  seed: {seed}", file=sys.stderr)
sys.exit(7 if scenario == "process_failure" else 0)
"""


class FuzzRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.bin_dir = self.directory / "bin"
        self.bin_dir.mkdir()
        fake = self.bin_dir / "tree-sitter"
        fake.write_text(f"#!{sys.executable}\n" + textwrap.dedent(FAKE_CLI), encoding="utf-8")
        fake.chmod(0o755)
        self.calls_path = self.directory / "calls.jsonl"
        self.logs = self.directory / "logs"
        self.env = os.environ.copy()
        self.env.update(
            {
                "PATH": str(self.bin_dir) + os.pathsep + self.env.get("PATH", ""),
                "FUZZ_TEST_CALLS": str(self.calls_path),
                "FUZZ_TEST_SCENARIO": "success",
            }
        )

    def run_fuzz(self, *args, scenario="success", defaults=False):
        self.env["FUZZ_TEST_SCENARIO"] = scenario
        command = [sys.executable, str(RUNNER), "--log-dir", str(self.logs)]
        if not defaults:
            command.extend(["--seed", "17", "--iterations", "2", "--edits", "3"])
        command.extend(args)
        return subprocess.run(
            command,
            cwd=self.directory,
            env=self.env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            encoding="utf-8",
            check=False,
            timeout=15,
        )

    def artifacts(self):
        directories = list(self.logs.iterdir())
        self.assertEqual(len(directories), 1)
        run_dir = directories[0]
        return run_dir, json.loads((run_dir / "settings.json").read_text(encoding="utf-8"))

    def calls(self):
        return [
            json.loads(line) for line in self.calls_path.read_text(encoding="utf-8").splitlines()
        ]

    def test_success_verifies_trials_and_preserves_combined_output(self):
        result = self.run_fuzz()
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertIn("Fuzz passed: 2 cases, 4 trials.", result.stdout)
        self.assertIn("Another test", result.stdout)
        self.assertNotIn("0.0  seed:", result.stdout)
        run_dir, settings = self.artifacts()
        log = (run_dir / "combined.log").read_text(encoding="utf-8")
        self.assertIn("  0. powershell - corpus - expressions - A test", log)
        self.assertIn("   0.0  seed: 17", log)
        self.assertIn("   1.1  seed: 18", log)
        self.assertEqual(settings["status"], "passed")
        self.assertEqual(settings["cases"], 2)
        self.assertEqual(settings["trials"], 4)
        self.assertEqual((run_dir / "version.log").read_text(), "tree-sitter 0.27.1\n")

    def test_exit_zero_failures_and_incomplete_runs_fail(self):
        scenarios = (
            "mismatch",
            "initial_mismatch",
            "scope_failure",
            "allocation_failure",
            "missing_corpus",
            "empty",
            "no_cases",
            "skipped",
            "mixed_skip",
            "zero_trials",
            "partial_trials",
            "wrong_seed",
            "duplicate_trial",
            "unknown",
        )
        for scenario in scenarios:
            with self.subTest(scenario=scenario):
                self.logs = self.directory / scenario
                result = self.run_fuzz(scenario=scenario)
                self.assertEqual(result.returncode, 1, result.stdout)
                self.assertIn("Fuzz failed:", result.stdout)
                run_dir, settings = self.artifacts()
                self.assertEqual(settings["returncode"], 0)
                self.assertEqual(settings["status"], "failed")
                self.assertTrue(settings["problems"])
                self.assertTrue((run_dir / "combined.log").is_file())
                self.assertTrue((run_dir / "replay.txt").is_file())
                self.assertNotIn("Fuzz passed:", result.stdout)

    def test_process_failure_fails_even_with_complete_success_output(self):
        result = self.run_fuzz(scenario="process_failure")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("status 7", result.stdout)
        _, settings = self.artifacts()
        self.assertEqual(settings["returncode"], 7)
        self.assertEqual(settings["status"], "failed")

    def test_wrong_cli_version_stops_before_fuzzing(self):
        result = self.run_fuzz(scenario="wrong_version")
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("expected tree-sitter 0.27.1", result.stdout)
        self.assertEqual(len(self.calls()), 1)
        run_dir, settings = self.artifacts()
        self.assertEqual(settings["status"], "failed")
        self.assertIn("0.28.0", (run_dir / "version.log").read_text())

    def test_missing_executable_is_a_reported_failure(self):
        self.env["PATH"] = str(self.directory / "empty-bin")
        result = self.run_fuzz()
        self.assertEqual(result.returncode, 1, result.stdout)
        self.assertIn("Fuzz failed:", result.stdout)
        _, settings = self.artifacts()
        self.assertEqual(settings["status"], "failed")
        self.assertTrue(settings["problems"])

    def test_command_environment_and_quoted_replay(self):
        removed = (
            "TREE_SITTER_LOG",
            "TREE_SITTER_LOG_GRAPHS",
            "TREE_SITTER_DUMP_EDITS",
            "TREE_SITTER_EXAMPLE_INCLUDE",
            "TREE_SITTER_EXAMPLE_EXCLUDE",
            "TREE_SITTER_LANGUAGE",
            "TREE_SITTER_LIBDIR",
        )
        self.env.update(dict.fromkeys(removed, "inherited-value"))
        self.env.update(
            {
                "TREE_SITTER_SEED": "999",
                "TREE_SITTER_ITERATIONS": "0",
                "TREE_SITTER_EDITS": "0",
                "TREE_SITTER_JS_RUNTIME": "node",
                "TREE_SITTER_LOG_SEED": "",
                "HTTPS_PROXY": "http://example.test/proxy",
            }
        )
        include = r"^corpus - quotes - (single's|double\"s)$"
        result = self.run_fuzz("--include", include)
        self.assertEqual(result.returncode, 0, result.stdout)
        version, fuzz = self.calls()
        self.assertEqual(version["args"], ["--version"])
        self.assertEqual(
            fuzz["args"],
            ["fuzz", "--rebuild", "--iterations", "2", "--edits", "3", "--include", include],
        )
        self.assertEqual(fuzz["cwd"], str(ROOT))
        env = fuzz["environment"]
        self.assertTrue(set(removed).isdisjoint(env))
        self.assertEqual(env["TREE_SITTER_JS_RUNTIME"], "native")
        self.assertEqual(env["TREE_SITTER_SEED"], "17")
        self.assertEqual(env["TREE_SITTER_ITERATIONS"], "2")
        self.assertEqual(env["TREE_SITTER_EDITS"], "3")
        self.assertEqual(env["TREE_SITTER_LOG_SEED"], "1")
        self.assertEqual(env["NO_COLOR"], "1")
        self.assertEqual(env["HTTPS_PROXY"], "http://example.test/proxy")
        run_dir, settings = self.artifacts()
        self.assertEqual(env["XDG_CACHE_HOME"], str(run_dir / "cache"))
        self.assertNotIn("HTTPS_PROXY", settings["environment_overrides"])
        replay = (run_dir / "replay.txt").read_text(encoding="utf-8").splitlines()
        self.assertEqual(shlex.split(replay[0]), ["cd", str(ROOT)])
        self.assertEqual(
            shlex.split(replay[1]),
            [
                "python3",
                "scripts/fuzz.py",
                "--seed",
                "17",
                "--iterations",
                "2",
                "--edits",
                "3",
                "--log-dir",
                str(self.logs),
                "--include",
                include,
            ],
        )

    def test_default_budget_is_bounded_and_recorded(self):
        result = self.run_fuzz(defaults=True)
        self.assertEqual(result.returncode, 0, result.stdout)
        _, settings = self.artifacts()
        self.assertEqual((settings["seed"], settings["iterations"], settings["edits"]), (0, 100, 3))
        self.assertEqual(settings["include"], None)
        self.assertEqual(settings["trials"], 200)

    def test_invalid_budgets_are_rejected_before_starting_cli(self):
        invalid = (
            ("--seed", "-1"),
            ("--seed", str(USIZE_MAX + 1)),
            ("--seed", str(USIZE_MAX)),
            ("--iterations", "0"),
            ("--iterations", "-1"),
            ("--iterations", "many"),
            ("--iterations", str(USIZE_MAX + 1)),
            ("--edits", "0"),
            ("--edits", "-1"),
        )
        for args in invalid:
            with self.subTest(args=args):
                result = self.run_fuzz(*args)
                self.assertEqual(result.returncode, 2, result.stdout)
                self.assertIn("error:", result.stdout)
        self.assertFalse(self.calls_path.exists())
        self.assertFalse(self.logs.exists())

    def test_last_platform_seed_is_valid_for_one_iteration(self):
        result = self.run_fuzz("--seed", str(USIZE_MAX), "--iterations", "1")
        self.assertEqual(result.returncode, 0, result.stdout)
        _, settings = self.artifacts()
        self.assertEqual(settings["seed"], USIZE_MAX)
        self.assertEqual(settings["trials"], 2)


if __name__ == "__main__":
    unittest.main()
