from __future__ import annotations

from collections import Counter
import hashlib
import json
from pathlib import Path
import unittest

from benchmarks.qualification.raw_traversal import verify_expected


ROOT = Path(__file__).resolve().parents[3]
QUALIFICATION = ROOT / "benchmarks" / "qualification"


class QualificationCorporaTests(unittest.TestCase):
    def test_manifest_is_closed_and_all_digests_match(self) -> None:
        manifest = json.loads(
            (QUALIFICATION / "manifest-v1.json").read_text(encoding="utf-8")
        )
        self.assertEqual(manifest["schema"], "compass.qualification-manifest/1")
        expected_paths = {
            path.relative_to(ROOT).as_posix()
            for path in QUALIFICATION.rglob("*")
            if path.is_file()
            and path.name != "manifest-v1.json"
            and "__pycache__" not in path.parts
        }
        entries = manifest["artifacts"]
        self.assertEqual([entry["path"] for entry in entries], sorted(expected_paths))
        for entry in entries:
            path = ROOT / entry["path"]
            self.assertEqual(path.stat().st_size, entry["bytes"])
            self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), entry["sha256"])

    def test_agent_suite_is_exactly_thirty_balanced_tasks(self) -> None:
        document = json.loads(
            (QUALIFICATION / "agent-tasks-v1.json").read_text(encoding="utf-8")
        )
        tasks = document["tasks"]
        self.assertEqual(document["schema"], "compass.qualification-agent-tasks/1")
        self.assertEqual(len(tasks), 30)
        self.assertEqual(len({task["id"] for task in tasks}), 30)
        self.assertEqual(
            Counter(task["operation"] for task in tasks),
            Counter({"search": 6, "callers": 6, "callees": 6, "impact": 6, "path": 6}),
        )
        for task in tasks:
            self.assertTrue(task["prompt"].strip())
            self.assertEqual(task["expected"]["status"], "complete")

    def test_semantic_sources_match_pinned_digests(self) -> None:
        document = json.loads(
            (QUALIFICATION / "semantic-corpus-v1.json").read_text(encoding="utf-8")
        )
        for source in document["sources"]:
            actual = hashlib.sha256((ROOT / source["path"]).read_bytes()).hexdigest()
            self.assertEqual(actual, source["sha256"])
        self.assertIn("parallel_multiplicity", document["requiredDimensions"])
        self.assertIn("pagination", document["requiredDimensions"])
        semantic_source = next(
            source
            for source in document["sources"]
            if source["schema"] == "compass.code-graph-qualification/2"
        )
        semantic = json.loads(
            (ROOT / semantic_source["path"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            document["counts"],
            {
                "edgeProducers": len(semantic["edgeProducers"]),
                "flows": len(semantic["flows"]),
                "negativeCases": len(semantic["negatives"]),
                "nodeProducers": len(semantic["nodeProducers"]),
            },
        )

    def test_medium_oracle_satisfies_every_versioned_task_expectation(self) -> None:
        task_path = QUALIFICATION / "agent-tasks-v1.json"
        tasks = json.loads(task_path.read_text(encoding="utf-8"))["tasks"]
        oracle = json.loads(
            (QUALIFICATION / "raw-traversal-oracle-v1.json").read_text(encoding="utf-8")
        )
        self.assertEqual(oracle["schema"], "compass.qualification-raw-oracle/1")
        self.assertEqual(oracle["graph"]["nodes"], 100_000)
        self.assertEqual(oracle["graph"]["edges"], 250_000)
        baseline = json.loads(
            (QUALIFICATION / "current-engine-baseline-v1.json").read_text(
                encoding="utf-8"
            )
        )
        self.assertEqual(oracle["graph"]["sha256"], baseline["graph"]["sha256"])
        self.assertEqual(
            hashlib.sha256(task_path.read_bytes()).hexdigest(),
            oracle["taskSuite"]["sha256"],
        )
        results = oracle["results"]
        self.assertEqual([item["id"] for item in results], [task["id"] for task in tasks])
        for task, item in zip(tasks, results, strict=True):
            verify_expected(task, item["result"])

    def test_skill_subcorpora_cover_the_shipped_trigger_corpus_exactly(self) -> None:
        descriptor = json.loads(
            (QUALIFICATION / "skill-corpora-v1.json").read_text(encoding="utf-8")
        )
        source = ROOT / descriptor["source"]["path"]
        self.assertEqual(
            hashlib.sha256(source.read_bytes()).hexdigest(),
            descriptor["source"]["sha256"],
        )
        cases = json.loads(source.read_text(encoding="utf-8"))["cases"]
        by_id = {case["id"]: case for case in cases}
        umbrella = descriptor["umbrellaInvocations"]["caseIds"]
        focused = descriptor["focusedSkillBoundaries"]["caseIds"]
        self.assertEqual(set(umbrella) | set(focused), set(by_id))
        self.assertFalse(set(umbrella) & set(focused))
        self.assertEqual({by_id[case_id]["expected"] for case_id in umbrella}, {"compass"})
        expected_counts = Counter(by_id[case_id]["expected"] for case_id in focused)
        self.assertEqual(set(expected_counts), set(descriptor["focusedSkillBoundaries"]["expectedSkills"]))
        self.assertEqual(set(expected_counts.values()), {2})


if __name__ == "__main__":
    unittest.main()
