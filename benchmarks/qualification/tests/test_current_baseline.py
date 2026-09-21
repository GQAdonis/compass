from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest

from benchmarks.qualification.run_current_baseline import (
    bounded_capture,
    percentile95,
    query_commands,
    raw_traversal_summary,
    require_medium_baseline_profile,
    repository_relative,
    validated_git_oid,
    validated_graph_contract,
    validated_sha256,
    workspace_patch_metadata,
    workspace_source_metadata,
)


class CurrentBaselineTests(unittest.TestCase):
    def test_nearest_rank_p95_is_deterministic(self) -> None:
        self.assertEqual(percentile95([5, 1, 4, 2, 3]), 5)
        self.assertEqual(percentile95(list(range(1, 21))), 19)
        with self.assertRaisesRegex(ValueError, "at least five"):
            percentile95([1, 2, 3, 4])

    def test_command_matrix_covers_ratified_query_classes(self) -> None:
        commands = query_commands(Path("/bin/compass"), Path("graph.json"), Path("cache"))
        self.assertEqual(
            set(commands),
            {"search", "callers", "callees", "impact-depth-3", "path-depth-3"},
        )
        for command in commands.values():
            self.assertIn("graph.json", command)
        path_command = commands["path-depth-3"]
        self.assertIn("qualification::Node0000009", path_command)
        self.assertEqual(
            commands["search"][commands["search"].index("--max-candidates") + 1],
            "256",
        )

    def test_reproduction_helpers_emit_portable_raw_contract(self) -> None:
        with tempfile.TemporaryDirectory() as directory_text:
            root = Path(directory_text)
            artifact = root / "target" / "artifact"
            artifact.parent.mkdir()
            artifact.write_text("evidence", encoding="utf-8")
            self.assertEqual(
                repository_relative(artifact.resolve(), root.resolve(), label="artifact"),
                Path("target/artifact"),
            )
            with self.assertRaisesRegex(ValueError, "inside repository root"):
                repository_relative(Path("/").resolve(), root.resolve(), label="artifact")

            raw = root / "raw.json"
            raw.write_text(
                json.dumps(
                    {
                        "schema": "compass.qualification-raw-traversal/1",
                        "oracleStatus": "PASS",
                        "elapsedMicroseconds": 123,
                        "limits": {"maxDepth": 32},
                        "results": [
                            {"id": f"task-{index}", "elapsedMicroseconds": index + 1}
                            for index in range(30)
                        ],
                    }
                ),
                encoding="utf-8",
            )
            summary = raw_traversal_summary(
                raw,
                measurement={"wallMicroseconds": 456, "peakRssBytes": 789},
                command=("/usr/bin/python3", "raw_traversal.py"),
            )
            self.assertEqual(summary["oracleStatus"], "PASS")
            self.assertEqual(summary["taskCount"], 30)
            self.assertEqual(summary["command"][0], "python3")
            self.assertEqual(
                summary["commandInterpreter"],
                {
                    "recordedLabel": "python3",
                    "measuredExecutableName": "python3",
                    "versionField": "host.python",
                },
            )
            self.assertEqual(summary["wallMicroseconds"], 456)
            self.assertEqual(summary["peakRssBytes"], 789)
            self.assertEqual(validated_sha256("a" * 64, label="digest"), "a" * 64)
            with self.assertRaisesRegex(ValueError, "lowercase SHA-256"):
                validated_sha256("A" * 64, label="digest")
            self.assertEqual(validated_git_oid("b" * 40), "b" * 40)
            self.assertEqual(validated_git_oid("c" * 64), "c" * 64)
            with self.assertRaisesRegex(ValueError, "40- or 64-hex"):
                validated_git_oid("d" * 39)

    def test_workspace_patch_identity_is_bounded_and_reproducible(self) -> None:
        root = Path(__file__).resolve().parents[3]
        with tempfile.TemporaryDirectory() as directory_text:
            work_dir = Path(directory_text)
            first = workspace_patch_metadata(
                root, ("Cargo.toml", "Cargo.lock"), work_dir=work_dir
            )
            second = workspace_patch_metadata(
                root, ("Cargo.toml", "Cargo.lock"), work_dir=work_dir
            )
            self.assertEqual(first, second)
            self.assertEqual(len(first["workspacePatchSha256"]), 64)
            source = workspace_source_metadata(
                root, ("Cargo.toml", "Cargo.lock"), work_dir=work_dir
            )
            self.assertEqual(source["workspaceTreeFiles"], 2)
            self.assertEqual(source["workspaceTrackedFiles"], 2)
            self.assertEqual(source["workspaceUntrackedFiles"], 0)
            self.assertEqual(
                source["workspaceTreeFiles"],
                source["workspaceTrackedFiles"] + source["workspaceUntrackedFiles"],
            )
            self.assertEqual(
                source["workspaceTreePolicy"],
                "existing tracked plus non-ignored untracked files in scope",
            )
            self.assertEqual(len(source["workspaceTreeSha256"]), 64)
            self.assertEqual(len(source["workspaceTrackedSha256"]), 64)
            self.assertEqual(len(source["workspaceUntrackedSha256"]), 64)
            with self.assertRaisesRegex(ValueError, "repository-relative"):
                workspace_patch_metadata(root, ("../outside",), work_dir=work_dir)

    def test_raw_summary_rejects_incomplete_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory_text:
            path = Path(directory_text) / "raw.json"
            path.write_text(
                json.dumps(
                    {
                        "schema": "compass.qualification-raw-traversal/1",
                        "oracleStatus": "PASS",
                        "elapsedMicroseconds": 1,
                        "limits": {},
                        "results": [{} for _ in range(30)],
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ValueError, "requires non-negative integer"):
                raw_traversal_summary(
                    path,
                    measurement={"wallMicroseconds": 1, "peakRssBytes": 1},
                    command=("python3", "raw_traversal.py"),
                )

    def test_graph_contract_matches_actual_file_and_pinned_profile(self) -> None:
        with tempfile.TemporaryDirectory() as directory_text:
            directory = Path(directory_text)
            graph = directory / "graph.json"
            graph.write_text("{}\n", encoding="utf-8")
            digest = hashlib.sha256(graph.read_bytes()).hexdigest()
            metadata = directory / "generation.json"
            metadata.write_text(
                json.dumps(
                    {
                        "schema": "compass.qualification-graph-generator/1",
                        "profile": "qualification-medium",
                        "nodes": 100_000,
                        "edges": 250_000,
                        "nodeRecordsSha256": "1" * 64,
                        "edgeRecordsSha256": "2" * 64,
                        "graphBytes": graph.stat().st_size,
                        "graphSha256": digest,
                    }
                ),
                encoding="utf-8",
            )
            pinned = directory / "digests.json"
            pinned.write_text(
                json.dumps(
                    {
                        "schema": "compass.qualification-profile-digests/1",
                        "profiles": [
                            {
                                "name": "qualification-medium",
                                "nodes": 100_000,
                                "edges": 250_000,
                                "nodeRecordsSha256": "1" * 64,
                                "edgeRecordsSha256": "2" * 64,
                            }
                        ],
                    }
                ),
                encoding="utf-8",
            )
            contract = validated_graph_contract(
                graph, metadata_path=metadata, scale_digests_path=pinned
            )
            self.assertEqual(contract["nodes"], 100_000)
            self.assertEqual(contract["sha256"], digest)
            require_medium_baseline_profile(contract)
            with self.assertRaisesRegex(ValueError, "qualification-medium"):
                require_medium_baseline_profile({"profile": "qualification-large"})
            graph.write_text("changed\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "graph metadata does not match"):
                validated_graph_contract(
                    graph, metadata_path=metadata, scale_digests_path=pinned
                )

    def test_bounded_capture_rejects_oversized_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory_text:
            directory = Path(directory_text)
            with self.assertRaisesRegex(ValueError, "maximum is 8"):
                bounded_capture(
                    (sys.executable, "-c", "print('0123456789')"),
                    repository_root=directory,
                    work_dir=directory,
                    name="oversized",
                    timeout_seconds=5,
                    max_output_bytes=8,
                )

    def test_retained_baseline_is_complete_and_portable(self) -> None:
        path = Path(__file__).resolve().parents[1] / "current-engine-baseline-v1.json"
        document = json.loads(path.read_text(encoding="utf-8"))
        self.assertEqual(document["graph"]["nodes"], 100_000)
        self.assertEqual(document["graph"]["edges"], 250_000)
        self.assertFalse(Path(document["binary"]["path"]).is_absolute())
        self.assertFalse(Path(document["graph"]["path"]).is_absolute())
        self.assertEqual(
            set(document),
            {"binary", "graph", "host", "measurement", "rawTraversal", "recordedAt", "schema", "source"},
        )
        self.assertIn("exact three-hop shortest path", document["measurement"]["pathBound"])
        workloads = document["measurement"]["workloads"]
        self.assertEqual(
            set(workloads),
            {"cold-start", "search", "callers", "callees", "impact-depth-3", "path-depth-3"},
        )
        for workload in workloads.values():
            self.assertFalse(Path(workload["command"][0]).is_absolute())
            samples = workload["samples"]
            self.assertEqual(len(samples), 5)
            self.assertEqual(
                workload["p95WallMicroseconds"],
                percentile95([sample["wallMicroseconds"] for sample in samples]),
            )
            self.assertEqual(len({sample["stdoutSha256"] for sample in samples}), 1)
        self.assertEqual(document["rawTraversal"]["oracleStatus"], "PASS")
        self.assertEqual(document["rawTraversal"]["taskCount"], 30)
        self.assertEqual(document["rawTraversal"]["command"][0], "python3")
        self.assertEqual(
            document["rawTraversal"]["commandInterpreter"]["versionField"],
            "host.python",
        )
        self.assertEqual(len(document["source"]["gitHead"]), 40)
        self.assertEqual(len(document["source"]["workspacePatchSha256"]), 64)
        self.assertGreater(document["source"]["workspaceTreeFiles"], 0)
        self.assertGreater(document["source"]["workspaceTrackedFiles"], 0)
        self.assertGreater(document["source"]["workspaceUntrackedFiles"], 0)
        self.assertEqual(
            document["source"]["workspaceTreeFiles"],
            document["source"]["workspaceTrackedFiles"]
            + document["source"]["workspaceUntrackedFiles"],
        )
        self.assertEqual(len(document["source"]["workspaceTreeSha256"]), 64)
        self.assertEqual(len(document["source"]["workspaceTrackedSha256"]), 64)
        self.assertEqual(len(document["source"]["workspaceUntrackedSha256"]), 64)


if __name__ == "__main__":
    unittest.main()
