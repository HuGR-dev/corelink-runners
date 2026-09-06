"""Adversarial checks for the WP dispatch ownership guard."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("disjoint_plan", Path(__file__).with_name("disjoint-plan.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class OwnershipTests(unittest.TestCase):
    def setUp(self):
        self.plan = json.loads((ROOT / "docs/plan/execution/2026-09-06-disjoint-plan.json").read_text())
        self.ledger = json.loads((ROOT / self.plan["ledger"]).read_text())

    def errors(self):
        return MODULE.validate(self.plan, self.ledger)

    def test_current_plan_covers_canonical_backlog(self):
        self.assertEqual(self.errors(), [])

    def test_shared_integration_file_cannot_be_granted_to_executor(self):
        self.plan["packets"][0]["owns"].append("deploy/cloudflare/src/index.ts")
        self.assertTrue(any("write conflict" in e for e in self.errors()))

    def test_parent_grant_conflicts_even_with_intervening_lexical_path(self):
        self.plan["packets"][0]["owns"] += ["foo", "foo-extra", "foo/bar.ts"]
        self.assertTrue(any("directory/file overlap" in e for e in self.errors()))

    def test_missing_or_duplicate_wp_refuses(self):
        self.plan["packets"].append(copy.deepcopy(self.plan["packets"][0]))
        self.assertTrue(any("coverage" in e for e in self.errors()))

    def test_broad_or_escape_grants_refuse(self):
        self.plan["packets"][0]["owns"] += ["deploy/**", "../sibling/file.ts"]
        self.assertEqual(sum("exact relative file" in e for e in self.errors()), 2)

    def test_open_decision_cannot_be_dispatched(self):
        packet = next(p for p in self.plan["packets"] if p["state"] in {"ready", "active"})
        packet["decisions_closed"] = False
        self.assertTrue(any("cannot dispatch without decisions_closed" in e for e in self.errors()))

    def test_packet_cannot_expand_its_allowlist(self):
        self.plan["packets"][0]["allowed_writes"] = ["outside.ts"]
        self.assertTrue(any("exceed ownership" in e for e in self.errors()))

    def test_candidate_cannot_substitute_an_ancestor_for_dispatch_base(self):
        packet = self.plan["packets"][0]
        packet.update(state="active", dispatch_baseline="a" * 40)
        errors = MODULE.candidate_errors(self.plan, ROOT, packet["id"], "b" * 40, "c" * 40)
        self.assertTrue(any("dispatch baseline" in e for e in errors))


if __name__ == "__main__":
    unittest.main()
