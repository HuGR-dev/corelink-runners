import importlib.util
import copy
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
        groups = [sorted(ledger.EXPECTED_SCOPE[i]) for i in range(1, 4)]
        baseline = sorted(self.ids - set().union(*map(set, groups)))
        head = ledger.git(self.repo, "rev-parse", "HEAD")
        items = []
        for ident in baseline:
            items.append({"id": ident, "sprint": 0, "state": "recorded_delivered", "implementation": "complete", "owner": "root", "dependencies": sorted(self.deps[ident]), "next_action": "historical", "blockers": [], "commits": [], "review": {"status": "pending", "commit": None, "evidence": []}, "evidence": []})
        for sprint, group in enumerate(groups, 1):
            for ident in group:
                historical_sprint = next(number for number, members in ledger.HISTORICAL_SCOPE.items() if ident in members)
                items.append({"id": ident, "sprint": sprint, "historical_sprint": historical_sprint, "state": "backlog", "implementation": "unknown", "owner": "root", "dependencies": sorted(self.deps[ident]), "next_action": "implement", "blockers": [], "commits": [], "review": {"status": "pending", "commit": None, "evidence": []}, "evidence": []})
        for item in items:
            if item["sprint"] == 0:
                item["historical_sprint"] = 0
        return {"schema_version": "delivery-ledger/v1", "registry": self.registry, "baseline": {"main_commit": head, "prepared_commit": head, "recorded_delivered": 16, "source": "history"}, "sprint_scope": {str(i): group for i, group in enumerate(groups, 1)}, "historical_sprint_scope": {str(i): sorted(ledger.HISTORICAL_SCOPE[i]) for i in range(1, 5)}, "operational_sprint_model": {"id": "fixture", "rules": "fixture", "bundles": {"1": {"bundle": "B1", "work_packages": 12, "starts_after_merge": None}, "2": {"bundle": "B2", "work_packages": 14, "starts_after_merge": "B1"}, "3": {"bundle": "B3", "work_packages": 28, "starts_after_merge": "B2"}}}, "sprints": [{"id": i, "state": "implementation", "tip_commit": None, "ci": {"status": "not_run", "commit": None, "evidence": []}, "acceptance": {"status": "pending", "evidence": []}, "merge_commit": None} for i in range(1, 4)], "items": items, "findings": [], "activity": []}

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
        value["findings"] = [{"id": "F-1", "wp": "T99-W99", "sprint": 1,
            "state": "open", "blocks_delivery": True, "severity": "high", "owner": "root",
            "summary": "fixture", "reproduction": "fixture", "acceptance": "fixture",
            "commits": [], "evidence": []}]
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

    def test_ready_ci_accepts_ancestral_review_only_commit(self):
        value, head = self.ready_fixture()
        review_only = ledger.git(self.repo, "rev-parse", "HEAD^")
        self.assertTrue(review_only)
        for item in value["items"]:
            if item.get("sprint") == 1:
                item["review"] = {
                    "status": "approved",
                    "commit": review_only,
                    "evidence": ["review-only commit"],
                }
        self.assertEqual(self.validate_fixture(value, "ready-ci", 1), [])
        value["items"][16]["review"]["commit"] = "0" * 40
        errors = self.validate_fixture(value, "ready-ci", 1)
        self.assertTrue(any("review commit is not valid" in error for error in errors))

    def test_frozen_scope_rejects_swap(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        value["sprint_scope"]["1"][0], value["sprint_scope"]["2"][0] = value["sprint_scope"]["2"][0], value["sprint_scope"]["1"][0]
        errors = self.validate_fixture(value)
        self.assertTrue(any("frozen membership" in error for error in errors))

    def ready_fixture(self, sprint_id=1):
        value = self.fixture()
        head = ledger.git(self.repo, "rev-parse", "HEAD")
        for item in value["items"]:
            if item["sprint"] == sprint_id:
                item.update(state="ready", implementation="complete", commits=[head])
                item["review"] = {"status": "approved", "commit": head, "evidence": ["fixture review"]}
        value["sprints"][sprint_id - 1]["tip_commit"] = head
        return value, head

    def test_malformed_records_fail_without_traceback(self):
        base = self.fixture()
        self.assertEqual(self.validate_fixture(base), [])
        cases = [("baseline", None), ("sprints", None), ("findings", [None]),
                 ("activity", [None]), ("items", [None])]
        for key, malformed in cases:
            with self.subTest(field=key):
                value = copy.deepcopy(base)
                value[key] = malformed
                self.assertTrue(self.validate_fixture(value))
        for key, malformed in [("dependencies", [{}]), ("sprint", None),
                               ("review", None), ("state", {})]:
            with self.subTest(item_field=key):
                value = copy.deepcopy(base)
                value["items"][0][key] = malformed
                self.assertTrue(self.validate_fixture(value))
        value = copy.deepcopy(base)
        value["sprint_scope"]["1"][0] = {}
        self.assertTrue(self.validate_fixture(value))

    def test_source_binding_and_earlier_dependencies(self):
        value, head = self.ready_fixture()
        self.assertEqual(self.validate_fixture(value, "ready-ci", 1), [])
        value["sprints"][0]["ci"] = {"status": "running", "commit": "0" * 40, "evidence": []}
        self.assertTrue(any("CI source" in e for e in self.validate_fixture(value)))
        value["sprints"][0]["ci"]["commit"] = head
        self.assertEqual(self.validate_fixture(value), [])
        value, _ = self.ready_fixture(2)
        self.assertTrue(any("is not ready" in e for e in self.validate_fixture(value, "ready-ci", 2)))

    def test_operational_sprints_cannot_gate_out_of_order(self):
        value, _ = self.ready_fixture(2)
        errors = self.validate_fixture(value, "ready-ci", 2)
        self.assertTrue(any("cannot enter a gate before sprint 1 is merged" in error for error in errors))

    def test_delivered_record_requires_complete_sprint_and_no_open_finding(self):
        value, head = self.ready_fixture()
        row = value["sprints"][0]
        row.update(state="delivered", merge_commit=head,
                   ci={"status": "passed", "commit": head, "evidence": ["fixture CI"]},
                   acceptance={"status": "passed", "evidence": ["fixture acceptance"]})
        self.assertEqual(self.validate_fixture(value), [])
        for item in value["items"]:
            if item["sprint"] == 1:
                item["state"] = "delivered"
        self.assertEqual(self.validate_fixture(value), [])
        value["findings"] = [{"id": "F-1", "wp": "T6-W4", "sprint": 1,
            "state": "open", "blocks_delivery": True, "severity": "high", "owner": "root",
            "summary": "fixture", "reproduction": "fixture", "acceptance": "fixture",
            "commits": [], "evidence": []}]
        self.assertTrue(any("open blocking finding" in e for e in self.validate_fixture(value)))
        value["findings"] = []
        next(i for i in value["items"] if i["sprint"] == 1)["implementation"] = "partial"
        self.assertTrue(any("complete implementation" in e for e in self.validate_fixture(value)))

    def test_external_dependency_and_baseline_count_cannot_disappear(self):
        value = self.fixture()
        self.assertEqual(self.validate_fixture(value), [])
        item = next(i for i in value["items"] if i["id"] == "T4-W4")
        item["dependencies"].remove("R1")
        self.assertTrue(any("dependencies do not match" in e for e in self.validate_fixture(value)))
        value = self.fixture()
        value["baseline"]["recorded_delivered"] = 0
        self.assertTrue(any("computed 16" in e for e in self.validate_fixture(value)))


if __name__ == "__main__":
    unittest.main()
