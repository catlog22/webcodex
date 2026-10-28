from __future__ import annotations

import copy
import io
import json
import subprocess
import unittest
from unittest.mock import Mock, patch

from scripts import upstream_sync_pr as sync


REPO = {"full_name": sync.TARGET, "fork": True, "default_branch": "main",
        "parent": {"full_name": sync.UPSTREAM}}
PR = {"number": 9, "html_url": "https://github.com/catlog22/webcodex/pull/9",
      "head": {"ref": "main", "repo": {"full_name": sync.UPSTREAM}},
      "base": {"ref": "main", "repo": {"full_name": sync.TARGET}}}


class UpstreamSyncTests(unittest.TestCase):
    def request(self, *, ahead=2, behind=3, prs=None, post_error=None):
        def call(endpoint, **kwargs):
            if endpoint == f"repos/{sync.TARGET}":
                return copy.deepcopy(REPO)
            if "/compare/" in endpoint:
                return {"status": "diverged" if ahead and behind else "ahead" if ahead else "behind",
                        "ahead_by": ahead, "behind_by": behind}
            if "?state=open" in endpoint:
                return copy.deepcopy(prs or [])
            if kwargs.get("payload"):
                if post_error:
                    raise post_error
                return {"number": PR["number"], "html_url": PR["html_url"]}
            self.fail(f"Unexpected API call: {endpoint}")
        return Mock(side_effect=call)

    def test_local_only_commits_never_create_pr(self):
        request = self.request(ahead=0, behind=8)
        self.assertEqual(sync.sync(request), {"action": "up_to_date"})
        self.assertEqual(request.call_count, 2)

    def test_divergence_creates_only_manual_review_pr(self):
        request = self.request()
        self.assertEqual(sync.sync(request)["action"], "created")
        writes = [call for call in request.call_args_list if "payload" in call.kwargs]
        self.assertEqual(len(writes), 1)
        self.assertEqual(writes[0].args, (f"repos/{sync.TARGET}/pulls",))
        payload = writes[0].kwargs["payload"]
        self.assertEqual((payload["head"], payload["base"]), (sync.HEAD, "main"))
        self.assertIs(payload["maintainer_can_modify"], False)
        self.assertNotIn("auto_merge", payload)

    def test_existing_conflicted_pr_is_reused_without_edits(self):
        pr = {**PR, "mergeable": False, "title": "Human edited title"}
        request = self.request(prs=[pr])
        self.assertEqual(sync.sync(request)["action"], "existing")
        self.assertFalse(any("payload" in call.kwargs for call in request.call_args_list))

    def test_dry_run_is_read_only(self):
        request = self.request()
        self.assertEqual(sync.sync(request, dry_run=True)["action"], "would_create")
        self.assertFalse(any("payload" in call.kwargs for call in request.call_args_list))

    def test_wrong_fork_fails_before_writes(self):
        for field, value in [("full_name", "other/repo"), ("fork", False),
                             ("default_branch", "dev"), ("parent", {"full_name": "other/repo"})]:
            request = Mock(return_value={**REPO, field: value})
            with self.subTest(field=field), self.assertRaises(ValueError):
                sync.sync(request)
            self.assertEqual(request.call_count, 1)

    def test_ambiguous_or_mismatched_prs_fail_closed(self):
        for prs in [[PR, PR], [{**PR, "head": {"ref": "other", "repo": {"full_name": sync.UPSTREAM}}}]]:
            request = self.request(prs=prs)
            with self.subTest(prs=prs), self.assertRaises(ValueError):
                sync.sync(request)
            self.assertFalse(any("payload" in call.kwargs for call in request.call_args_list))

    def test_uncertain_create_reconciles_without_second_post(self):
        request = self.request(post_error=subprocess.TimeoutExpired("gh", 45))
        original = request.side_effect
        reads = 0

        def call(endpoint, **kwargs):
            nonlocal reads
            if "?state=open" in endpoint:
                reads += 1
                return [] if reads == 1 else [PR]
            return original(endpoint, **kwargs)

        request.side_effect = call
        self.assertEqual(sync.sync(request)["action"], "reconciled")
        self.assertEqual(sum("payload" in call.kwargs for call in request.call_args_list), 1)

    def test_unreconciled_create_failure_is_reported_without_retry(self):
        request = self.request(post_error=subprocess.CalledProcessError(1, "gh"))
        with self.assertRaises(subprocess.CalledProcessError):
            sync.sync(request)
        self.assertEqual(sum("payload" in call.kwargs for call in request.call_args_list), 1)

    def test_actions_rejects_wrong_target_ref_or_event(self):
        env = {"GITHUB_ACTIONS": "true", "GITHUB_REPOSITORY": sync.TARGET,
               "GITHUB_REF": "refs/heads/main", "GITHUB_EVENT_NAME": "workflow_dispatch"}
        for field, value in [("GITHUB_REPOSITORY", "other/repo"), ("GITHUB_REF", "refs/heads/dev"),
                             ("GITHUB_EVENT_NAME", "pull_request")]:
            with self.subTest(field=field), patch.dict("os.environ", {**env, field: value}), \
                    patch("sys.argv", ["upstream_sync_pr.py"]), patch("sys.stderr", new_callable=io.StringIO), \
                    patch.object(sync, "sync") as run:
                with self.assertRaises(SystemExit):
                    sync.main()
                run.assert_not_called()

    def test_api_uses_fixed_host_structured_json_and_bounded_timeout(self):
        payload = {"title": "literal $() `text`\nsecond line", "head": sync.HEAD}
        with patch.object(sync.subprocess, "run", return_value=Mock(stdout='{"number":9}')) as run:
            self.assertEqual(sync.api("repos/example/pulls", payload=payload), {"number": 9})
        args, kwargs = run.call_args
        self.assertEqual(args[0][:4], ["gh", "api", "--hostname", "github.com"])
        self.assertEqual(json.loads(kwargs["input"]), payload)
        self.assertEqual(kwargs["timeout"], 45)
        self.assertNotIn("shell", kwargs)


if __name__ == "__main__":
    unittest.main()
