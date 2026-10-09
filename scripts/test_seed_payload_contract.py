#!/usr/bin/env python3
"""PAYLOAD_CONTRACT in seed_vtn.py is the table docs/reference/WIRE_PROFILE.md publishes.

Run: python -m unittest scripts/test_seed_payload_contract.py
The Rust side is pinned to the same document by lab-core's
`wire_contract::tests::the_table_matches_the_published_profile`.
"""
import os
import re
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))
import seed_vtn  # noqa: E402

PROFILE = os.path.join(os.path.dirname(__file__), "..", "docs", "reference", "WIRE_PROFILE.md")


def published_contract():
    """{payload type: {"units": ..., "currency": ...}} from every table row of the profile."""
    out = {}
    with open(PROFILE, encoding="utf-8") as f:
        for line in f:
            if not line.startswith("| `"):
                continue
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            entry = {}
            units = re.match(r"`([A-Z]+)`", cells[2])
            if units:
                entry["units"] = units.group(1)
            currency = re.search(r"currency: ([A-Z]+)", cells[2])
            if currency:
                entry["currency"] = currency.group(1)
            for payload_type in re.findall(r"`([A-Z_]+)`", cells[0]):
                out[payload_type] = entry
    return out


class SeedContract(unittest.TestCase):
    def test_every_seeded_type_is_published_with_the_same_unit(self):
        published = published_contract()
        self.assertGreaterEqual(len(published), 20, "the profile's tables were not parsed")
        for payload_type, declared in seed_vtn.PAYLOAD_CONTRACT.items():
            self.assertIn(payload_type, published, f"{payload_type} is seeded but not in WIRE_PROFILE.md")
            self.assertEqual(declared, published[payload_type], payload_type)


if __name__ == "__main__":
    unittest.main()
