#!/usr/bin/env python3
"""Open one upstream review PR without merging or writing Git refs."""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys


TARGET = "catlog22/webcodex"
UPSTREAM = "yyjeqhc/webcodex"
HEAD = "yyjeqhc:main"


def api(endpoint: str, *, payload: dict | None = None, projection: str = "."):
    command = ["gh", "api", "--hostname", "github.com", endpoint, "--jq", projection]
    if payload is not None:
        command += ["--method", "POST", "--input", "-"]
    result = subprocess.run(
        command, input=json.dumps(payload) if payload is not None else None,
        capture_output=True, text=True, encoding="utf-8", timeout=45, check=True,
    )
    return json.loads(result.stdout)


def existing_pr(request) -> dict | None:
    prs = request(
        f"repos/{TARGET}/pulls?state=open&base=main&head={HEAD}&per_page=2",
        projection='[.[] | {number,html_url,head:{ref:.head.ref,repo:{full_name:.head.repo.full_name}},base:{ref:.base.ref,repo:{full_name:.base.repo.full_name}}}]',
    )
    if len(prs) > 1:
        raise ValueError("Multiple upstream PRs require manual reconciliation")
    if not prs:
        return None
    pr = prs[0]
    if (pr["head"]["repo"]["full_name"] != UPSTREAM or pr["head"]["ref"] != "main"
            or pr["base"]["repo"]["full_name"] != TARGET or pr["base"]["ref"] != "main"):
        raise ValueError("Unexpected upstream PR identity")
    return {"number": pr["number"], "url": pr["html_url"]}


def sync(request=api, *, dry_run: bool = False) -> dict:
    repo = request(f"repos/{TARGET}", projection="{full_name,fork,default_branch,parent:{full_name:.parent.full_name}}")
    if (repo["full_name"] != TARGET or repo["fork"] is not True
            or repo["default_branch"] != "main" or repo["parent"]["full_name"] != UPSTREAM):
        raise ValueError("Repository is not the configured upstream fork")
    comparison = request(
        f"repos/{TARGET}/compare/main...{HEAD}?per_page=1",
        projection="{status,ahead_by,behind_by}",
    )
    if comparison["status"] not in {"identical", "ahead", "behind", "diverged"}:
        raise ValueError("Unexpected comparison status")
    if comparison["ahead_by"] == 0:
        return {"action": "up_to_date"}
    current = existing_pr(request)
    if current:
        return {"action": "existing", **current}
    if dry_run:
        return {"action": "would_create", **comparison}
    payload = {
        "title": "chore: 同步上游 main（人工审核）",
        "head": HEAD,
        "base": "main",
        "maintainer_can_modify": False,
        "body": (
            "## 上游同步\n\n"
            "来源：`yyjeqhc/webcodex:main` → `catlog22/webcodex:main`。\n\n"
            "此 PR 跟随上游 main 更新。请人工审核 diff、CI 和冲突，确认保留本 fork 的"
            "工作区任务与 Skill 资源白名单改动后再合并。\n\n"
            "自动化只创建或复用 PR，不批准、不合并、不更新任何 Git 分支，也不自动解决冲突。"
            "若有冲突，请在人工维护的集成分支解决；不要覆盖本地修改。\n\n"
            "GitHub 可能要求人工批准由 GITHUB_TOKEN 创建的 PR 的 CI 运行。\n"
        ),
    }
    try:
        created = request(f"repos/{TARGET}/pulls", payload=payload, projection="{number,html_url}")
        return {"action": "created", "number": created["number"], "url": created["html_url"]}
    except (subprocess.SubprocessError, OSError, ValueError, KeyError):
        # POST 的结果可能不确定；只读取核对，禁止盲目重试创建。
        current = existing_pr(request)
        if current:
            return {"action": "reconciled", **current}
        raise


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    if os.environ.get("GITHUB_ACTIONS") == "true" and (
        os.environ.get("GITHUB_REPOSITORY") != TARGET
        or os.environ.get("GITHUB_REF") != "refs/heads/main"
        or os.environ.get("GITHUB_EVENT_NAME") not in {"schedule", "workflow_dispatch"}
    ):
        parser.error("Upstream sync only runs from the configured fork main")
    try:
        print(json.dumps(sync(dry_run=args.dry_run), ensure_ascii=False))
        return 0
    except (subprocess.SubprocessError, OSError, ValueError, KeyError, TypeError) as error:
        # 不输出包含 HTTP 响应或认证上下文的异常详情。
        print(f"Upstream PR sync failed ({type(error).__name__}); inspect repository/Actions permissions and open PRs", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
