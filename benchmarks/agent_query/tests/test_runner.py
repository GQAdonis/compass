from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest

from benchmarks.agent_query.runner import (
    Anchor,
    Question,
    Repository,
    _paired_summary,
    estimate_tokens,
    graph_metrics,
    judge,
    load_suite,
)

ROOT = Path(__file__).resolve().parents[1]


def question(**overrides) -> Question:
    values = {
        "identifier": "sample",
        "kind": "explain",
        "subject": "sample",
        "compass": ("explain", "sample"),
        "graphify": ("explain", "sample"),
        "expect": "answer",
        "required": ("sample.go",),
        "required_one_of": (),
        "min_one_of": 0,
        "min_candidates": 0,
        "forbidden": (),
        "budget_tokens": 0,
        "max_follow_ups": 0,
        "judgment": "reviewed",
    }
    values.update(overrides)
    return Question(**values)


class SuiteTests(unittest.TestCase):
    def test_checked_in_suite_covers_five_languages(self) -> None:
        suite = load_suite(ROOT / "suite.toml")
        self.assertEqual(len(suite.digest), 64)
        self.assertEqual(len(suite.repositories), 5)
        self.assertEqual(
            {repository.language for repository in suite.repositories},
            {"Go", "Python", "Java", "TypeScript", "Rust"},
        )
        for repository in suite.repositories:
            self.assertEqual(len(repository.commit), 40)
            self.assertGreaterEqual(len(repository.questions), 6)
            self.assertGreaterEqual(len(repository.anchors), 3)
            for anchor in repository.anchors:
                self.assertTrue(anchor.judgment)
        kinds = {
            question.kind
            for repository in suite.repositories
            for question in repository.questions
        }
        self.assertEqual(
            kinds,
            {
                "explain",
                "explain_source",
                "brief",
                "callers",
                "brief_callers",
                "paged_callers",
                "path",
                "file_path",
                "ambiguity",
                "negative",
                "broad",
            },
        )

    def test_blackbox_suite_asks_fifty_symmetric_questions(self) -> None:
        suite = load_suite(ROOT / "suite_v2.toml")
        self.assertEqual(
            {repository.name for repository in suite.repositories},
            {"cobra", "flask", "gson", "zod", "axum"},
        )
        questions = [question for repository in suite.repositories for question in repository.questions]
        self.assertEqual(len(questions), 50)
        for repository in suite.repositories:
            self.assertEqual(len(repository.questions), 10)
            self.assertGreaterEqual(len(repository.anchors), 3)
        self.assertEqual(
            {question.kind for question in questions},
            {
                "explain",
                "explain_source",
                "callers",
                "callees",
                "impact",
                "path",
                "file_path",
                "ambiguity",
                "negative",
                "broad",
            },
        )
        for question in questions:
            # The blackbox suite never prices a Compass-only projection.
            self.assertNotIn("--brief", question.compass)
            self.assertNotIn("--format", question.compass)
            if question.kind == "explain_source":
                self.assertIn("--source", question.compass)
            if question.kind == "path" or question.kind == "file_path":
                # Compass `path` searches relationships in both directions.
                self.assertIn("--undirected", question.graphify)
            if question.kind == "impact":
                # Compass pages the bounded traversal with its cursor ledger.
                self.assertGreater(question.max_follow_ups, 0)
            if question.kind == "broad":
                self.assertGreater(question.budget_tokens, 0)


class PairedSummaryTests(unittest.TestCase):
    def observation(self, question: str, tool: str, passed: bool, tokens: int) -> dict:
        return {
            "repository": "cobra",
            "question": question,
            "kind": "callers",
            "tool": tool,
            "passed": passed,
            "totalTokens": tokens,
            "wallMs": tokens,
        }

    def test_paired_tokens_use_only_questions_both_tools_passed(self) -> None:
        observations = [
            # Both answered: this pair sets the reported medians.
            self.observation("shared", "compass", True, 400),
            self.observation("shared", "graphify", True, 100),
            # Only Compass answered: its cheap row must not enter the medians.
            self.observation("compass-only", "compass", True, 10),
            self.observation("compass-only", "graphify", False, 0),
            # Only Graphify answered.
            self.observation("graphify-only", "compass", False, 0),
            self.observation("graphify-only", "graphify", True, 20),
            # Neither answered.
            self.observation("neither", "compass", False, 0),
            self.observation("neither", "graphify", False, 0),
        ]
        summary = _paired_summary(observations)
        self.assertEqual(summary["questions"], 4)
        self.assertEqual(summary["bothPassed"], 1)
        self.assertEqual(summary["compassOnly"], 1)
        self.assertEqual(summary["graphifyOnly"], 1)
        self.assertEqual(summary["neither"], 1)
        self.assertEqual(summary["medianCompassTokens"], 400)
        self.assertEqual(summary["medianGraphifyTokens"], 100)

    def test_paired_tokens_are_zero_without_a_shared_answer(self) -> None:
        observations = [
            self.observation("a", "compass", True, 50),
            self.observation("a", "graphify", False, 5),
        ]
        summary = _paired_summary(observations)
        self.assertEqual(summary["bothPassed"], 0)
        self.assertEqual(summary["medianCompassTokens"], 0)
        self.assertEqual(summary["medianGraphifyTokens"], 0)


class EstimateTests(unittest.TestCase):
    def test_tokens_round_up_by_four_bytes(self) -> None:
        self.assertEqual(estimate_tokens(0), 0)
        self.assertEqual(estimate_tokens(1), 1)
        self.assertEqual(estimate_tokens(4), 1)
        self.assertEqual(estimate_tokens(5), 2)


class JudgeTests(unittest.TestCase):
    def test_answer_oracle(self) -> None:
        oracle = question(required=("ExecuteC", "command.go"))
        passed, failures = judge(oracle, "compass", "ExecuteC in command.go")
        self.assertTrue(passed)
        self.assertEqual(failures, ())
        passed, failures = judge(oracle, "compass", "ExecuteC only")
        self.assertFalse(passed)
        self.assertEqual(failures, ("missing 'command.go'",))

    def test_answer_oracle_accepts_one_of_alternatives(self) -> None:
        oracle = question(
            required=(),
            required_one_of=("safeParse", "validate"),
            min_one_of=1,
        )
        self.assertTrue(judge(oracle, "compass", "ZodType.validate")[0])
        passed, failures = judge(oracle, "compass", "unrelated output")
        self.assertFalse(passed)
        self.assertTrue(any("at least 1 of" in failure for failure in failures))

    def test_forbidden_anchor_fails(self) -> None:
        oracle = question(forbidden=("wrong.go",))
        passed, failures = judge(oracle, "graphify", "sample.go wrong.go")
        self.assertFalse(passed)
        self.assertEqual(failures, ("forbidden 'wrong.go'",))

    def test_pick_list_requires_enough_distinct_candidates(self) -> None:
        oracle = question(
            kind="ambiguity",
            expect="pick_list",
            required=("parse",),
            required_one_of=("core/parse.ts", "classic/parse.ts"),
            min_one_of=2,
        )
        self.assertTrue(judge(oracle, "compass", "core/parse.ts classic/parse.ts")[0])
        passed, failures = judge(oracle, "compass", "core/parse.ts only")
        self.assertFalse(passed)
        self.assertTrue(failures[0].startswith("pick list"))

    def test_pick_list_counts_compass_identifiers(self) -> None:
        oracle = question(
            kind="ambiguity",
            expect="pick_list",
            required=("fromJson",),
            required_one_of=("fromJson",),
            min_one_of=1,
            min_candidates=2,
        )
        payload = json.dumps(
            {
                "request": {"operation": "search", "operands": [{"role": "query", "value": "fromJson"}]},
                "primaryResults": [
                    {"id": "sha256:" + "a" * 64},
                    {"id": "sha256:" + "b" * 64},
                ]
            }
        )
        self.assertTrue(judge(oracle, "compass", payload)[0])
        single = json.dumps(
            {
                "request": {"operation": "search", "operands": [{"role": "query", "value": "fromJson"}]},
                "primaryResults": [{"id": "sha256:" + "a" * 64}],
            }
        )
        self.assertFalse(judge(oracle, "compass", single)[0])

    def test_negative_oracle_requires_an_explicit_no_match(self) -> None:
        oracle = question(kind="negative", expect="no_match", required=())
        self.assertTrue(judge(oracle, "compass", '{"resultState": "no_match"}')[0])
        self.assertTrue(judge(oracle, "graphify", "No matching nodes found.")[0])
        self.assertFalse(judge(oracle, "graphify", "NODE Zebra [src=a.go loc=L1")[0])


class GraphMetricTests(unittest.TestCase):
    def test_compass_metrics_find_dangling_and_duplicate_records(self) -> None:
        repository = Repository(
            name="sample",
            language="Go",
            url="https://example.invalid/sample.git",
            commit="0" * 40,
            questions=(),
            anchors=(
                Anchor(file="a.go", line=3, symbol="A", judgment="reviewed"),
                Anchor(file="b.go", line=9, symbol="B", judgment="reviewed"),
            ),
        )
        document = {
            "nodes": [
                {
                    "id": "one",
                    "source": {"file": "a.go", "startLine": 1, "endLine": 5},
                },
                {
                    "id": "one",
                    "source": {"file": "b.go", "startLine": 9, "endLine": 12},
                },
            ],
            "links": [{"source": "one", "target": "missing"}],
        }
        with tempfile.TemporaryDirectory() as directory:
            graph = Path(directory) / "graph.json"
            graph.write_text(json.dumps(document), encoding="utf-8")
            metrics = graph_metrics(repository, "compass", graph)
        self.assertEqual(metrics["nodes"], 2)
        self.assertEqual(metrics["duplicateIds"], 1)
        self.assertEqual(metrics["danglingEdges"], 1)
        self.assertEqual(metrics["sourceBackedNodes"], 2)
        self.assertEqual(metrics["anchorHits"], 2)

    def test_graphify_metrics_match_declaration_lines(self) -> None:
        repository = Repository(
            name="sample",
            language="Go",
            url="https://example.invalid/sample.git",
            commit="0" * 40,
            questions=(),
            anchors=(Anchor(file="a.go", line=42, symbol="A", judgment="reviewed"),),
        )
        document = {
            "nodes": [
                {"id": "one", "source_file": "a.go", "source_location": "L42"},
                {"id": "two", "source_file": "a.go", "source_location": "L7"},
            ],
            "links": [],
        }
        with tempfile.TemporaryDirectory() as directory:
            graph = Path(directory) / "graph.json"
            graph.write_text(json.dumps(document), encoding="utf-8")
            metrics = graph_metrics(repository, "graphify", graph)
        self.assertEqual(metrics["anchorHits"], 1)
        self.assertEqual(metrics["sourceBackedRatio"], 1.0)


if __name__ == "__main__":
    unittest.main()
