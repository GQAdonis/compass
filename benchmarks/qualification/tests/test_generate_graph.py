from __future__ import annotations

import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[3]
GENERATOR = ROOT / "benchmarks" / "qualification" / "generate_graph.py"

from benchmarks.qualification.generate_graph import _profile, generation_metadata
import benchmarks.qualification.generate_graph as generator


class GenerateGraphTests(unittest.TestCase):
    def test_profiles_have_exact_ratified_counts_and_deterministic_plans(self) -> None:
        profiles = json.loads(
            (GENERATOR.with_name("profiles-v1.json")).read_text(encoding="utf-8")
        )
        counts = {
            item["name"]: (item["nodes"], item["edges"])
            for item in profiles["profiles"]
        }
        self.assertEqual(counts["qualification-medium"], (100_000, 250_000))
        self.assertEqual(counts["qualification-large"], (1_000_000, 2_500_000))
        first = subprocess.check_output(
            [sys.executable, str(GENERATOR), "--profile", "qualification-large", "--plan-only"]
        )
        second = subprocess.check_output(
            [sys.executable, str(GENERATOR), "--profile", "qualification-large", "--plan-only"]
        )
        self.assertEqual(first, second)
        plan = json.loads(first)
        self.assertEqual(plan["topology"]["parallelEdge"]["source"], 0)
        self.assertEqual(plan["topology"]["parallelEdge"]["target"], 1)

    def test_small_materialization_is_byte_deterministic(self) -> None:
        profile = {
            "schema": "compass.qualification-profiles/1",
            "profiles": [
                {
                    "name": "test-small",
                    "nodes": 32,
                    "edges": 80,
                    "sampleOrdinals": [0, 1, 31],
                }
            ],
        }
        with tempfile.TemporaryDirectory() as directory_text:
            directory = Path(directory_text)
            profile_path = directory / "profiles.json"
            profile_path.write_text(json.dumps(profile), encoding="utf-8")
            outputs = []
            for name in ("a.json", "b.json"):
                output = directory / name
                subprocess.run(
                    [
                        sys.executable,
                        str(GENERATOR),
                        "--profiles",
                        str(profile_path),
                        "--profile",
                        "test-small",
                        "--output",
                        str(output),
                    ],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
                outputs.append(output.read_bytes())
            self.assertEqual(outputs[0], outputs[1])
            self.assertEqual(
                hashlib.sha256(outputs[0]).hexdigest(),
                hashlib.sha256(outputs[1]).hexdigest(),
            )
            graph = json.loads(outputs[0])
            self.assertEqual(len(graph["nodes"]), 32)
            self.assertEqual(len(graph["links"]), 80)
            parallel = [
                edge
                for edge in graph["links"]
                if edge["source"] == "n:qualification:0000000"
                and edge["target"] == "n:qualification:0000001"
            ]
            self.assertEqual(len(parallel), 2)
            self.assertNotEqual(parallel[0]["id"], parallel[1]["id"])

    def test_interrupted_materialization_preserves_previous_graph(self) -> None:
        profile = {
            "name": "test-interruption",
            "nodes": 8,
            "edges": 16,
            "sampleOrdinals": [0, 1, 7],
        }
        with tempfile.TemporaryDirectory() as directory_text:
            directory = Path(directory_text)
            output = directory / "graph.json"
            output.write_bytes(b"previous-complete-graph\n")
            original_edge_record = generator.edge_record

            def interrupted_edge_record(index: int, nodes: int) -> dict[str, object]:
                if index == 3:
                    raise RuntimeError("simulated interruption")
                return original_edge_record(index, nodes)

            with mock.patch.object(
                generator, "edge_record", side_effect=interrupted_edge_record
            ):
                with self.assertRaisesRegex(RuntimeError, "simulated interruption"):
                    generator.write_graph(output, profile)
            self.assertEqual(output.read_bytes(), b"previous-complete-graph\n")
            self.assertEqual(list(directory.glob(".graph.json.*.tmp")), [])

    def test_complete_scale_profile_digests_match_pinned_values(self) -> None:
        profiles_path = GENERATOR.with_name("profiles-v1.json")
        expected = json.loads(
            GENERATOR.with_name("scale-profile-digests-v1.json").read_text(
                encoding="utf-8"
            )
        )
        for pinned in expected["profiles"]:
            with self.subTest(profile=pinned["name"]):
                profile = _profile(profiles_path, pinned["name"])
                actual = generation_metadata(profile, complete=True)
                self.assertEqual(actual["nodes"], pinned["nodes"])
                self.assertEqual(actual["edges"], pinned["edges"])
                self.assertEqual(
                    actual["nodeRecordsSha256"], pinned["nodeRecordsSha256"]
                )
                self.assertEqual(
                    actual["edgeRecordsSha256"], pinned["edgeRecordsSha256"]
                )

    def test_invalid_or_oversized_profiles_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory_text:
            directory = Path(directory_text)
            invalid_profiles = [
                {"name": "oversized", "nodes": 1_000_001, "edges": 2_500_000},
                {"name": "too-small", "nodes": 2, "edges": 4},
                {"name": "bad-sample", "nodes": 8, "edges": 16, "sampleOrdinals": [8]},
                {"name": "missing-nodes", "edges": 16},
            ]
            for profile in invalid_profiles:
                with self.subTest(profile=profile["name"]):
                    path = directory / f"{profile['name']}.json"
                    path.write_text(
                        json.dumps(
                            {
                                "schema": "compass.qualification-profiles/1",
                                "profiles": [profile],
                            }
                        ),
                        encoding="utf-8",
                    )
                    completed = subprocess.run(
                        [
                            sys.executable,
                            str(GENERATOR),
                            "--profiles",
                            str(path),
                            "--profile",
                            str(profile["name"]),
                            "--plan-only",
                        ],
                        check=False,
                        text=True,
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                    )
                    self.assertEqual(completed.returncode, 2)
                    self.assertNotIn("Traceback", completed.stderr)


if __name__ == "__main__":
    unittest.main()
