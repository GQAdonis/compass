from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from benchmarks.qualification.raw_traversal import (
    MAX_TASK_BYTES,
    LimitExceeded,
    execute_task,
)


ROOT = Path(__file__).resolve().parents[3]
GENERATOR = ROOT / "benchmarks" / "qualification" / "generate_graph.py"
TRAVERSAL = ROOT / "benchmarks" / "qualification" / "raw_traversal.py"


class RawTraversalTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        profiles = self.directory / "profiles.json"
        profiles.write_text(
            json.dumps(
                {
                    "schema": "compass.qualification-profiles/1",
                    "profiles": [
                        {
                            "name": "test",
                            "nodes": 128,
                            "edges": 320,
                            "sampleOrdinals": [0, 1, 127],
                        }
                    ],
                }
            ),
            encoding="utf-8",
        )
        self.graph = self.directory / "graph.json"
        subprocess.run(
            [
                sys.executable,
                str(GENERATOR),
                "--profiles",
                str(profiles),
                "--profile",
                "test",
                "--output",
                str(self.graph),
            ],
            check=True,
            stdout=subprocess.DEVNULL,
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def run_tasks(
        self,
        tasks: list[dict[str, object]],
        *extra: str,
        graph: Path | None = None,
        declared_limits: dict[str, object] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        task_path = self.directory / "tasks.json"
        document: dict[str, object] = {"tasks": tasks}
        if declared_limits is not None:
            document["limits"] = declared_limits
        task_path.write_text(json.dumps(document), encoding="utf-8")
        return subprocess.run(
            [
                sys.executable,
                str(TRAVERSAL),
                "--graph",
                str(self.graph if graph is None else graph),
                "--tasks",
                str(task_path),
                *extra,
            ],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def test_search_direction_parallel_impact_and_path(self) -> None:
        completed = self.run_tasks(
            [
                {"id": "search", "operation": "search", "query": "Node0000127", "expected": {"status": "complete"}},
                {"id": "callers", "operation": "callers", "source": "qualification::Node0000001", "expected": {"status": "complete"}},
                {"id": "callees", "operation": "callees", "source": "qualification::Node0000000", "expected": {"status": "complete"}},
                {"id": "impact", "operation": "impact", "source": "qualification::Node0000003", "maxDepth": 3, "expected": {"status": "complete"}},
                {"id": "path", "operation": "path", "source": "qualification::Node0000000", "target": "qualification::Node0000003", "expected": {"status": "complete"}},
            ]
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        results = {item["id"]: item["result"] for item in json.loads(completed.stdout)["results"]}
        self.assertEqual(results["search"]["nodeIds"], ["n:qualification:0000127"])
        self.assertEqual(results["callers"]["nodeIds"].count("n:qualification:0000000"), 2)
        self.assertEqual(results["callees"]["nodeIds"].count("n:qualification:0000001"), 2)
        self.assertEqual(results["path"]["status"], "complete")
        self.assertIn("n:qualification:0000000", results["impact"]["nodeIds"])

    def test_limit_exhaustion_is_an_error_not_empty(self) -> None:
        completed = self.run_tasks(
            [{"id": "search", "operation": "search", "query": "Node", "expected": {"status": "complete"}}],
            "--max-results",
            "4",
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("exceed", completed.stderr)
        self.assertEqual(completed.stdout, "")

    def test_malformed_task_is_a_clean_input_error(self) -> None:
        completed = self.run_tasks([{"id": "missing-query", "operation": "search"}])
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires a non-empty string query", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

        completed = self.run_tasks(["not-an-object"])  # type: ignore[list-item]
        self.assertEqual(completed.returncode, 2)
        self.assertIn("must be an object", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

        completed = self.run_tasks(
            [{"id": "missing-expected", "operation": "search", "query": "Node0000001"}]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires an expected evidence object", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

        completed = self.run_tasks(
            [{"id": "null-expected", "operation": "search", "query": "Node0000001", "expected": None}]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("requires an expected evidence object", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

        completed = self.run_tasks(
            [
                {
                    "id": "invalid-expected",
                    "operation": "search",
                    "query": "Node0000001",
                    "expected": {"status": "complete", "minimumEdgeCount": True},
                }
            ]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("minimumEdgeCount must be a non-negative integer", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

        completed = self.run_tasks(
            [
                {
                    "id": "unknown-expected",
                    "operation": "search",
                    "query": "Node0000001",
                    "expected": {"status": "complete", "firstNodeID": "typo"},
                }
            ]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("unsupported expected fields", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

    def test_declared_limits_must_match_effective_limits(self) -> None:
        completed = self.run_tasks(
            [
                {
                    "id": "search",
                    "operation": "search",
                    "query": "Node0000001",
                    "expected": {"status": "complete"},
                }
            ],
            declared_limits={
                "maxDepth": 31,
                "maxEdges": 2_500_000,
                "maxNodes": 1_000_000,
                "maxResultsPerTask": 10_000,
                "timeoutSeconds": 120,
            },
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("does not match effective value", completed.stderr)

    def test_tasks_document_size_is_bounded(self) -> None:
        completed = self.run_tasks(
            [
                {
                    "id": "oversized",
                    "operation": "search",
                    "query": "Node0000001",
                    "prompt": "x" * MAX_TASK_BYTES,
                    "expected": {"status": "complete"},
                }
            ]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("tasks document", completed.stderr)
        self.assertIn("maximum", completed.stderr)

    def test_empty_task_suite_is_rejected(self) -> None:
        completed = self.run_tasks([])
        self.assertEqual(completed.returncode, 2)
        self.assertIn("task count must be in", completed.stderr)

    def test_duplicate_edge_identity_is_rejected(self) -> None:
        document = json.loads(self.graph.read_text(encoding="utf-8"))
        document["links"][1]["id"] = document["links"][0]["id"]
        duplicate_graph = self.directory / "duplicate-edge.json"
        duplicate_graph.write_text(json.dumps(document), encoding="utf-8")
        completed = self.run_tasks(
            [{"id": "search", "operation": "search", "query": "Node0000001", "expected": {"status": "complete"}}],
            graph=duplicate_graph,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("duplicate edge ID", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

    def test_non_string_graph_identity_is_rejected(self) -> None:
        document = json.loads(self.graph.read_text(encoding="utf-8"))
        document["nodes"][0]["id"] = 7
        malformed_graph = self.directory / "non-string-node.json"
        malformed_graph.write_text(json.dumps(document), encoding="utf-8")
        completed = self.run_tasks(
            [
                {
                    "id": "search",
                    "operation": "search",
                    "query": "Node0000001",
                    "expected": {"status": "complete"},
                }
            ],
            graph=malformed_graph,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("node id must be a non-empty string", completed.stderr)
        self.assertNotIn("Traceback", completed.stderr)

    def test_scan_timeout_is_checked_inside_search(self) -> None:
        nodes = {
            f"n:{index}": (f"Node{index}", f"qualification::Node{index}")
            for index in range(2048)
        }
        with self.assertRaisesRegex(LimitExceeded, "time limit"):
            execute_task(
                {"id": "timeout", "operation": "search", "query": "missing"},
                nodes,
                {},
                {},
                max_depth=1,
                max_results=10,
                deadline=0.0,
            )

    def test_expected_evidence_mismatch_is_an_input_failure(self) -> None:
        completed = self.run_tasks(
            [
                {
                    "id": "wrong-expected",
                    "operation": "search",
                    "query": "Node0000001",
                    "expected": {
                        "status": "complete",
                        "containsNodeIds": ["n:qualification:does-not-exist"],
                    },
                }
            ]
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("missing expected node evidence", completed.stderr)

    def test_empty_search_has_explicit_empty_status(self) -> None:
        completed = self.run_tasks(
            [
                {
                    "id": "empty-search",
                    "operation": "search",
                    "query": "not-present",
                    "expected": {"status": "empty"},
                }
            ]
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        result = json.loads(completed.stdout)["results"][0]["result"]
        self.assertEqual(result, {"nodeIds": [], "status": "empty"})

    def test_output_is_published_atomically(self) -> None:
        output = self.directory / "nested" / "result.json"
        completed = self.run_tasks(
            [{"id": "search", "operation": "search", "query": "Node0000001", "expected": {"status": "complete"}}],
            "--output",
            str(output),
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertEqual(completed.stdout, "")
        self.assertEqual(json.loads(output.read_text())["oracleStatus"], "PASS")
        self.assertEqual(list(output.parent.glob(f".{output.name}.*.tmp")), [])


if __name__ == "__main__":
    unittest.main()
