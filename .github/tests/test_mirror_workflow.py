"""Offline mirror caller checks against its pinned sandbox workflow.

Run: python3 .github/tests/test_mirror_workflow.py /path/to/sandbox-checkout
Requires PyYAML; the sandbox checkout must already contain the pinned commit.
No fetch, registry call, workflow dispatch, or credentials are required.
"""

import os
from pathlib import Path
import subprocess
import sys
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
if len(sys.argv) != 2:
    raise SystemExit("usage: test_mirror_workflow.py /path/to/sandbox-checkout")
SANDBOX = Path(sys.argv.pop()).resolve()
CALLER = yaml.safe_load((ROOT / ".github/workflows/mirror-ecr.yml").read_text())


def triggers(workflow):
    # PyYAML's YAML 1.1 resolver treats the unquoted Actions key "on" as True.
    return workflow.get("on", workflow.get(True))


class MirrorWorkflowContract(unittest.TestCase):
    def test_pinned_interface_and_permissions(self):
        mirror = CALLER["jobs"]["mirror"]
        reference = mirror["uses"]
        prefix = "gauransh/itsm-sandbox-policy-engine/.github/workflows/mirror-ecr.yml@"
        self.assertTrue(reference.startswith(prefix))
        revision = reference.removeprefix(prefix)
        self.assertRegex(revision, r"^[0-9a-f]{40}$")
        content = subprocess.run(
            ["git", "-C", str(SANDBOX), "show",
             f"{revision}:.github/workflows/mirror-ecr.yml"],
            check=True, capture_output=True, text=True,
        ).stdout
        callee = yaml.safe_load(content)
        interface = triggers(callee)["workflow_call"]
        for section, supplied in (("inputs", mirror["with"]),
                                  ("secrets", mirror["secrets"])):
            declared = interface[section]
            self.assertLessEqual(set(supplied), set(declared))
            required = {key for key, value in declared.items()
                        if value.get("required") is True}
            self.assertLessEqual(required, set(supplied))
        self.assertTrue(all(interface["inputs"][key]["type"] == "string"
                            for key in mirror["with"]))
        self.assertTrue(all(isinstance(value, str)
                            for value in mirror["with"].values()))
        self.assertEqual(mirror["with"]["images"], "session-capture")
        self.assertEqual(mirror["with"]["tag"], "${{ inputs.tag }}")
        self.assertEqual(mirror["with"]["aws_account_id"], "335971291626")
        self.assertEqual(mirror["permissions"], {
            "contents": "read", "packages": "read", "id-token": "write",
        })
        self.assertEqual(mirror["secrets"], {
            "aws_role_to_assume": "${{ secrets.AWS_ECR_MIRROR_ROLE_ARN }}",
            "ecr_password": "${{ secrets.ECR_MIRROR_TOKEN }}",
            "ghcr_token": "${{ secrets.GITHUB_TOKEN }}",
        })

    def test_validation_precedes_credentials(self):
        jobs = CALLER["jobs"]
        self.assertEqual(set(jobs), {"validate", "mirror"})
        self.assertEqual(jobs["mirror"]["needs"], "validate")
        self.assertNotIn("if", jobs["mirror"])
        self.assertEqual(jobs["validate"]["permissions"], {})
        steps = jobs["validate"]["steps"]
        self.assertEqual(len(steps), 1)
        self.assertEqual(steps[0]["env"], {"TAG": "${{ inputs.tag }}"})
        self.assertEqual(steps[0]["shell"], "bash")
        self.assertNotIn("${{", steps[0]["run"])
        self.assertEqual(set(triggers(CALLER)), {"workflow_dispatch"})

    def test_tag_boundaries_and_untrusted_text(self):
        script = CALLER["jobs"]["validate"]["steps"][0]["run"]
        accepted = ["sha-1adbcd0", "sha-" + "a" * 40]
        rejected = ["", "main", "sha-123456", "sha-" + "a" * 41,
                    "sha-ABCDEF0", "sha-123456g", "sha-1234567\n",
                    " sha-1234567", "sha-1234567; true", "$(exit 0)",
                    "sha-1234567/other", "sha-1234567 --tag other"]
        for tag in accepted + rejected:
            with self.subTest(tag=tag):
                result = subprocess.run(
                    ["bash", "--noprofile", "--norc", "-c", script],
                    env={"PATH": os.defpath, "TAG": tag},
                    capture_output=True, text=True, timeout=5,
                )
                self.assertEqual(result.returncode, 0 if tag in accepted else 1)
                if tag in rejected:
                    self.assertIn("::error::tag must be", result.stdout)


if __name__ == "__main__":
    unittest.main()
