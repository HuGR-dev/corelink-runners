import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("delivery_ledger", HERE / "delivery-ledger.py")
ledger = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ledger)


class LedgerCorruptionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.repo = HERE.parents[1]
        cls.registry = "docs/plan/2026-09-01-reconciled-dispatch-dag.md"
        cls.ids, cls.deps, _ = ledger.canonical(cls.repo, cls.registry)
        cls.ordered = sorted(cls.ids)

    def fixture(self):
        groups = [sorted(ledger.EXPECTED_SCOPE[i]) for i in range(1, 5)]
        baseline = sorted(self.ids - set().union(*map(set, groups)))
        head = ledger.git(self.repo, "rev-parse", "HEAD")
        items = []
        for ident in baseline:
            items.append({"id": ident, "sprint": 0, "state": "recorded_delivered", "implementation": "complete", "owner": "root", "dependencies": sorted(self.deps[ident]), "next_action": "historical", "blockers": [], "commits": [], "review": {"status": "pending", "commit": None, "evidence": []}, "evidence": []})
        for sprint, group in enumerate(groups, 1):
            for ident in group:
                items.append({"id": ident, "sprint": sprint, "state": "backlog", "implementation": "unknown", "owner": "root", "dependencies": sorted(self.deps[ident]), "next_action": "implement", "blockers": [], "commits": [], "review": {"status": "pending", "commit": None, "evidence": []}, "evidence": []})
        return {"schema_version": "delivery-ledger/v1", "registry": self.registry, "baseline": {"main_commit": head, "prepared_commit": head, "recorded_delivered": 16, "source": "history"}, "sprint_scope": {str(i): group for i, group in enumerate(groups, 1)}, "sprints": [{"id": i, "state": "implementation", "tip_commit": None, "ci": {"status": "not_run", "commit": None, "evidence": []}, "acceptance": {"status": "pending", "evidence": []}, "merge_commit": None} for i in range(1, 5)], "items": items, "findings": [], "activity": []}

    def validate_fixture(self, value, mode="check", target=None):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.json"
            path.write_text(json.dumps(value), encoding="utf-8")
            return ledger.validate(self.repo, path, mode, target)[0]

    def test_missing_and_duplicate_wp_rejected(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        value["items"].pop()
        errors = self.validate_fixture(value)
        self.assertTrue(any("coverage mismatch" in error for error in errors))
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        value["items"].append(dict(value["items"][0]))
        errors = self.validate_fixture(value)
        self.assertTrue(any("duplicate item" in error for error in errors))

    def test_premature_ci_and_delivery_rejected(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        errors = self.validate_fixture(value, "ready-ci", 1)
        self.assertTrue(any("implementation is not complete" in error for error in errors))
        errors = self.validate_fixture(value, "ready-delivery", 1)
        self.assertTrue(any("implementation is not complete" in error for error in errors))

    def test_bad_dependency_and_finding_rejected(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        value["items"][16]["dependencies"] = ["T99-W99"]
        value["findings"] = [{"id": "F-1", "wp": "T99-W99", "sprint": 1, "state": "open", "blocks_delivery": True}]
        errors = self.validate_fixture(value)
        self.assertTrue(any("dependencies do not match" in error for error in errors))
        self.assertTrue(any("unknown WP" in error for error in errors))
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        original = list(value["items"][16]["dependencies"])
        value["items"][16]["dependencies"] = original[:-1]
        errors = self.validate_fixture(value)
        self.assertTrue(any("dependencies do not match" in error for error in errors))

    def test_ready_ci_accepts_complete_tip_bound_sprint(self):
        value = self.fixture()
        head = ledger.git(self.repo, "rev-parse", "HEAD")
        selected = {x["id"] for x in value["items"] if x.get("sprint") == 1}
        for item in value["items"]:
            if item.get("id") in selected:
                item.update(implementation="complete", state="ready", blockers=[], commits=[head])
                item["review"] = {"status": "approved", "commit": head, "evidence": ["review"]}
        sprint = next(x for x in value["sprints"] if x["id"] == 1)
        sprint["tip_commit"] = head
        errors = self.validate_fixture(value, "ready-ci", 1)
        self.assertFalse(errors)

    def test_frozen_scope_rejects_swap(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        value["sprint_scope"]["1"][0], value["sprint_scope"]["2"][0] = value["sprint_scope"]["2"][0], value["sprint_scope"]["1"][0]
        errors = self.validate_fixture(value)
        self.assertTrue(any("frozen membership" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
